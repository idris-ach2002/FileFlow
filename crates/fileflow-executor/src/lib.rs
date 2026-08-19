//! Bounded job execution, process lifecycle and cancellation.
//!
//! External programs are always launched directly through `Command`; FileFlow
//! never interpolates paths into a shell command. Batch work is windowed and
//! every item must acquire a resource lease from the scheduler first.

use chrono::Utc;
use fileflow_domain::{JobId, JobState, OutputPolicy, ResourceProfile};
use fileflow_output::{OutputPlan, OutputRequest, OutputResolver};
use fileflow_scheduler::ResourceScheduler;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Instant,
};
use thiserror::Error;
use tokio::{process::Command, sync::mpsc, task::JoinSet};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionInput {
    pub path: PathBuf,
    pub source_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRequest {
    pub action_id: String,
    pub inputs: Vec<ExecutionInput>,
    pub output_policy: OutputPolicy,
    pub target_format: Option<String>,
    pub quality: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EnginePaths {
    paths: HashMap<String, PathBuf>,
}

impl EnginePaths {
    pub fn new(paths: HashMap<String, PathBuf>) -> Self {
        Self { paths }
    }

    pub fn get(&self, id: &str) -> Result<&Path, ExecutionError> {
        self.paths
            .get(id)
            .map(PathBuf::as_path)
            .ok_or_else(|| ExecutionError::MissingEngine(id.to_owned()))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemFailure {
    pub input: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSummary {
    pub job_id: JobId,
    pub action_id: String,
    pub state: JobState,
    pub total: usize,
    pub succeeded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub outputs: Vec<PathBuf>,
    pub failures: Vec<ItemFailure>,
    pub duration_ms: u64,
    pub finished_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "event", content = "data")]
pub enum ExecutionEvent {
    Started {
        job_id: JobId,
        action_id: String,
        total: usize,
    },
    ItemStarted {
        job_id: JobId,
        index: usize,
        input: PathBuf,
    },
    ItemCompleted {
        job_id: JobId,
        index: usize,
        input: PathBuf,
        output: Option<PathBuf>,
        skipped: bool,
    },
    ItemFailed {
        job_id: JobId,
        index: usize,
        input: PathBuf,
        message: String,
    },
    Progress {
        job_id: JobId,
        completed: usize,
        total: usize,
    },
    Finished {
        summary: ExecutionSummary,
    },
}

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error("action is not executable yet: {0}")]
    UnsupportedAction(String),
    #[error("required engine is missing: {0}")]
    MissingEngine(String),
    #[error("no input was provided")]
    EmptyInput,
    #[error("job was cancelled")]
    Cancelled,
    #[error("process could not be started: {0}")]
    Process(#[from] std::io::Error),
    #[error("process failed ({program}): {message}")]
    ProcessFailed { program: String, message: String },
    #[error(transparent)]
    Output(#[from] fileflow_output::OutputError),
    #[error(transparent)]
    Scheduler(#[from] fileflow_scheduler::SchedulerError),
    #[error("event consumer disconnected")]
    EventConsumerDisconnected,
    #[error("internal job task failed: {0}")]
    Join(String),
    #[error("no safe destination name is available: {0}")]
    Destination(String),
    #[error("invalid output format for {action}: {format}")]
    InvalidTargetFormat { action: String, format: String },
    #[error("archive rejected by safety checks: {0}")]
    UnsafeArchive(String),
}

#[derive(Debug, Clone)]
struct ItemResult {
    index: usize,
    input: PathBuf,
    output: Option<PathBuf>,
    skipped: bool,
}

pub struct ActionExecutor {
    scheduler: Arc<ResourceScheduler>,
    output: OutputResolver,
}

impl ActionExecutor {
    pub fn new(scheduler: Arc<ResourceScheduler>) -> Self {
        Self {
            scheduler,
            output: OutputResolver,
        }
    }

    pub async fn execute(
        &self,
        job_id: JobId,
        mut request: ExecutionRequest,
        engines: EnginePaths,
        cancellation: CancellationToken,
        events: mpsc::Sender<ExecutionEvent>,
    ) -> Result<ExecutionSummary, ExecutionError> {
        if request.inputs.is_empty() {
            return Err(ExecutionError::EmptyInput);
        }
        if !is_supported(&request.action_id) {
            return Err(ExecutionError::UnsupportedAction(request.action_id));
        }
        request.target_format = normalize_target_format(&request.action_id, request.target_format.as_deref())?;
        request.quality = normalize_quality(request.quality.as_deref());

        let started = Instant::now();
        let total = request.inputs.len();
        send(&events, ExecutionEvent::Started {
            job_id,
            action_id: request.action_id.clone(),
            total,
        }).await?;

        let summary = if is_collective(&request.action_id) {
            self.execute_collective(job_id, request, engines, cancellation, events.clone()).await?
        } else {
            self.execute_batch(job_id, request, engines, cancellation, events.clone()).await?
        };

        let summary = ExecutionSummary {
            duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            finished_at: Utc::now(),
            ..summary
        };
        send(&events, ExecutionEvent::Finished { summary: summary.clone() }).await?;
        Ok(summary)
    }

    async fn execute_batch(
        &self,
        job_id: JobId,
        request: ExecutionRequest,
        engines: EnginePaths,
        cancellation: CancellationToken,
        events: mpsc::Sender<ExecutionEvent>,
    ) -> Result<ExecutionSummary, ExecutionError> {
        let total = request.inputs.len();
        let window = self.scheduler.budget().cpu_tokens.saturating_mul(2).clamp(1, 16);
        let mut join_set = JoinSet::new();
        let mut next_index = 0_usize;
        let mut completed = 0_usize;
        let mut succeeded = 0_usize;
        let mut skipped = 0_usize;
        let mut outputs = Vec::new();
        let mut failures = Vec::new();

        while next_index < total || !join_set.is_empty() {
            while next_index < total && join_set.len() < window && !cancellation.is_cancelled() {
                let index = next_index;
                next_index += 1;
                let input = request.inputs[index].clone();
                send(
                    &events,
                    ExecutionEvent::ItemStarted {
                        job_id,
                        index,
                        input: input.path.clone(),
                    },
                )
                .await?;
                let action_id = request.action_id.clone();
                let output_policy = request.output_policy.clone();
                let target_format = request.target_format.clone();
                let quality = request.quality.clone();
                let engines = engines.clone();
                let scheduler = self.scheduler.clone();
                let cancellation = cancellation.clone();
                let output = self.output;
                join_set.spawn(async move {
                    execute_item(
                        index,
                        input,
                        &action_id,
                        &output_policy,
                        target_format.as_deref(),
                        quality.as_deref(),
                        &engines,
                        scheduler,
                        cancellation,
                        output,
                    )
                    .await
                });
            }

            if cancellation.is_cancelled() {
                join_set.abort_all();
                while join_set.join_next().await.is_some() {}
                return Ok(batch_summary(
                    job_id,
                    request.action_id,
                    JobState::Cancelled,
                    total,
                    succeeded,
                    skipped,
                    outputs,
                    failures,
                ));
            }

            let Some(joined) = join_set.join_next().await else {
                continue;
            };
            completed += 1;
            match joined {
                Ok(Ok(result)) => {
                    if result.skipped {
                        skipped += 1;
                    } else {
                        succeeded += 1;
                    }
                    if let Some(output) = result.output.clone() {
                        outputs.push(output);
                    }
                    send(
                        &events,
                        ExecutionEvent::ItemCompleted {
                            job_id,
                            index: result.index,
                            input: result.input,
                            output: result.output,
                            skipped: result.skipped,
                        },
                    )
                    .await?;
                }
                Ok(Err(ItemExecutionError {
                    error: ExecutionError::Cancelled,
                    ..
                })) => {
                    join_set.abort_all();
                    while join_set.join_next().await.is_some() {}
                    return Ok(batch_summary(
                        job_id,
                        request.action_id,
                        JobState::Cancelled,
                        total,
                        succeeded,
                        skipped,
                        outputs,
                        failures,
                    ));
                }
                Ok(Err(ItemExecutionError { index, input, error })) => {
                    failures.push(ItemFailure {
                        input: input.clone(),
                        message: error.to_string(),
                    });
                    send(
                        &events,
                        ExecutionEvent::ItemFailed {
                            job_id,
                            index,
                            input,
                            message: error.to_string(),
                        },
                    )
                    .await?;
                }
                Err(error) if error.is_cancelled() => {
                    return Ok(batch_summary(
                        job_id,
                        request.action_id,
                        JobState::Cancelled,
                        total,
                        succeeded,
                        skipped,
                        outputs,
                        failures,
                    ));
                }
                Err(error) => return Err(ExecutionError::Join(error.to_string())),
            }
            send(
                &events,
                ExecutionEvent::Progress {
                    job_id,
                    completed,
                    total,
                },
            )
            .await?;
        }

        let state = if failures.is_empty() {
            JobState::Completed
        } else {
            JobState::Failed
        };
        Ok(batch_summary(
            job_id,
            request.action_id,
            state,
            total,
            succeeded,
            skipped,
            outputs,
            failures,
        ))
    }

    async fn execute_collective(
        &self,
        job_id: JobId,
        request: ExecutionRequest,
        engines: EnginePaths,
        cancellation: CancellationToken,
        events: mpsc::Sender<ExecutionEvent>,
    ) -> Result<ExecutionSummary, ExecutionError> {
        for (index, input) in request.inputs.iter().enumerate() {
            send(
                &events,
                ExecutionEvent::ItemStarted {
                    job_id,
                    index,
                    input: input.path.clone(),
                },
            )
            .await?;
        }

        let result = execute_collective_action(
            &request,
            &engines,
            self.scheduler.clone(),
            cancellation,
            self.output,
        )
        .await;

        let total = request.inputs.len();
        let mut outputs = Vec::new();
        let mut failures = Vec::new();
        let state = match result {
            Ok(output) => {
                if let Some(path) = output.clone() {
                    outputs.push(path);
                }
                for (index, input) in request.inputs.iter().enumerate() {
                    send(
                        &events,
                        ExecutionEvent::ItemCompleted {
                            job_id,
                            index,
                            input: input.path.clone(),
                            output: output.clone(),
                            skipped: false,
                        },
                    )
                    .await?;
                }
                JobState::Completed
            }
            Err(ExecutionError::Cancelled) => JobState::Cancelled,
            Err(error) => {
                let message = error.to_string();
                failures.push(ItemFailure {
                    input: request.inputs[0].path.clone(),
                    message: message.clone(),
                });
                send(
                    &events,
                    ExecutionEvent::ItemFailed {
                        job_id,
                        index: 0,
                        input: request.inputs[0].path.clone(),
                        message,
                    },
                )
                .await?;
                JobState::Failed
            }
        };
        send(
            &events,
            ExecutionEvent::Progress {
                job_id,
                completed: if state == JobState::Cancelled { 0 } else { total },
                total,
            },
        )
        .await?;

        Ok(ExecutionSummary {
            job_id,
            action_id: request.action_id,
            state,
            total,
            succeeded: if state == JobState::Completed { total } else { 0 },
            skipped: 0,
            failed: if state == JobState::Failed { total } else { 0 },
            outputs,
            failures,
            duration_ms: 0,
            finished_at: Utc::now(),
        })
    }

}

fn batch_summary(
    job_id: JobId,
    action_id: String,
    state: JobState,
    total: usize,
    succeeded: usize,
    skipped: usize,
    outputs: Vec<PathBuf>,
    failures: Vec<ItemFailure>,
) -> ExecutionSummary {
    ExecutionSummary {
        job_id,
        action_id,
        state,
        total,
        succeeded,
        skipped,
        failed: failures.len(),
        outputs,
        failures,
        duration_ms: 0,
        finished_at: Utc::now(),
    }
}

#[derive(Debug)]
struct ItemExecutionError {
    index: usize,
    input: PathBuf,
    error: ExecutionError,
}

async fn execute_item(
    index: usize,
    input: ExecutionInput,
    action_id: &str,
    output_policy: &OutputPolicy,
    target_format: Option<&str>,
    quality: Option<&str>,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
    resolver: OutputResolver,
) -> Result<ItemResult, ItemExecutionError> {
    let result = execute_item_inner(
        &input.path,
        input.source_root.as_deref(),
        action_id,
        output_policy,
        target_format,
        quality,
        engines,
        scheduler,
        cancellation,
        resolver,
    )
    .await;
    result
        .map(|(output, skipped)| ItemResult {
            index,
            input: input.path.clone(),
            output,
            skipped,
        })
        .map_err(|error| ItemExecutionError {
            index,
            input: input.path,
            error,
        })
}

async fn execute_item_inner(
    input: &Path,
    source_root: Option<&Path>,
    action_id: &str,
    output_policy: &OutputPolicy,
    target_format: Option<&str>,
    quality: Option<&str>,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
    resolver: OutputResolver,
) -> Result<(Option<PathBuf>, bool), ExecutionError> {
    if cancellation.is_cancelled() { return Err(ExecutionError::Cancelled); }
    if action_id == "archive-extract" {
        return execute_archive_extract(input, source_root, output_policy, engines, scheduler, cancellation)
            .await
            .map(|(path, skipped)| (Some(path), skipped));
    }
    if action_id == "pdf-split" {
        return execute_pdf_split(input, source_root, output_policy, engines, scheduler, cancellation)
            .await
            .map(|(path, skipped)| (Some(path), skipped));
    }
    if action_id == "pdf-to-images" {
        return execute_pdf_to_images(input, source_root, output_policy, engines, scheduler, cancellation)
            .await
            .map(|(path, skipped)| (Some(path), skipped));
    }

    let (engine_id, extension, suffix) = item_output(action_id, input, target_format)?;
    let engine = engines.get(engine_id)?;
    let profile = profile_for(engine_id);
    let engine_threads = scheduler
        .budget()
        .cpu_tokens
        .min(usize::from(profile.cpu_weight).max(1))
        .max(1);
    let _lease = scheduler.acquire(engine_id, profile, &cancellation).await?;
    let plan = resolver.plan(&OutputRequest {
        source: input.to_path_buf(),
        source_root: source_root.map(Path::to_path_buf),
        desired_extension: extension.map(str::to_owned),
        operation_suffix: suffix.map(str::to_owned),
        policy: output_policy.clone(),
    })?;
    if plan.skipped { return Ok((Some(plan.final_path), true)); }
    resolver.prepare(&plan).await?;

    let execution = match action_id {
        "image-convert" | "image-batch-convert" => run_vips_copy(engine, input, &plan.temporary_path, engine_threads, &cancellation).await,
        "image-optimize" | "image-resize" => run_vips_thumbnail(engine, input, &plan.temporary_path, quality, engine_threads, &cancellation).await,
        "strip-metadata" => run_exiftool_strip(engine, input, &plan.temporary_path, &cancellation).await,
        "extract-metadata" => run_exiftool_json(engine, input, &plan.temporary_path, &cancellation).await,
        "office-to-pdf" => run_office_to_pdf(engine, input, &plan, &cancellation).await,
        "pdf-compress" => run_pdf_compress(engine, input, &plan.temporary_path, quality, &cancellation).await,
        "pdf-extract-text" => run_process(engine, &[input.as_os_str().into(), plan.temporary_path.as_os_str().into()], &cancellation).await,
        "pdf-ocr" => run_pdf_ocr(engine, input, &plan.temporary_path, &cancellation).await,
        "ocr-image" => run_tesseract(engine, input, &plan.temporary_path, &cancellation).await,
        "media-compatible" | "media-compress" | "audio-convert" | "extract-audio" | "video-to-gif" => run_ffmpeg(engine, action_id, input, &plan.temporary_path, quality, engine_threads, &cancellation).await,
        _ => Err(ExecutionError::UnsupportedAction(action_id.into())),
    };

    if let Err(error) = execution {
        resolver.cleanup(&plan).await;
        return Err(error);
    }
    resolver.finalize(&plan).await?;
    Ok((Some(plan.final_path), false))
}

async fn execute_collective_action(
    request: &ExecutionRequest,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
    resolver: OutputResolver,
) -> Result<Option<PathBuf>, ExecutionError> {
    match request.action_id.as_str() {
        "images-to-pdf" => {
            let engine = engines.get("img2pdf")?;
            let _lease = scheduler.acquire("img2pdf", ResourceProfile::PDF, &cancellation).await?;
            let first = &request.inputs[0].path;
            let plan = resolver.plan(&OutputRequest {
                source: first.clone(),
                source_root: None,
                desired_extension: Some("pdf".into()),
                operation_suffix: Some("images".into()),
                policy: fileflow_domain::OutputPolicy { naming: fileflow_domain::NamingStrategy::OperationSuffix, ..request.output_policy.clone() },
            })?;
            if plan.skipped { return Ok(Some(plan.final_path)); }
            resolver.prepare(&plan).await?;
            let list_path = plan.destination_directory.join(format!(".fileflow-img2pdf-{}.list", Uuid::new_v4().simple()));
            tokio::fs::write(&list_path, nul_separated_paths(&request.inputs)).await?;
            let args = [
                OsString::from("--rotation=ifvalid"),
                OsString::from("--from-file"),
                list_path.as_os_str().into(),
                OsString::from("-o"),
                plan.temporary_path.as_os_str().into(),
            ];
            if let Err(error) = run_process(engine, &args, &cancellation).await {
                let _ = tokio::fs::remove_file(&list_path).await;
                resolver.cleanup(&plan).await;
                return Err(error);
            }
            let _ = tokio::fs::remove_file(&list_path).await;
            resolver.finalize(&plan).await?;
            Ok(Some(plan.final_path))
        }
        "pdf-merge" => {
            let engine = engines.get("qpdf")?;
            let _lease = scheduler.acquire("qpdf", ResourceProfile::PDF, &cancellation).await?;
            let first = &request.inputs[0].path;
            let plan = resolver.plan(&OutputRequest {
                source: first.clone(),
                source_root: None,
                desired_extension: Some("pdf".into()),
                operation_suffix: Some("fusion".into()),
                policy: fileflow_domain::OutputPolicy { naming: fileflow_domain::NamingStrategy::OperationSuffix, ..request.output_policy.clone() },
            })?;
            if plan.skipped { return Ok(Some(plan.final_path)); }
            resolver.prepare(&plan).await?;
            let mut args = vec![OsString::from("--empty"), OsString::from("--pages")];
            for input in &request.inputs { args.push(input.path.as_os_str().into()); }
            args.extend([OsString::from("--"), plan.temporary_path.as_os_str().into()]);
            run_process(engine, &args, &cancellation).await?;
            resolver.finalize(&plan).await?;
            Ok(Some(plan.final_path))
        }
        "archive-create" => {
            let engine = engines.get("archive")?;
            let _lease = scheduler.acquire("archive", ResourceProfile::ARCHIVE, &cancellation).await?;
            let first = &request.inputs[0].path;
            let target_format = request.target_format.as_deref().unwrap_or("zip");
            let plan = resolver.plan(&OutputRequest {
                source: first.clone(),
                source_root: None,
                desired_extension: Some(target_format.into()),
                operation_suffix: Some("archive".into()),
                policy: fileflow_domain::OutputPolicy { naming: fileflow_domain::NamingStrategy::OperationSuffix, ..request.output_policy.clone() },
            })?;
            if plan.skipped { return Ok(Some(plan.final_path)); }
            resolver.prepare(&plan).await?;
            let mut args = vec![OsString::from("a"), plan.temporary_path.as_os_str().into()];
            for input in &request.inputs { args.push(input.path.as_os_str().into()); }
            run_process(engine, &args, &cancellation).await?;
            resolver.finalize(&plan).await?;
            Ok(Some(plan.final_path))
        }
        _ => Err(ExecutionError::UnsupportedAction(request.action_id.clone())),
    }
}

fn item_output<'a>(
    action_id: &str,
    input: &'a Path,
    target_format: Option<&'a str>,
) -> Result<(&'static str, Option<&'a str>, Option<&'static str>), ExecutionError> {
    let source_extension = input.extension().and_then(|value| value.to_str());
    Ok(match action_id {
        "image-convert" | "image-batch-convert" => ("vips", Some(target_format.unwrap_or("jpg")), Some("converti")),
        "image-optimize" => ("vips", Some(target_format.or(source_extension).unwrap_or("jpg")), Some("optimise")),
        "image-resize" => ("vips", Some(target_format.or(source_extension).unwrap_or("jpg")), Some("redimensionne")),
        "strip-metadata" => ("metadata", source_extension, Some("prive")),
        "extract-metadata" => ("metadata", Some("json"), Some("metadonnees")),
        "office-to-pdf" => ("office", Some("pdf"), Some("pdf")),
        "pdf-compress" => ("ghostscript", Some("pdf"), Some("leger")),
        "pdf-extract-text" => ("poppler", Some("txt"), Some("texte")),
        "pdf-ocr" => ("ocr", Some("pdf"), Some("ocr")),
        "ocr-image" => ("tesseract", Some("txt"), Some("texte")),
        "media-compatible" => ("ffmpeg", Some(if is_audio_extension(source_extension) { "m4a" } else { "mp4" }), Some("compatible")),
        "media-compress" => ("ffmpeg", source_extension.or(Some("mp4")), Some("leger")),
        "audio-convert" => ("ffmpeg", Some(target_format.unwrap_or("mp3")), Some("converti")),
        "extract-audio" => ("ffmpeg", Some(target_format.unwrap_or("m4a")), Some("audio")),
        "video-to-gif" => ("ffmpeg", Some("gif"), Some("animation")),
        _ => return Err(ExecutionError::UnsupportedAction(action_id.into())),
    })
}

fn nul_separated_paths(inputs: &[ExecutionInput]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for input in inputs {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;
            bytes.extend_from_slice(input.path.as_os_str().as_bytes());
        }
        #[cfg(not(unix))]
        {
            bytes.extend_from_slice(input.path.to_string_lossy().as_bytes());
        }
        bytes.push(0);
    }
    bytes
}

async fn run_vips_copy(
    engine: &Path,
    input: &Path,
    output: &Path,
    threads: usize,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    run_process_with_env(
        engine,
        &[OsString::from("copy"), input.as_os_str().into(), output.as_os_str().into()],
        &[("VIPS_CONCURRENCY", threads.to_string())],
        cancellation,
    )
    .await
}

async fn run_vips_thumbnail(
    engine: &Path,
    input: &Path,
    output: &Path,
    quality: Option<&str>,
    threads: usize,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let size = match quality { Some("small") => "1280", Some("high") => "2560", _ => "2048" };
    run_process_with_env(
        engine,
        &[OsString::from("thumbnail"), input.as_os_str().into(), output.as_os_str().into(), OsString::from(size), OsString::from("--size"), OsString::from("down")],
        &[("VIPS_CONCURRENCY", threads.to_string())],
        cancellation,
    )
    .await
}

async fn run_exiftool_strip(engine: &Path, input: &Path, output: &Path, cancellation: &CancellationToken) -> Result<(), ExecutionError> {
    run_process(engine, &[OsString::from("-all="), OsString::from("-o"), output.as_os_str().into(), input.as_os_str().into()], cancellation).await
}

async fn run_exiftool_json(
    engine: &Path,
    input: &Path,
    output: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let json = capture_process(
        engine,
        &[OsString::from("-j"), OsString::from("-G"), OsString::from("-n"), input.as_os_str().into()],
        cancellation,
    )
    .await?;
    tokio::fs::write(output, json).await?;
    Ok(())
}

async fn run_office_to_pdf(engine: &Path, input: &Path, plan: &OutputPlan, cancellation: &CancellationToken) -> Result<(), ExecutionError> {
    let staging = plan.destination_directory.join(format!(".fileflow-office-{}", Uuid::new_v4().simple()));
    tokio::fs::create_dir_all(&staging).await?;
    let result = run_process(engine, &[OsString::from("--headless"), OsString::from("--convert-to"), OsString::from("pdf"), OsString::from("--outdir"), staging.as_os_str().into(), input.as_os_str().into()], cancellation).await;
    if let Err(error) = result { let _ = tokio::fs::remove_dir_all(&staging).await; return Err(error); }
    let generated = staging.join(format!("{}.pdf", input.file_stem().and_then(|value| value.to_str()).unwrap_or("document")));
    if let Err(rename_error) = tokio::fs::rename(&generated, &plan.temporary_path).await {
        if let Err(copy_error) = tokio::fs::copy(&generated, &plan.temporary_path).await {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            tracing::debug!(%rename_error, "office output rename failed before copy fallback");
            return Err(ExecutionError::Process(copy_error));
        }
        let _ = tokio::fs::remove_file(&generated).await;
    }
    let _ = tokio::fs::remove_dir_all(&staging).await;
    Ok(())
}

async fn run_pdf_compress(engine: &Path, input: &Path, output: &Path, quality: Option<&str>, cancellation: &CancellationToken) -> Result<(), ExecutionError> {
    let profile = match quality { Some("small") => "/screen", Some("high") => "/prepress", _ => "/ebook" };
    run_process(engine, &[
        OsString::from("-sDEVICE=pdfwrite"), OsString::from("-dCompatibilityLevel=1.4"),
        OsString::from(format!("-dPDFSETTINGS={profile}")), OsString::from("-dNOPAUSE"), OsString::from("-dQUIET"), OsString::from("-dBATCH"),
        OsString::from(format!("-sOutputFile={}", output.to_string_lossy())), input.as_os_str().into(),
    ], cancellation).await
}

async fn run_pdf_ocr(engine: &Path, input: &Path, output: &Path, cancellation: &CancellationToken) -> Result<(), ExecutionError> {
    run_process(engine, &[OsString::from("--skip-text"), OsString::from("--deskew"), OsString::from("--optimize"), OsString::from("1"), input.as_os_str().into(), output.as_os_str().into()], cancellation).await
}

async fn run_tesseract(engine: &Path, input: &Path, output: &Path, cancellation: &CancellationToken) -> Result<(), ExecutionError> {
    let base = output.with_extension("");
    run_process(engine, &[input.as_os_str().into(), base.as_os_str().into(), OsString::from("-l"), OsString::from("fra+eng"), OsString::from("txt")], cancellation).await?;
    let generated = base.with_extension("txt");
    if generated != output { tokio::fs::rename(generated, output).await?; }
    Ok(())
}

async fn run_ffmpeg(
    engine: &Path,
    action_id: &str,
    input: &Path,
    output: &Path,
    quality: Option<&str>,
    thread_count: usize,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let thread_count = thread_count.max(1);
    let mut args = vec![
        OsString::from("-hide_banner"),
        OsString::from("-loglevel"),
        OsString::from("error"),
        OsString::from("-y"),
        OsString::from("-i"),
        input.as_os_str().into(),
        OsString::from("-threads"),
        OsString::from(thread_count.to_string()),
        OsString::from("-filter_threads"),
        OsString::from(thread_count.to_string()),
        OsString::from("-filter_complex_threads"),
        OsString::from(thread_count.to_string()),
    ];
    let source_audio = is_audio_extension(input.extension().and_then(|value| value.to_str()));

    match action_id {
        "media-compatible" => {
            if source_audio {
                args.push(OsString::from("-vn"));
                push_audio_codec(&mut args, output);
            } else {
                args.extend([
                    OsString::from("-c:v"), OsString::from("libx264"),
                    OsString::from("-preset"), OsString::from("medium"),
                    OsString::from("-crf"), OsString::from("23"),
                    OsString::from("-c:a"), OsString::from("aac"),
                    OsString::from("-b:a"), OsString::from("160k"),
                    OsString::from("-movflags"), OsString::from("+faststart"),
                ]);
            }
        }
        "media-compress" if source_audio => {
            args.extend([OsString::from("-vn"), OsString::from("-b:a")]);
            args.push(OsString::from(if quality == Some("small") { "96k" } else if quality == Some("high") { "192k" } else { "128k" }));
        }
        "media-compress" => {
            let crf = if quality == Some("small") { "30" } else if quality == Some("high") { "20" } else { "26" };
            args.extend([
                OsString::from("-c:v"), OsString::from("libx264"),
                OsString::from("-preset"), OsString::from("medium"),
                OsString::from("-crf"), OsString::from(crf),
                OsString::from("-c:a"), OsString::from("aac"),
                OsString::from("-b:a"), OsString::from("128k"),
            ]);
        }
        "audio-convert" | "extract-audio" => {
            args.push(OsString::from("-vn"));
            push_audio_codec(&mut args, output);
        }
        "video-to-gif" => {
            args.extend([
                OsString::from("-vf"),
                OsString::from("fps=12,scale=960:-1:flags=lanczos"),
            ]);
        }
        _ => {}
    }
    args.push(output.as_os_str().into());
    run_process(engine, &args, cancellation).await
}

fn push_audio_codec(args: &mut Vec<OsString>, output: &Path) {
    match output.extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("mp3") => args.extend([OsString::from("-c:a"), OsString::from("libmp3lame"), OsString::from("-q:a"), OsString::from("2")]),
        Some("opus") => args.extend([OsString::from("-c:a"), OsString::from("libopus"), OsString::from("-b:a"), OsString::from("128k")]),
        Some("ogg") => args.extend([OsString::from("-c:a"), OsString::from("libvorbis"), OsString::from("-q:a"), OsString::from("5")]),
        Some("flac") => args.extend([OsString::from("-c:a"), OsString::from("flac")]),
        Some("wav") => args.extend([OsString::from("-c:a"), OsString::from("pcm_s16le")]),
        _ => args.extend([OsString::from("-c:a"), OsString::from("aac"), OsString::from("-b:a"), OsString::from("192k")]),
    }
}

async fn execute_archive_extract(
    input: &Path,
    source_root: Option<&Path>,
    policy: &OutputPolicy,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
) -> Result<(PathBuf, bool), ExecutionError> {
    let engine = engines.get("archive")?;
    let _lease = scheduler.acquire("archive", ResourceProfile::ARCHIVE, &cancellation).await?;
    validate_archive(engine, input, &cancellation).await?;
    let plan = prepare_directory_output(input, source_root, policy, "extrait").await?;
    if plan.skipped { return Ok((plan.final_path, true)); }
    let output_arg = OsString::from(format!("-o{}", plan.temporary_path.to_string_lossy()));
    let result = run_process(
        engine,
        &[OsString::from("x"), input.as_os_str().into(), output_arg, OsString::from("-y")],
        &cancellation,
    )
    .await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_dir_all(&plan.temporary_path).await;
        return Err(error);
    }
    finalize_directory_output(&plan).await?;
    Ok((plan.final_path, false))
}

async fn execute_pdf_split(
    input: &Path,
    source_root: Option<&Path>,
    policy: &OutputPolicy,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
) -> Result<(PathBuf, bool), ExecutionError> {
    let engine = engines.get("qpdf")?;
    let _lease = scheduler.acquire("qpdf", ResourceProfile::PDF, &cancellation).await?;
    let plan = prepare_directory_output(input, source_root, policy, "pages").await?;
    if plan.skipped { return Ok((plan.final_path, true)); }
    let pattern = plan.temporary_path.join("page-%d.pdf");
    let result = run_process(
        engine,
        &[OsString::from("--split-pages"), input.as_os_str().into(), pattern.as_os_str().into()],
        &cancellation,
    )
    .await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_dir_all(&plan.temporary_path).await;
        return Err(error);
    }
    finalize_directory_output(&plan).await?;
    Ok((plan.final_path, false))
}

async fn execute_pdf_to_images(
    input: &Path,
    source_root: Option<&Path>,
    policy: &OutputPolicy,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
) -> Result<(PathBuf, bool), ExecutionError> {
    let engine = engines.get("poppler")?;
    let _lease = scheduler.acquire("poppler", ResourceProfile::PDF, &cancellation).await?;
    let plan = prepare_directory_output(input, source_root, policy, "images").await?;
    if plan.skipped { return Ok((plan.final_path, true)); }
    let prefix = plan.temporary_path.join("page");
    let result = run_process(
        engine,
        &[OsString::from("-png"), OsString::from("-r"), OsString::from("150"), input.as_os_str().into(), prefix.as_os_str().into()],
        &cancellation,
    )
    .await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_dir_all(&plan.temporary_path).await;
        return Err(error);
    }
    finalize_directory_output(&plan).await?;
    Ok((plan.final_path, false))
}

#[derive(Debug)]
struct DirectoryOutputPlan {
    final_path: PathBuf,
    temporary_path: PathBuf,
    skipped: bool,
}

async fn prepare_directory_output(
    input: &Path,
    source_root: Option<&Path>,
    policy: &OutputPolicy,
    suffix: &str,
) -> Result<DirectoryOutputPlan, ExecutionError> {
    let source_parent = input.parent().unwrap_or_else(|| Path::new("."));
    let mut parent = match policy.destination {
        fileflow_domain::DestinationPolicy::SameFolder | fileflow_domain::DestinationPolicy::AskEveryTime => source_parent.to_path_buf(),
        fileflow_domain::DestinationPolicy::Subfolder => source_parent.join(safe_folder_name(&policy.subfolder_name)),
        fileflow_domain::DestinationPolicy::CustomFolder => policy.custom_directory.clone().ok_or(fileflow_output::OutputError::MissingCustomDirectory)?,
    };
    if policy.preserve_tree {
        if let Some(root) = source_root {
            if let Ok(relative) = input.strip_prefix(root) {
                if let Some(relative_parent) = relative.parent().filter(|path| !path.as_os_str().is_empty()) {
                    parent = parent.join(relative_parent);
                }
            }
        }
    }
    tokio::fs::create_dir_all(&parent).await?;
    let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("document");
    let base = parent.join(format!("{stem}_{suffix}"));
    let target = resolve_directory_conflict(base, policy.conflict).await?;
    let skipped = target.exists() && policy.conflict == fileflow_domain::ConflictStrategy::Skip;
    if target.exists() && policy.conflict == fileflow_domain::ConflictStrategy::Replace {
        tokio::fs::remove_dir_all(&target).await?;
    }
    let temporary_path = parent.join(format!(".fileflow-{suffix}-{}", Uuid::new_v4().simple()));
    if !skipped {
        tokio::fs::create_dir_all(&temporary_path).await?;
    }
    Ok(DirectoryOutputPlan { final_path: target, temporary_path, skipped })
}

async fn finalize_directory_output(plan: &DirectoryOutputPlan) -> Result<(), ExecutionError> {
    if plan.skipped { return Ok(()); }
    tokio::fs::rename(&plan.temporary_path, &plan.final_path).await?;
    Ok(())
}

const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const MAX_ARCHIVE_UNPACKED_BYTES: u64 = 100 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_RATIO: u64 = 1_000;

async fn validate_archive(
    engine: &Path,
    input: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let output = capture_process(
        engine,
        &[OsString::from("l"), OsString::from("-slt"), input.as_os_str().into()],
        cancellation,
    )
    .await?;
    validate_archive_listing(&output)
}

fn validate_archive_listing(listing: &str) -> Result<(), ExecutionError> {
    let mut in_entries = false;
    let mut entries = 0_usize;
    let mut unpacked = 0_u64;
    let mut physical_size = 0_u64;

    for line in listing.lines() {
        let line = line.trim();
        if line.starts_with("----------") {
            in_entries = true;
            continue;
        }
        if !in_entries {
            if let Some(value) = line.strip_prefix("Physical Size = ") {
                physical_size = value.trim().parse::<u64>().unwrap_or(0);
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("Path = ") {
            entries = entries.saturating_add(1);
            if entries > MAX_ARCHIVE_ENTRIES {
                return Err(ExecutionError::UnsafeArchive(format!("plus de {MAX_ARCHIVE_ENTRIES} entrées")));
            }
            validate_archive_path(value)?;
        } else if let Some(value) = line.strip_prefix("Size = ") {
            unpacked = unpacked.saturating_add(value.trim().parse::<u64>().unwrap_or(0));
            if unpacked > MAX_ARCHIVE_UNPACKED_BYTES {
                return Err(ExecutionError::UnsafeArchive("taille décompressée supérieure à 100 Gio".into()));
            }
        } else if line.starts_with("Symbolic Link = ") || line.starts_with("Hard Link = ") {
            return Err(ExecutionError::UnsafeArchive("les liens contenus dans une archive ne sont pas extraits automatiquement".into()));
        }
    }

    if physical_size > 0 && unpacked > 1024 * 1024 * 1024 {
        let ratio = unpacked / physical_size.max(1);
        if ratio > MAX_ARCHIVE_RATIO {
            return Err(ExecutionError::UnsafeArchive(format!("ratio de compression suspect ({ratio}:1)")));
        }
    }
    Ok(())
}

fn validate_archive_path(value: &str) -> Result<(), ExecutionError> {
    let normalized = value.replace('\\', "/");
    let drive_prefix = normalized.as_bytes().get(1).is_some_and(|value| *value == b':');
    if normalized.starts_with('/')
        || normalized.starts_with("//")
        || drive_prefix
        || normalized.split('/').any(|part| part == "..")
    {
        return Err(ExecutionError::UnsafeArchive(format!("chemin non sûr : {value}")));
    }
    Ok(())
}

async fn resolve_directory_conflict(
    base: PathBuf,
    strategy: fileflow_domain::ConflictStrategy,
) -> Result<PathBuf, ExecutionError> {
    if !base.exists() || matches!(strategy, fileflow_domain::ConflictStrategy::Skip | fileflow_domain::ConflictStrategy::Replace) {
        return Ok(base);
    }
    if strategy == fileflow_domain::ConflictStrategy::Ask {
        return Err(ExecutionError::Destination(format!("la destination existe déjà : {}", base.display())));
    }
    let parent = base.parent().unwrap_or_else(|| Path::new("."));
    let stem = base.file_name().and_then(|value| value.to_str()).unwrap_or("extraction");
    for index in 1..=10_000 {
        let candidate = parent.join(format!("{stem} ({index})"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(ExecutionError::Destination(base.display().to_string()))
}

fn safe_folder_name(value: &str) -> String {
    let value = value.trim();
    let cleaned = value
        .chars()
        .map(|character| match character { '/' | '\\' | ':' | '\0' => '-', other => other })
        .collect::<String>();
    if cleaned.is_empty() { "FileFlow".into() } else { cleaned }
}

async fn capture_process(
    engine: &Path,
    args: &[OsString],
    cancellation: &CancellationToken,
) -> Result<String, ExecutionError> {
    if cancellation.is_cancelled() {
        return Err(ExecutionError::Cancelled);
    }
    let mut command = Command::new(engine);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let program = engine.file_name().and_then(|value| value.to_str()).unwrap_or("engine").to_owned();
    let future = command.output();
    tokio::pin!(future);
    let output = tokio::select! {
        result = &mut future => result?,
        _ = cancellation.cancelled() => return Err(ExecutionError::Cancelled),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ExecutionError::ProcessFailed {
            program,
            message: tail_message(&stderr, output.status),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn run_process_with_env(
    engine: &Path,
    args: &[OsString],
    env: &[(&str, String)],
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    if cancellation.is_cancelled() { return Err(ExecutionError::Cancelled); }
    let mut command = Command::new(engine);
    command
        .args(args)
        .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let program = engine.file_name().and_then(|value| value.to_str()).unwrap_or("engine").to_owned();
    let future = command.output();
    tokio::pin!(future);
    let output = tokio::select! {
        result = &mut future => result?,
        _ = cancellation.cancelled() => return Err(ExecutionError::Cancelled),
    };
    if output.status.success() { return Ok(()); }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(ExecutionError::ProcessFailed { program, message: tail_message(&stderr, output.status) })
}

async fn run_process(engine: &Path, args: &[OsString], cancellation: &CancellationToken) -> Result<(), ExecutionError> {
    if cancellation.is_cancelled() { return Err(ExecutionError::Cancelled); }
    let mut command = Command::new(engine);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
    let program = engine.file_name().and_then(|value| value.to_str()).unwrap_or("engine").to_owned();
    let future = command.output();
    tokio::pin!(future);
    let output = tokio::select! {
        result = &mut future => result?,
        _ = cancellation.cancelled() => return Err(ExecutionError::Cancelled),
    };
    if output.status.success() { return Ok(()); }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(ExecutionError::ProcessFailed { program, message: tail_message(&stderr, output.status) })
}

fn tail_message(stderr: &str, status: std::process::ExitStatus) -> String {
    let message = stderr.chars().rev().take(4000).collect::<String>().chars().rev().collect::<String>();
    if message.trim().is_empty() { format!("exit status {status}") } else { message }
}

fn profile_for(engine: &str) -> ResourceProfile {
    match engine {
        "ffmpeg" => ResourceProfile::MEDIA,
        "vips" => ResourceProfile::IMAGE,
        "office" => ResourceProfile::OFFICE,
        "ocr" | "tesseract" => ResourceProfile::OCR,
        "archive" => ResourceProfile::ARCHIVE,
        "qpdf" | "ghostscript" | "poppler" => ResourceProfile::PDF,
        _ => ResourceProfile::LIGHT,
    }
}

fn is_audio_extension(extension: Option<&str>) -> bool {
    extension.is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "mp3"|"wav"|"aac"|"m4a"|"flac"|"ogg"|"opus"|"wma"|"aiff"|"aif"))
}

fn normalize_target_format(action_id: &str, format: Option<&str>) -> Result<Option<String>, ExecutionError> {
    let Some(format) = format else { return Ok(None); };
    let normalized = format.trim().trim_start_matches('.').to_ascii_lowercase();
    let allowed: &[&str] = match action_id {
        "image-convert" | "image-batch-convert" | "image-optimize" | "image-resize" =>
            &["jpg", "jpeg", "png", "webp", "avif", "heic", "heif", "tif", "tiff", "bmp", "gif"],
        "audio-convert" | "extract-audio" =>
            &["mp3", "m4a", "aac", "wav", "flac", "ogg", "opus"],
        "archive-create" => &["zip", "7z", "tar"],
        _ => &[],
    };
    if !allowed.is_empty() && !allowed.contains(&normalized.as_str()) {
        return Err(ExecutionError::InvalidTargetFormat {
            action: action_id.to_owned(),
            format: normalized,
        });
    }
    if allowed.is_empty() {
        return Ok(None);
    }
    Ok(Some(normalized))
}

fn normalize_quality(quality: Option<&str>) -> Option<String> {
    match quality.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("small") => Some("small".into()),
        Some("high") => Some("high".into()),
        Some("balanced") | Some("standard") => Some("balanced".into()),
        _ => None,
    }
}

pub const EXECUTABLE_ACTIONS: &[&str] = &[
    "images-to-pdf", "image-convert", "image-batch-convert", "image-optimize", "image-resize", "strip-metadata", "extract-metadata",
    "office-to-pdf", "pdf-merge", "pdf-split", "pdf-compress", "pdf-to-images", "pdf-extract-text", "pdf-ocr", "ocr-image",
    "archive-extract", "archive-create", "media-compatible", "media-compress", "audio-convert",
    "extract-audio", "video-to-gif",
];

pub fn executable_action_ids() -> Vec<&'static str> { EXECUTABLE_ACTIONS.to_vec() }

pub fn is_supported(action_id: &str) -> bool { EXECUTABLE_ACTIONS.contains(&action_id) }

fn is_collective(action_id: &str) -> bool { matches!(action_id, "images-to-pdf"|"pdf-merge"|"archive-create") }

async fn send(events: &mpsc::Sender<ExecutionEvent>, event: ExecutionEvent) -> Result<(), ExecutionError> {
    events.send(event).await.map_err(|_| ExecutionError::EventConsumerDisconnected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_archive_parent_traversal_and_absolute_paths() {
        assert!(validate_archive_path("docs/report.pdf").is_ok());
        assert!(validate_archive_path("../secret.txt").is_err());
        assert!(validate_archive_path("folder/../../secret.txt").is_err());
        assert!(validate_archive_path("/etc/passwd").is_err());
        assert!(validate_archive_path("C:\\Windows\\system.ini").is_err());
    }

    #[test]
    fn rejects_suspicious_archive_listing() {
        let listing = "Physical Size = 1000000\n----------\nPath = safe.txt\nSize = 120\nPath = ../escape.txt\nSize = 10\n";
        assert!(matches!(validate_archive_listing(listing), Err(ExecutionError::UnsafeArchive(_))));
    }

    #[test]
    fn normalizes_only_supported_target_formats() {
        assert_eq!(normalize_target_format("image-convert", Some(".WEBP")).unwrap().as_deref(), Some("webp"));
        assert!(normalize_target_format("image-convert", Some("../../pdf")).is_err());
        assert_eq!(normalize_target_format("pdf-compress", Some("png")).unwrap(), None);
    }

    #[test]
    fn executor_capability_list_matches_runtime_guard() {
        assert!(is_supported("pdf-merge"));
        assert!(is_supported("video-to-gif"));
        assert!(!is_supported("duplicate-scan"));
        assert!(executable_action_ids().iter().all(|action| is_supported(action)));
    }
    #[test]
    fn image_pdf_list_is_nul_separated() {
        let inputs = vec![
            ExecutionInput { path: PathBuf::from("a.jpg"), source_root: None },
            ExecutionInput { path: PathBuf::from("b image.png"), source_root: None },
        ];
        let bytes = nul_separated_paths(&inputs);
        assert!(bytes.ends_with(&[0]));
        assert_eq!(bytes.iter().filter(|byte| **byte == 0).count(), 2);
    }

}
