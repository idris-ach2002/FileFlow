//! Bounded job execution, process lifecycle and cancellation.
//!
//! External programs are always launched directly through `Command`; FileFlow
//! never interpolates paths into a shell command. Batch work is windowed and
//! every item must acquire a resource lease from the scheduler first.

use chrono::Utc;
use fileflow_domain::{FormatFamily, JobId, JobState, OutputPolicy, ResourceProfile};
use fileflow_formats::FormatRegistry;
use fileflow_output::{OutputPlan, OutputRequest, OutputResolver};
use fileflow_planner::{CapabilityCatalog, ConversionStep};
use fileflow_scheduler::ResourceScheduler;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::{io::AsyncReadExt, process::Command, sync::mpsc, task::JoinSet};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[cfg(target_os = "linux")]
fn configure_external_command(command: &mut Command) {
    // AppImage exports its own loader/library environment to the desktop
    // process. System-managed conversion engines must not inherit it: doing so
    // can mix Ubuntu libraries (for example libcurl) with libraries shipped by
    // the AppImage (for example libnghttp2), causing ABI "undefined symbol"
    // failures even though all host dependencies are installed correctly.
    const APPIMAGE_ENV_VARS: &[&str] = &[
        "APPDIR",
        "APPIMAGE",
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "PYTHONHOME",
        "PYTHONPATH",
        "GI_TYPELIB_PATH",
        "GIO_EXTRA_MODULES",
        "GSETTINGS_SCHEMA_DIR",
        "GTK_PATH",
        "QT_PLUGIN_PATH",
        "QML2_IMPORT_PATH",
        "GST_PLUGIN_PATH",
        "GST_PLUGIN_SYSTEM_PATH",
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        "MAGICK_HOME",
        "MAGICK_CONFIGURE_PATH",
        "MAGICK_CODER_MODULE_PATH",
        "TESSDATA_PREFIX",
        "VIPS_PLUGIN_PATH",
        "GS_LIB",
    ];

    for variable in APPIMAGE_ENV_VARS {
        command.env_remove(variable);
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_external_command(_command: &mut Command) {}

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
    #[serde(default)]
    pub parameters: HashMap<String, serde_json::Value>,
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
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "event",
    content = "data"
)]
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
    Phase {
        job_id: JobId,
        phase: String,
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
    #[error("invalid input: {0}")]
    InvalidInput(String),
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
        request.target_format =
            normalize_target_format(&request.action_id, request.target_format.as_deref())?;
        request.quality = normalize_quality(request.quality.as_deref());

        let started = Instant::now();
        let total = request.inputs.len();
        send(
            &events,
            ExecutionEvent::Started {
                job_id,
                action_id: request.action_id.clone(),
                total,
            },
        )
        .await?;

        let summary = if is_collective(&request.action_id) {
            self.execute_collective(job_id, request, engines, cancellation, events.clone())
                .await?
        } else {
            self.execute_batch(job_id, request, engines, cancellation, events.clone())
                .await?
        };

        let summary = ExecutionSummary {
            duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            finished_at: Utc::now(),
            ..summary
        };
        send(
            &events,
            ExecutionEvent::Finished {
                summary: summary.clone(),
            },
        )
        .await?;
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
        let window = self
            .scheduler
            .budget()
            .cpu_tokens
            .saturating_mul(2)
            .clamp(1, 16);
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
                let parameters = request.parameters.clone();
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
                        &parameters,
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
                Ok(Err(ItemExecutionError {
                    index,
                    input,
                    error,
                })) => {
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
            job_id,
            events.clone(),
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
                completed: if state == JobState::Cancelled {
                    0
                } else {
                    total
                },
                total,
            },
        )
        .await?;

        Ok(ExecutionSummary {
            job_id,
            action_id: request.action_id,
            state,
            total,
            succeeded: if state == JobState::Completed {
                total
            } else {
                0
            },
            skipped: 0,
            failed: if state == JobState::Failed { total } else { 0 },
            outputs,
            failures,
            duration_ms: 0,
            finished_at: Utc::now(),
        })
    }
}

// Internal summary factory; arguments directly mirror ExecutionSummary data.
#[allow(clippy::too_many_arguments)]
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

// Internal execution boundary; explicit parameters keep per-item job state visible.
#[allow(clippy::too_many_arguments)]
async fn execute_item(
    index: usize,
    input: ExecutionInput,
    action_id: &str,
    output_policy: &OutputPolicy,
    target_format: Option<&str>,
    quality: Option<&str>,
    parameters: &HashMap<String, serde_json::Value>,
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
        parameters,
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

// Internal engine dispatch boundary; parameters represent the complete execution context.
#[allow(clippy::too_many_arguments)]
async fn execute_item_inner(
    input: &Path,
    source_root: Option<&Path>,
    action_id: &str,
    output_policy: &OutputPolicy,
    target_format: Option<&str>,
    quality: Option<&str>,
    parameters: &HashMap<String, serde_json::Value>,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
    resolver: OutputResolver,
) -> Result<(Option<PathBuf>, bool), ExecutionError> {
    if cancellation.is_cancelled() {
        return Err(ExecutionError::Cancelled);
    }
    if action_id == "archive-extract" {
        return execute_archive_extract(
            input,
            source_root,
            output_policy,
            engines,
            scheduler,
            cancellation,
        )
        .await
        .map(|(path, skipped)| (Some(path), skipped));
    }
    if action_id == "zstd-decompress" {
        return execute_zstd_decompress(
            input,
            source_root,
            output_policy,
            engines,
            scheduler,
            cancellation,
            resolver,
        )
        .await
        .map(|(path, skipped)| (Some(path), skipped));
    }
    if action_id == "lz4-decompress" {
        return execute_lz4_decompress(
            input,
            source_root,
            output_policy,
            engines,
            scheduler,
            cancellation,
            resolver,
        )
        .await
        .map(|(path, skipped)| (Some(path), skipped));
    }
    if action_id == "pdf-split" {
        return execute_pdf_split(
            input,
            source_root,
            output_policy,
            engines,
            scheduler,
            cancellation,
        )
        .await
        .map(|(path, skipped)| (Some(path), skipped));
    }
    if action_id == "pdf-to-images" {
        return execute_pdf_to_images(
            input,
            source_root,
            output_policy,
            engines,
            scheduler,
            cancellation,
        )
        .await
        .map(|(path, skipped)| (Some(path), skipped));
    }
    if action_id == "ebook-convert" {
        validate_pandoc_ebook_input(input)?;
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
    if plan.skipped {
        return Ok((Some(plan.final_path), true));
    }
    resolver.prepare(&plan).await?;

    let execution = match action_id {
        "image-convert" | "image-batch-convert" => {
            run_vips_copy(
                engine,
                input,
                &plan.temporary_path,
                engine_threads,
                &cancellation,
            )
            .await
        }
        "image-optimize" | "image-resize" => {
            run_vips_thumbnail(
                engine,
                input,
                &plan.temporary_path,
                quality,
                engine_threads,
                &cancellation,
            )
            .await
        }
        action if is_imagemagick_action(action) => {
            run_imagemagick_action(
                engine,
                action,
                input,
                &plan.temporary_path,
                parameters,
                &cancellation,
            )
            .await
        }
        "strip-metadata" => {
            run_exiftool_strip(engine, input, &plan.temporary_path, &cancellation).await
        }
        "extract-metadata" => {
            run_exiftool_json(engine, input, &plan.temporary_path, &cancellation).await
        }
        "office-to-pdf" => run_office_convert(engine, input, &plan, "pdf", &cancellation).await,
        "office-convert" => {
            run_office_convert(
                engine,
                input,
                &plan,
                extension.unwrap_or("pdf"),
                &cancellation,
            )
            .await
        }
        action if is_qpdf_action(action) => {
            run_qpdf_action(
                engine,
                action,
                input,
                &plan.temporary_path,
                parameters,
                &cancellation,
            )
            .await
        }
        "pdf-protect" => {
            run_pdf_protect(
                engine,
                input,
                &plan.temporary_path,
                parameters,
                &cancellation,
            )
            .await
        }
        "text-to-pdf" => run_text_to_pdf(engines, input, &plan.temporary_path, &cancellation).await,
        "pdf-compress" => {
            run_pdf_compress(engine, input, &plan.temporary_path, quality, &cancellation).await
        }
        "pdf-extract-text" => {
            run_process(
                engine,
                &[
                    input.as_os_str().into(),
                    plan.temporary_path.as_os_str().into(),
                ],
                &cancellation,
            )
            .await
        }
        "pdf-ocr" => run_pdf_ocr(engine, input, &plan.temporary_path, &cancellation).await,
        "ocr-image" => run_tesseract(engine, input, &plan.temporary_path, &cancellation).await,
        "media-compatible" | "media-compress" | "video-convert" | "audio-convert"
        | "extract-audio" | "video-to-gif" | "video-rotate" | "video-resize" | "video-mute"
        | "video-thumbnail" | "media-trim" | "audio-normalize" | "audio-gain" | "audio-mono" => {
            run_ffmpeg(
                engine,
                action_id,
                input,
                &plan.temporary_path,
                quality,
                parameters,
                engine_threads,
                &cancellation,
            )
            .await
        }
        "zstd-compress" => {
            run_zstd_compress(
                engine,
                input,
                &plan.temporary_path,
                quality,
                engine_threads,
                &cancellation,
            )
            .await
        }
        "lz4-compress" => {
            run_lz4_compress(engine, input, &plan.temporary_path, quality, &cancellation).await
        }
        "text-convert" | "ebook-convert" => {
            run_pandoc(engine, input, &plan.temporary_path, &cancellation).await
        }
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
    job_id: JobId,
    events: mpsc::Sender<ExecutionEvent>,
) -> Result<Option<PathBuf>, ExecutionError> {
    match request.action_id.as_str() {
        "smart-to-pdf" | "collection-to-pdf" => {
            execute_smart_pdf(
                request,
                engines,
                scheduler,
                cancellation,
                resolver,
                job_id,
                events,
            )
            .await
        }
        "images-to-pdf" => {
            let engine = engines.get("img2pdf")?;
            let _lease = scheduler
                .acquire("img2pdf", ResourceProfile::PDF, &cancellation)
                .await?;
            let first = &request.inputs[0].path;
            let plan = resolver.plan(&OutputRequest {
                source: first.clone(),
                source_root: None,
                desired_extension: Some("pdf".into()),
                operation_suffix: Some("images".into()),
                policy: fileflow_domain::OutputPolicy {
                    naming: fileflow_domain::NamingStrategy::OperationSuffix,
                    ..request.output_policy.clone()
                },
            })?;
            if plan.skipped {
                return Ok(Some(plan.final_path));
            }
            resolver.prepare(&plan).await?;
            let list_path = plan.destination_directory.join(format!(
                ".fileflow-img2pdf-{}.list",
                Uuid::new_v4().simple()
            ));
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
            let _lease = scheduler
                .acquire("qpdf", ResourceProfile::PDF, &cancellation)
                .await?;
            let first = &request.inputs[0].path;
            let plan = resolver.plan(&OutputRequest {
                source: first.clone(),
                source_root: None,
                desired_extension: Some("pdf".into()),
                operation_suffix: Some("fusion".into()),
                policy: fileflow_domain::OutputPolicy {
                    naming: fileflow_domain::NamingStrategy::OperationSuffix,
                    ..request.output_policy.clone()
                },
            })?;
            if plan.skipped {
                return Ok(Some(plan.final_path));
            }
            resolver.prepare(&plan).await?;
            let mut args = vec![OsString::from("--empty"), OsString::from("--pages")];
            for input in &request.inputs {
                args.push(input.path.as_os_str().into());
            }
            args.extend([OsString::from("--"), plan.temporary_path.as_os_str().into()]);
            run_process(engine, &args, &cancellation).await?;
            resolver.finalize(&plan).await?;
            Ok(Some(plan.final_path))
        }
        "tar-zstd-create" => {
            execute_tar_compressed_archive(
                request,
                "zstd",
                "tar.zst",
                engines,
                scheduler,
                cancellation,
                resolver,
            )
            .await
        }
        "tar-lz4-create" => {
            execute_tar_compressed_archive(
                request,
                "lz4",
                "tar.lz4",
                engines,
                scheduler,
                cancellation,
                resolver,
            )
            .await
        }
        "archive-package" => {
            execute_archive_package(request, engines, scheduler, cancellation, resolver).await
        }
        "archive-create" => {
            let engine = engines.get("archive")?;
            let _lease = scheduler
                .acquire("archive", ResourceProfile::ARCHIVE, &cancellation)
                .await?;
            let first = &request.inputs[0].path;
            let target_format = request.target_format.as_deref().unwrap_or("zip");
            let plan = resolver.plan(&OutputRequest {
                source: first.clone(),
                source_root: None,
                desired_extension: Some(target_format.into()),
                operation_suffix: Some("archive".into()),
                policy: fileflow_domain::OutputPolicy {
                    naming: fileflow_domain::NamingStrategy::OperationSuffix,
                    ..request.output_policy.clone()
                },
            })?;
            if plan.skipped {
                return Ok(Some(plan.final_path));
            }
            resolver.prepare(&plan).await?;
            let mut args = vec![OsString::from("a"), plan.temporary_path.as_os_str().into()];
            for input in &request.inputs {
                args.push(input.path.as_os_str().into());
            }
            run_process(engine, &args, &cancellation).await?;
            resolver.finalize(&plan).await?;
            Ok(Some(plan.final_path))
        }
        _ => Err(ExecutionError::UnsupportedAction(request.action_id.clone())),
    }
}

#[derive(Debug)]
struct TemporaryJobWorkspace {
    root: PathBuf,
}

impl TemporaryJobWorkspace {
    async fn create() -> Result<Self, ExecutionError> {
        let root = std::env::temp_dir()
            .join("fileflow-jobs")
            .join(Uuid::new_v4().simple().to_string());
        tokio::fs::create_dir_all(&root).await?;
        Ok(Self { root })
    }

    fn path(&self, name: impl AsRef<Path>) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for TemporaryJobWorkspace {
    fn drop(&mut self) {
        // The result is promoted outside this directory before Drop. Everything
        // else is an implementation detail and must disappear after the job.
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn execute_smart_pdf(
    request: &ExecutionRequest,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
    resolver: OutputResolver,
    job_id: JobId,
    events: mpsc::Sender<ExecutionEvent>,
) -> Result<Option<PathBuf>, ExecutionError> {
    let first = &request.inputs[0].path;
    let suffix = if request.action_id == "collection-to-pdf" {
        "dossier"
    } else {
        "pdf"
    };
    let plan = resolver.plan(&OutputRequest {
        source: first.clone(),
        source_root: request.inputs[0].source_root.clone(),
        desired_extension: Some("pdf".into()),
        operation_suffix: Some(suffix.into()),
        policy: fileflow_domain::OutputPolicy {
            naming: fileflow_domain::NamingStrategy::OperationSuffix,
            preserve_tree: false,
            ..request.output_policy.clone()
        },
    })?;
    if plan.skipped {
        return Ok(Some(plan.final_path));
    }
    resolver.prepare(&plan).await?;
    send_phase(
        &events,
        job_id,
        "preparation",
        0,
        request.inputs.len().max(1),
    )
    .await?;

    let job = TemporaryJobWorkspace::create().await?;
    let expanded_root = job.path("expanded");
    tokio::fs::create_dir_all(&expanded_root).await?;
    let mut inputs = Vec::<PathBuf>::new();

    for (index, input) in request.inputs.iter().enumerate() {
        if cancellation.is_cancelled() {
            return Err(ExecutionError::Cancelled);
        }
        if input.path.is_dir() {
            collect_regular_files(&input.path, &mut inputs)?;
            continue;
        }
        let detected = detect_path(&input.path).await?;
        if detected.family == FormatFamily::Archive {
            if engines.get("archive").is_err() {
                inputs.push(input.path.clone());
                continue;
            }
            let archive_destination = expanded_root.join(format!("archive-{index}"));
            tokio::fs::create_dir_all(&archive_destination).await?;
            let temporary_policy = fileflow_domain::OutputPolicy {
                destination: fileflow_domain::DestinationPolicy::CustomFolder,
                custom_directory: Some(archive_destination),
                subfolder_name: "extracted".into(),
                preserve_tree: false,
                conflict: fileflow_domain::ConflictStrategy::Increment,
                naming: fileflow_domain::NamingStrategy::Original,
                overwrite_original: false,
            };
            let (folder, _) = execute_archive_extract(
                &input.path,
                None,
                &temporary_policy,
                engines,
                scheduler.clone(),
                cancellation.clone(),
            )
            .await?;
            collect_regular_files(&folder, &mut inputs)?;
        } else if input.path.is_file() {
            inputs.push(input.path.clone());
        }
    }

    deduplicate_paths(&mut inputs);
    sort_pdf_inputs(
        &mut inputs,
        parameter_optional_string(&request.parameters, "collectionOrder", 24).as_deref(),
    );
    if inputs.is_empty() {
        return Err(ExecutionError::InvalidInput(
            "Aucun fichier exploitable n’a été trouvé dans cette sélection.".into(),
        ));
    }
    if inputs.len() > 2_000 {
        return Err(ExecutionError::InvalidInput(
            "Ce dossier contient plus de 2 000 fichiers. Découpez-le en plusieurs lots.".into(),
        ));
    }

    send_phase(
        &events,
        job_id,
        "preparation",
        inputs.len(),
        inputs.len().max(1),
    )
    .await?;

    let available = engines.paths.keys().cloned().collect::<HashSet<_>>();
    let catalog = CapabilityCatalog::default();
    let parts_root = job.path("parts");
    tokio::fs::create_dir_all(&parts_root).await?;
    let pdf_parts = convert_components_parallel(
        inputs,
        ParallelConversionContext {
            parts_root: parts_root.clone(),
            catalog,
            available,
            engines: engines.clone(),
            scheduler: scheduler.clone(),
            cancellation: cancellation.clone(),
            job_id,
            events: events.clone(),
        },
    )
    .await?;

    send_phase(&events, job_id, "assemblage", 0, 1).await?;
    let mut current = job.path("assembled.pdf");
    if pdf_parts.len() == 1 {
        tokio::fs::copy(&pdf_parts[0], &current).await?;
    } else {
        merge_pdf_files(
            &pdf_parts,
            &current,
            engines,
            scheduler.clone(),
            &cancellation,
        )
        .await?;
    }

    send_phase(&events, job_id, "assemblage", 1, 1).await?;
    send_phase(&events, job_id, "finalisation", 0, 1).await?;

    if let Some(signature) = parameter_optional_string(&request.parameters, "signatureText", 180)
        && !signature.trim().is_empty()
    {
        let signature_pdf =
            make_signature_page(&job, &signature, engines, scheduler.clone(), &cancellation)
                .await?;
        let signed = job.path("signed.pdf");
        merge_pdf_files(
            &[current.clone(), signature_pdf],
            &signed,
            engines,
            scheduler.clone(),
            &cancellation,
        )
        .await?;
        current = signed;
    }

    if parameter_bool(&request.parameters, "improve", false)
        && let Ok(ocr) = engines.get("ocr")
    {
        let improved = job.path("improved.pdf");
        let _lease = scheduler
            .acquire("ocr", ResourceProfile::PDF, &cancellation)
            .await?;
        run_pdf_ocr(ocr, &current, &improved, &cancellation).await?;
        current = improved;
    }

    let compression = parameter_optional_string(&request.parameters, "finalCompression", 24)
        .unwrap_or_else(|| request.quality.clone().unwrap_or_else(|| "balanced".into()));
    let target_size_mb = parameter_number(&request.parameters, "targetSizeMb", 0.0, 0.0, 4096.0);
    let current_size = tokio::fs::metadata(&current)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let target_exceeded =
        target_size_mb > 0.0 && current_size > (target_size_mb * 1024.0 * 1024.0) as u64;
    if (compression != "keep" || target_exceeded)
        && let Ok(ghostscript) = engines.get("ghostscript")
    {
        let compressed = job.path("compressed.pdf");
        let quality = if target_exceeded {
            Some("small")
        } else {
            Some(compression.as_str())
        };
        let _lease = scheduler
            .acquire("ghostscript", ResourceProfile::PDF, &cancellation)
            .await?;
        run_pdf_compress(ghostscript, &current, &compressed, quality, &cancellation).await?;
        current = compressed;
    }

    if parameter_bool(&request.parameters, "stripMetadata", false)
        && let Ok(metadata) = engines.get("metadata")
    {
        let private = job.path("private.pdf");
        let _lease = scheduler
            .acquire("metadata", ResourceProfile::LIGHT, &cancellation)
            .await?;
        run_exiftool_strip(metadata, &current, &private, &cancellation).await?;
        current = private;
    }

    if parameter_optional_string(&request.parameters, "password", 256)
        .is_some_and(|password| !password.trim().is_empty())
    {
        let qpdf = engines.get("qpdf")?;
        let protected = job.path("protected.pdf");
        let _lease = scheduler
            .acquire("qpdf", ResourceProfile::PDF, &cancellation)
            .await?;
        run_pdf_protect(
            qpdf,
            &current,
            &protected,
            &request.parameters,
            &cancellation,
        )
        .await?;
        current = protected;
    }

    send_phase(&events, job_id, "validation", 0, 1).await?;
    validate_pdf_output(&current, engines, scheduler.clone(), &cancellation).await?;
    tokio::fs::copy(&current, &plan.temporary_path).await?;
    if let Err(error) =
        validate_pdf_output(&plan.temporary_path, engines, scheduler, &cancellation).await
    {
        resolver.cleanup(&plan).await;
        return Err(error);
    }
    resolver.finalize(&plan).await?;
    send_phase(&events, job_id, "finalisation", 1, 1).await?;
    Ok(Some(plan.final_path))
}

#[derive(Clone)]
struct ParallelConversionContext {
    parts_root: PathBuf,
    catalog: CapabilityCatalog,
    available: HashSet<String>,
    engines: EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
    job_id: JobId,
    events: mpsc::Sender<ExecutionEvent>,
}

async fn convert_components_parallel(
    inputs: Vec<PathBuf>,
    context: ParallelConversionContext,
) -> Result<Vec<PathBuf>, ExecutionError> {
    let total = inputs.len();
    let window = context.scheduler.budget().cpu_tokens.clamp(1, 8);
    let mut next = 0_usize;
    let mut join_set = JoinSet::new();
    let mut ordered = vec![None::<PathBuf>; total];
    let mut completed = 0_usize;
    send_phase(
        &context.events,
        context.job_id,
        "conversion",
        0,
        total.max(1),
    )
    .await?;

    while next < total || !join_set.is_empty() {
        while next < total && join_set.len() < window && !context.cancellation.is_cancelled() {
            let index = next;
            next += 1;
            let input = inputs[index].clone();
            let parts_root = context.parts_root.clone();
            let catalog = context.catalog.clone();
            let available = context.available.clone();
            let engines = context.engines.clone();
            let scheduler = context.scheduler.clone();
            let task_cancellation = context.cancellation.clone();
            join_set.spawn(async move {
                let output = convert_component_to_pdf(
                    &input,
                    index,
                    &parts_root,
                    &catalog,
                    &available,
                    &engines,
                    scheduler,
                    &task_cancellation,
                )
                .await?;
                Ok::<(usize, PathBuf), ExecutionError>((index, output))
            });
        }

        if context.cancellation.is_cancelled() {
            join_set.abort_all();
            while join_set.join_next().await.is_some() {}
            return Err(ExecutionError::Cancelled);
        }

        match join_set.join_next().await {
            Some(Ok(Ok((index, output)))) => {
                ordered[index] = Some(output);
                completed = completed.saturating_add(1);
                send_phase(
                    &context.events,
                    context.job_id,
                    "conversion",
                    completed,
                    total.max(1),
                )
                .await?;
            }
            Some(Ok(Err(error))) => {
                join_set.abort_all();
                while join_set.join_next().await.is_some() {}
                return Err(error);
            }
            Some(Err(error)) => {
                join_set.abort_all();
                while join_set.join_next().await.is_some() {}
                return Err(ExecutionError::Join(error.to_string()));
            }
            None => break,
        }
    }

    Ok(ordered.into_iter().flatten().collect())
}

fn is_collection_noise(path: &Path) -> bool {
    path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        value == "__MACOSX"
            || value == ".DS_Store"
            || value == "Thumbs.db"
            || value == "desktop.ini"
            || value.starts_with("._")
    })
}

async fn detect_path(path: &Path) -> Result<fileflow_domain::DetectedFormat, ExecutionError> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut sample = vec![0_u8; 64 * 1024];
    let read = file.read(&mut sample).await?;
    sample.truncate(read);
    Ok(FormatRegistry.detect(path, &sample))
}

fn deduplicate_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
}

fn sort_pdf_inputs(paths: &mut [PathBuf], order: Option<&str>) {
    match order.unwrap_or("name") {
        "selection" => {}
        "date" => paths.sort_by(|left, right| {
            let left_time = std::fs::metadata(left)
                .and_then(|meta| meta.modified())
                .ok();
            let right_time = std::fs::metadata(right)
                .and_then(|meta| meta.modified())
                .ok();
            left_time.cmp(&right_time).then_with(|| {
                left.to_string_lossy()
                    .to_ascii_lowercase()
                    .cmp(&right.to_string_lossy().to_ascii_lowercase())
            })
        }),
        _ => paths.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase()),
    }
}

fn collect_regular_files(root: &Path, output: &mut Vec<PathBuf>) -> Result<(), ExecutionError> {
    fn walk(path: &Path, output: &mut Vec<PathBuf>, depth: usize) -> std::io::Result<()> {
        if depth > 32 {
            return Ok(());
        }
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let child = entry.path();
            if file_type.is_symlink() || is_collection_noise(&child) {
                continue;
            }
            if file_type.is_dir() {
                walk(&child, output, depth + 1)?;
            } else if file_type.is_file() {
                output.push(child);
            }
        }
        Ok(())
    }
    walk(root, output, 0)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn convert_component_to_pdf(
    input: &Path,
    index: usize,
    parts_root: &Path,
    catalog: &CapabilityCatalog,
    available: &HashSet<String>,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: &CancellationToken,
) -> Result<PathBuf, ExecutionError> {
    let detected = detect_path(input).await?;
    if detected.family == FormatFamily::Pdf {
        let output = parts_root.join(format!("{index:05}.pdf"));
        tokio::fs::copy(input, &output).await?;
        return Ok(output);
    }

    if detected.family == FormatFamily::Image
        && matches!(detected.id.as_str(), "jpeg" | "png" | "tiff")
        && engines.get("img2pdf").is_ok()
    {
        return image_file_to_pdf(
            input,
            &parts_root.join(format!("{index:05}.pdf")),
            engines,
            scheduler,
            cancellation,
        )
        .await;
    }

    if detected.family == FormatFamily::Video {
        let ffmpeg = engines.get("ffmpeg")?;
        let thumbnail = parts_root.join(format!("{index:05}-preview.jpg"));
        let _lease = scheduler
            .acquire("ffmpeg", profile_for("ffmpeg"), cancellation)
            .await?;
        run_ffmpeg(
            ffmpeg,
            "video-thumbnail",
            input,
            &thumbnail,
            Some("balanced"),
            &HashMap::new(),
            scheduler.budget().cpu_tokens.max(1),
            cancellation,
        )
        .await?;
        return image_file_to_pdf(
            &thumbnail,
            &parts_root.join(format!("{index:05}.pdf")),
            engines,
            scheduler,
            cancellation,
        )
        .await;
    }

    if detected.family == FormatFamily::Audio {
        return Err(ExecutionError::InvalidInput(format!(
            "Le fichier audio {} ne possède pas de représentation PDF automatique fiable.",
            input.display()
        )));
    }

    let plan = catalog
        .conversion_plan_with_engines(&detected.id, "pdf", available)
        .or_else(|| {
            // Extension-specific text files are intentionally normalized to the
            // generic text node instead of pretending every structured syntax has
            // a native PDF renderer.
            (detected.family == FormatFamily::Text)
                .then(|| catalog.conversion_plan_with_engines("text", "pdf", available))
                .flatten()
        })
        .ok_or_else(|| {
            ExecutionError::InvalidInput(format!(
                "Aucun chemin de conversion fiable vers PDF pour {} ({})",
                input.display(),
                detected.id
            ))
        })?;

    let mut current = input.to_path_buf();
    for (step_index, step) in plan.steps.iter().enumerate() {
        let extension = safe_conversion_extension(&step.to).ok_or_else(|| {
            ExecutionError::InvalidInput(format!(
                "Format intermédiaire non autorisé dans le plan de conversion : {}",
                step.to
            ))
        })?;
        let output = parts_root.join(format!("{index:05}-step-{step_index:02}.{extension}"));
        execute_conversion_step(
            &current,
            &output,
            step,
            engines,
            scheduler.clone(),
            cancellation,
        )
        .await?;
        current = output;
    }

    if current.extension().and_then(|value| value.to_str()) != Some("pdf") {
        return Err(ExecutionError::InvalidInput(
            "Le plan de conversion n’a pas produit de PDF.".into(),
        ));
    }
    let final_part = parts_root.join(format!("{index:05}.pdf"));
    if current != final_part {
        tokio::fs::copy(&current, &final_part).await?;
    }
    Ok(final_part)
}

async fn execute_conversion_step(
    input: &Path,
    output: &Path,
    step: &ConversionStep,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let engine = engines.get(&step.engine_id)?;
    let profile = profile_for(&step.engine_id);
    let _lease = scheduler
        .acquire(&step.engine_id, profile, cancellation)
        .await?;
    match step.engine_id.as_str() {
        "vips" => {
            run_vips_copy(
                engine,
                input,
                output,
                scheduler.budget().cpu_tokens.max(1),
                cancellation,
            )
            .await
        }
        "imagemagick" => {
            run_process(
                engine,
                &[input.as_os_str().into(), output.as_os_str().into()],
                cancellation,
            )
            .await
        }
        "img2pdf" => {
            run_process(
                engine,
                &[
                    OsString::from("--rotation=ifvalid"),
                    input.as_os_str().into(),
                    OsString::from("-o"),
                    output.as_os_str().into(),
                ],
                cancellation,
            )
            .await
        }
        "office" => {
            if !matches!(
                step.from.as_str(),
                "doc"
                    | "docx"
                    | "odt"
                    | "rtf"
                    | "wpd"
                    | "xls"
                    | "xlsx"
                    | "ods"
                    | "csv"
                    | "tsv"
                    | "ppt"
                    | "pptx"
                    | "odp"
            ) {
                return Err(ExecutionError::InvalidInput(format!(
                    "LibreOffice n’est pas autorisé comme route intermédiaire depuis {}.",
                    step.from
                )));
            }
            let output_plan = transient_output_plan(output);
            run_office_convert(engine, input, &output_plan, &step.to, cancellation).await
        }
        "pandoc" => run_pandoc(engine, input, output, cancellation).await,
        other => Err(ExecutionError::InvalidInput(format!(
            "Le moteur {other} n’est pas encore autorisé comme intermédiaire PDF."
        ))),
    }
}

fn transient_output_plan(output: &Path) -> OutputPlan {
    OutputPlan {
        destination_directory: output
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
        final_path: output.to_path_buf(),
        temporary_path: output.to_path_buf(),
        replaces_existing: false,
        skipped: false,
    }
}

async fn image_file_to_pdf(
    input: &Path,
    output: &Path,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: &CancellationToken,
) -> Result<PathBuf, ExecutionError> {
    let img2pdf = engines.get("img2pdf")?;
    let _lease = scheduler
        .acquire("img2pdf", ResourceProfile::PDF, cancellation)
        .await?;
    run_process(
        img2pdf,
        &[
            OsString::from("--rotation=ifvalid"),
            input.as_os_str().into(),
            OsString::from("-o"),
            output.as_os_str().into(),
        ],
        cancellation,
    )
    .await?;
    Ok(output.to_path_buf())
}

async fn merge_pdf_files(
    inputs: &[PathBuf],
    output: &Path,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let qpdf = engines.get("qpdf")?;
    let _lease = scheduler
        .acquire("qpdf", ResourceProfile::PDF, cancellation)
        .await?;
    let mut args = vec![OsString::from("--empty"), OsString::from("--pages")];
    for input in inputs {
        args.push(input.as_os_str().into());
    }
    args.extend([OsString::from("--"), output.as_os_str().into()]);
    run_process(qpdf, &args, cancellation).await
}

async fn validate_pdf_output(
    path: &Path,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let metadata = tokio::fs::metadata(path).await?;
    if metadata.len() < 8 {
        return Err(ExecutionError::InvalidInput(
            "Le PDF généré est vide ou incomplet.".into(),
        ));
    }
    let mut file = tokio::fs::File::open(path).await?;
    let mut signature = [0_u8; 5];
    file.read_exact(&mut signature).await?;
    if &signature != b"%PDF-" {
        return Err(ExecutionError::InvalidInput(
            "Le résultat ne possède pas une signature PDF valide.".into(),
        ));
    }
    if let Ok(qpdf) = engines.get("qpdf") {
        let _lease = scheduler
            .acquire("qpdf", ResourceProfile::PDF, cancellation)
            .await?;
        run_process(
            qpdf,
            &[
                OsString::from("--warning-exit-0"),
                OsString::from("--check"),
                path.as_os_str().into(),
            ],
            cancellation,
        )
        .await?;
    }
    Ok(())
}

async fn make_signature_page(
    job: &TemporaryJobWorkspace,
    signature: &str,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: &CancellationToken,
) -> Result<PathBuf, ExecutionError> {
    let input = job.path("signature.html");
    let output = job.path("signature.pdf");
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><style>body{{font-family:Arial,sans-serif;margin:72px;color:#171a29}}.box{{margin-top:55vh;border-top:1px solid #d8dbea;padding-top:22px}}.sig{{font-family:cursive;font-size:38px;color:#25316d}}.hint{{font-size:13px;color:#73798f}}</style><div class=\"box\"><div class=\"hint\">Signature ajoutée par FileFlow</div><div class=\"sig\">{}</div></div>",
        escape_html(signature)
    );
    tokio::fs::write(&input, body).await?;
    let office = engines.get("office")?;
    let _lease = scheduler
        .acquire("office", ResourceProfile::OFFICE, cancellation)
        .await?;
    run_office_convert(
        office,
        &input,
        &transient_output_plan(&output),
        "pdf",
        cancellation,
    )
    .await?;
    Ok(output)
}

async fn run_text_to_pdf(
    engines: &EnginePaths,
    input: &Path,
    output: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let pandoc = engines.get("pandoc")?;
    let office = engines.get("office")?;
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let docx = parent.join(format!(".fileflow-text-{}.docx", Uuid::new_v4().simple()));
    let result = async {
        run_pandoc(pandoc, input, &docx, cancellation).await?;
        run_office_convert(
            office,
            &docx,
            &transient_output_plan(output),
            "pdf",
            cancellation,
        )
        .await
    }
    .await;
    let _ = tokio::fs::remove_file(&docx).await;
    result
}

async fn run_pdf_protect(
    engine: &Path,
    input: &Path,
    output: &Path,
    parameters: &HashMap<String, serde_json::Value>,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let password = parameter_optional_string(parameters, "password", 256)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ExecutionError::InvalidInput("Un mot de passe est nécessaire.".into()))?;
    run_process(
        engine,
        &[
            OsString::from("--encrypt"),
            OsString::from(&password),
            OsString::from(&password),
            OsString::from("256"),
            OsString::from("--"),
            input.as_os_str().into(),
            output.as_os_str().into(),
        ],
        cancellation,
    )
    .await
}

fn parameter_optional_string(
    parameters: &HashMap<String, serde_json::Value>,
    key: &str,
    max_len: usize,
) -> Option<String> {
    parameters
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(max_len).collect())
}

fn parameter_bool(
    parameters: &HashMap<String, serde_json::Value>,
    key: &str,
    fallback: bool,
) -> bool {
    parameters
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(fallback)
}

fn safe_conversion_extension(format: &str) -> Option<&'static str> {
    match format.to_ascii_lowercase().as_str() {
        "jpeg" | "jpg" => Some("jpg"),
        "png" => Some("png"),
        "webp" => Some("webp"),
        "avif" => Some("avif"),
        "tiff" | "tif" => Some("tiff"),
        "bmp" => Some("bmp"),
        "gif" => Some("gif"),
        "docx" => Some("docx"),
        "odt" => Some("odt"),
        "rtf" => Some("rtf"),
        "html" => Some("html"),
        "epub" => Some("epub"),
        "text" | "txt" => Some("txt"),
        "pdf" => Some("pdf"),
        _ => None,
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

async fn execute_archive_package(
    request: &ExecutionRequest,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
    resolver: OutputResolver,
) -> Result<Option<PathBuf>, ExecutionError> {
    let target = request.target_format.as_deref().unwrap_or("zip");
    if target == "tar.zst" {
        return execute_tar_compressed_archive(
            request,
            "zstd",
            "tar.zst",
            engines,
            scheduler,
            cancellation,
            resolver,
        )
        .await;
    }
    if target == "tar.lz4" {
        return execute_tar_compressed_archive(
            request,
            "lz4",
            "tar.lz4",
            engines,
            scheduler,
            cancellation,
            resolver,
        )
        .await;
    }
    let archive_engine = engines.get("archive")?;
    let _lease = scheduler
        .acquire("archive", ResourceProfile::ARCHIVE, &cancellation)
        .await?;
    let first = request
        .inputs
        .first()
        .ok_or_else(|| ExecutionError::InvalidInput("Aucun élément à compresser.".into()))?;
    let plan = resolver.plan(&OutputRequest {
        source: first.path.clone(),
        source_root: None,
        desired_extension: Some(target.into()),
        operation_suffix: Some("archive".into()),
        policy: fileflow_domain::OutputPolicy {
            naming: fileflow_domain::NamingStrategy::OperationSuffix,
            ..request.output_policy.clone()
        },
    })?;
    if plan.skipped {
        return Ok(Some(plan.final_path));
    }
    resolver.prepare(&plan).await?;

    let result = if matches!(target, "zip" | "7z" | "tar") {
        let archive_type = match target {
            "zip" => "-tzip",
            "7z" => "-t7z",
            _ => "-ttar",
        };
        let mut args = vec![
            OsString::from("a"),
            OsString::from(archive_type),
            plan.temporary_path.as_os_str().into(),
        ];
        for input in &request.inputs {
            args.push(input.path.as_os_str().into());
        }
        run_process(archive_engine, &args, &cancellation).await
    } else {
        let compression_type = match target {
            "tar.gz" => "-tgzip",
            "tar.xz" => "-txz",
            "tar.bz2" => "-tbzip2",
            _ => {
                return Err(ExecutionError::InvalidTargetFormat {
                    action: request.action_id.clone(),
                    format: target.into(),
                });
            }
        };
        let staging_tar = plan
            .destination_directory
            .join(format!(".fileflow-package-{}.tar", Uuid::new_v4().simple()));
        let mut tar_args = vec![
            OsString::from("a"),
            OsString::from("-ttar"),
            staging_tar.as_os_str().into(),
        ];
        for input in &request.inputs {
            tar_args.push(input.path.as_os_str().into());
        }
        run_process(archive_engine, &tar_args, &cancellation).await?;
        let compression = run_process(
            archive_engine,
            &[
                OsString::from("a"),
                OsString::from(compression_type),
                plan.temporary_path.as_os_str().into(),
                staging_tar.as_os_str().into(),
            ],
            &cancellation,
        )
        .await;
        let _ = tokio::fs::remove_file(&staging_tar).await;
        compression
    };
    if let Err(error) = result {
        resolver.cleanup(&plan).await;
        return Err(error);
    }
    resolver.finalize(&plan).await?;
    Ok(Some(plan.final_path))
}

#[allow(clippy::too_many_arguments)]
async fn execute_tar_compressed_archive(
    request: &ExecutionRequest,
    compressor_id: &str,
    output_extension: &str,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
    resolver: OutputResolver,
) -> Result<Option<PathBuf>, ExecutionError> {
    let archive_engine = engines.get("archive")?;
    let compressor = engines.get(compressor_id)?;
    let first = request
        .inputs
        .first()
        .ok_or_else(|| ExecutionError::InvalidInput("Aucun élément à compresser.".into()))?;
    let plan = resolver.plan(&OutputRequest {
        source: first.path.clone(),
        source_root: None,
        desired_extension: Some(output_extension.into()),
        operation_suffix: Some("archive".into()),
        policy: fileflow_domain::OutputPolicy {
            naming: fileflow_domain::NamingStrategy::OperationSuffix,
            ..request.output_policy.clone()
        },
    })?;
    if plan.skipped {
        return Ok(Some(plan.final_path));
    }
    resolver.prepare(&plan).await?;

    let staging_tar = plan
        .destination_directory
        .join(format!(".fileflow-stage-{}.tar", Uuid::new_v4().simple()));

    {
        let _archive_lease = scheduler
            .acquire("archive", ResourceProfile::ARCHIVE, &cancellation)
            .await?;
        let mut args = vec![
            OsString::from("a"),
            OsString::from("-ttar"),
            staging_tar.as_os_str().into(),
        ];
        for input in &request.inputs {
            args.push(input.path.as_os_str().into());
        }
        if let Err(error) = run_process(archive_engine, &args, &cancellation).await {
            let _ = tokio::fs::remove_file(&staging_tar).await;
            resolver.cleanup(&plan).await;
            return Err(error);
        }
    }

    let compression_lease = match scheduler
        .acquire(compressor_id, ResourceProfile::ARCHIVE, &cancellation)
        .await
    {
        Ok(lease) => lease,
        Err(error) => {
            let _ = tokio::fs::remove_file(&staging_tar).await;
            resolver.cleanup(&plan).await;
            return Err(error.into());
        }
    };
    let compression_result = {
        let _compression_lease = compression_lease;
        let threads = scheduler.budget().cpu_tokens.max(1);
        if compressor_id == "zstd" {
            run_zstd_compress(
                compressor,
                &staging_tar,
                &plan.temporary_path,
                request.quality.as_deref(),
                threads,
                &cancellation,
            )
            .await
        } else {
            run_lz4_compress(
                compressor,
                &staging_tar,
                &plan.temporary_path,
                request.quality.as_deref(),
                &cancellation,
            )
            .await
        }
    };

    let _ = tokio::fs::remove_file(&staging_tar).await;
    if let Err(error) = compression_result {
        resolver.cleanup(&plan).await;
        return Err(error);
    }
    resolver.finalize(&plan).await?;
    Ok(Some(plan.final_path))
}

fn item_output<'a>(
    action_id: &str,
    input: &'a Path,
    target_format: Option<&'a str>,
) -> Result<(&'static str, Option<&'a str>, Option<&'static str>), ExecutionError> {
    let source_extension = input.extension().and_then(|value| value.to_str());
    Ok(match action_id {
        "image-convert" | "image-batch-convert" => (
            "vips",
            Some(target_format.unwrap_or("jpg")),
            Some("converti"),
        ),
        "image-optimize" => (
            "vips",
            Some(target_format.or(source_extension).unwrap_or("jpg")),
            Some("optimise"),
        ),
        "image-resize" => (
            "vips",
            Some(target_format.or(source_extension).unwrap_or("jpg")),
            Some("redimensionne"),
        ),
        "image-rotate-left" => ("imagemagick", source_extension, Some("gauche")),
        "image-rotate-right" => ("imagemagick", source_extension, Some("droite")),
        "image-rotate-180" => ("imagemagick", source_extension, Some("rotation180")),
        "image-rotate" => ("imagemagick", source_extension, Some("rotation")),
        "image-flip-horizontal" => ("imagemagick", source_extension, Some("miroir-h")),
        "image-flip-vertical" => ("imagemagick", source_extension, Some("miroir-v")),
        "image-auto-orient" => ("imagemagick", source_extension, Some("oriente")),
        "image-grayscale" => ("imagemagick", source_extension, Some("gris")),
        "image-sepia" => ("imagemagick", source_extension, Some("sepia")),
        "image-auto-enhance" => ("imagemagick", source_extension, Some("ameliore")),
        "image-adjust" => ("imagemagick", source_extension, Some("corrige")),
        "image-sharpen" => ("imagemagick", source_extension, Some("net")),
        "image-blur" => ("imagemagick", source_extension, Some("flou")),
        "image-noise-reduce" => ("imagemagick", source_extension, Some("denoise")),
        "image-threshold" => ("imagemagick", source_extension, Some("seuil")),
        "image-posterize" => ("imagemagick", source_extension, Some("posterise")),
        "image-pixelate" => ("imagemagick", source_extension, Some("pixelise")),
        "image-flatten" => ("imagemagick", source_extension, Some("aplati")),
        "image-trim" => ("imagemagick", source_extension, Some("recadre")),
        "image-crop-center" => ("imagemagick", source_extension, Some("crop")),
        "image-resize-exact" => ("imagemagick", source_extension, Some("dimensions")),
        "image-crop-custom" => ("imagemagick", source_extension, Some("recadrage")),
        "image-canvas" => ("imagemagick", source_extension, Some("canevas")),
        "image-auto-gamma" => ("imagemagick", source_extension, Some("gamma")),
        "image-contrast-stretch" => ("imagemagick", source_extension, Some("contraste")),
        "image-colorspace-srgb" => ("imagemagick", source_extension, Some("srgb")),
        "image-set-dpi" => ("imagemagick", source_extension, Some("dpi")),
        "image-perspective" => ("imagemagick", source_extension, Some("perspective")),
        "image-border" => ("imagemagick", source_extension, Some("bordure")),
        "image-vignette" => ("imagemagick", source_extension, Some("vignette")),
        "image-watermark" => ("imagemagick", source_extension, Some("filigrane")),
        "strip-metadata" => ("metadata", source_extension, Some("prive")),
        "extract-metadata" => ("metadata", Some("json"), Some("metadonnees")),
        "office-to-pdf" => ("office", Some("pdf"), Some("pdf")),
        "office-convert" => (
            "office",
            Some(target_format.unwrap_or("pdf")),
            Some("converti"),
        ),
        "pdf-rotate-pages" => ("qpdf", Some("pdf"), Some("rotation")),
        "pdf-select-pages" => ("qpdf", Some("pdf"), Some("pages")),
        "pdf-linearize" => ("qpdf", Some("pdf"), Some("web")),
        "pdf-optimize-lossless" => ("qpdf", Some("pdf"), Some("optimise")),
        "pdf-repair" => ("qpdf", Some("pdf"), Some("repare")),
        "pdf-flatten-rotation" => ("qpdf", Some("pdf"), Some("rotation-aplatie")),
        "pdf-flatten-annotations" => ("qpdf", Some("pdf"), Some("annotations-aplaties")),
        "pdf-protect" => ("qpdf", Some("pdf"), Some("protege")),
        "text-to-pdf" => ("pandoc", Some("pdf"), Some("pdf")),
        "pdf-compress" => ("ghostscript", Some("pdf"), Some("leger")),
        "pdf-extract-text" => ("poppler", Some("txt"), Some("texte")),
        "pdf-ocr" => ("ocr", Some("pdf"), Some("ocr")),
        "ocr-image" => ("tesseract", Some("txt"), Some("texte")),
        "media-compatible" => (
            "ffmpeg",
            Some(if is_audio_extension(source_extension) {
                "m4a"
            } else {
                "mp4"
            }),
            Some("compatible"),
        ),
        "media-compress" => ("ffmpeg", source_extension.or(Some("mp4")), Some("leger")),
        "audio-convert" => (
            "ffmpeg",
            Some(target_format.unwrap_or("mp3")),
            Some("converti"),
        ),
        "extract-audio" => (
            "ffmpeg",
            Some(target_format.unwrap_or("m4a")),
            Some("audio"),
        ),
        "video-to-gif" => ("ffmpeg", Some("gif"), Some("animation")),
        "video-thumbnail" => ("ffmpeg", Some("jpg"), Some("miniature")),
        "video-rotate" => ("ffmpeg", source_extension.or(Some("mp4")), Some("rotation")),
        "video-resize" => (
            "ffmpeg",
            source_extension.or(Some("mp4")),
            Some("redimensionne"),
        ),
        "video-mute" => ("ffmpeg", source_extension.or(Some("mp4")), Some("sans-son")),
        "media-trim" => ("ffmpeg", source_extension.or(Some("mp4")), Some("extrait")),
        "audio-normalize" => (
            "ffmpeg",
            source_extension.or(Some("m4a")),
            Some("normalise"),
        ),
        "audio-gain" => ("ffmpeg", source_extension.or(Some("m4a")), Some("volume")),
        "audio-mono" => ("ffmpeg", source_extension.or(Some("m4a")), Some("mono")),
        "video-convert" => (
            "ffmpeg",
            Some(target_format.unwrap_or("mp4")),
            Some("converti"),
        ),
        "zstd-compress" => ("zstd", Some("zst"), Some("compresse")),
        "lz4-compress" => ("lz4", Some("lz4"), Some("compresse")),
        "text-convert" | "ebook-convert" => (
            "pandoc",
            Some(target_format.unwrap_or("html")),
            Some("converti"),
        ),
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
        &[
            OsString::from("copy"),
            input.as_os_str().into(),
            output.as_os_str().into(),
        ],
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
    let size = match quality {
        Some("small") => "1280",
        Some("high") => "2560",
        _ => "2048",
    };
    run_process_with_env(
        engine,
        &[
            OsString::from("thumbnail"),
            input.as_os_str().into(),
            output.as_os_str().into(),
            OsString::from(size),
            OsString::from("--size"),
            OsString::from("down"),
        ],
        &[("VIPS_CONCURRENCY", threads.to_string())],
        cancellation,
    )
    .await
}

fn is_imagemagick_action(action_id: &str) -> bool {
    matches!(
        action_id,
        "image-rotate-left"
            | "image-rotate-right"
            | "image-rotate-180"
            | "image-rotate"
            | "image-flip-horizontal"
            | "image-flip-vertical"
            | "image-auto-orient"
            | "image-grayscale"
            | "image-sepia"
            | "image-auto-enhance"
            | "image-adjust"
            | "image-sharpen"
            | "image-blur"
            | "image-noise-reduce"
            | "image-threshold"
            | "image-posterize"
            | "image-pixelate"
            | "image-flatten"
            | "image-trim"
            | "image-crop-center"
            | "image-resize-exact"
            | "image-crop-custom"
            | "image-canvas"
            | "image-auto-gamma"
            | "image-contrast-stretch"
            | "image-colorspace-srgb"
            | "image-set-dpi"
            | "image-perspective"
            | "image-border"
            | "image-vignette"
            | "image-watermark"
    )
}

fn parameter_number(
    parameters: &HashMap<String, serde_json::Value>,
    key: &str,
    default: f64,
    minimum: f64,
    maximum: f64,
) -> f64 {
    parameters
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(default)
        .clamp(minimum, maximum)
}

fn parameter_string(
    parameters: &HashMap<String, serde_json::Value>,
    key: &str,
    default: &str,
    max_chars: usize,
) -> String {
    parameters
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or(default)
        .chars()
        .take(max_chars)
        .collect()
}

async fn run_imagemagick_action(
    engine: &Path,
    action_id: &str,
    input: &Path,
    output: &Path,
    parameters: &HashMap<String, serde_json::Value>,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let mut args = vec![input.as_os_str().into()];
    match action_id {
        "image-rotate-left" => args.extend([OsString::from("-rotate"), OsString::from("-90")]),
        "image-rotate-right" => args.extend([OsString::from("-rotate"), OsString::from("90")]),
        "image-rotate-180" => args.extend([OsString::from("-rotate"), OsString::from("180")]),
        "image-rotate" => {
            let angle = parameter_number(parameters, "angle", 90.0, -360.0, 360.0);
            args.extend([
                OsString::from("-rotate"),
                OsString::from(format!("{angle:.2}")),
            ]);
        }
        "image-flip-horizontal" => args.push(OsString::from("-flop")),
        "image-flip-vertical" => args.push(OsString::from("-flip")),
        "image-auto-orient" => args.push(OsString::from("-auto-orient")),
        "image-grayscale" => args.extend([OsString::from("-colorspace"), OsString::from("Gray")]),
        "image-sepia" => {
            let strength = parameter_number(parameters, "strength", 80.0, 0.0, 100.0);
            args.extend([
                OsString::from("-sepia-tone"),
                OsString::from(format!("{strength:.0}%")),
            ]);
        }
        "image-auto-enhance" => args.extend([
            OsString::from("-auto-orient"),
            OsString::from("-auto-level"),
            OsString::from("-contrast-stretch"),
            OsString::from("0.5%x0.5%"),
        ]),
        "image-adjust" => {
            let brightness = parameter_number(parameters, "brightness", 0.0, -100.0, 100.0);
            let contrast = parameter_number(parameters, "contrast", 0.0, -100.0, 100.0);
            let saturation = parameter_number(parameters, "saturation", 100.0, 0.0, 200.0);
            let gamma = parameter_number(parameters, "gamma", 1.0, 0.1, 5.0);
            args.extend([
                OsString::from("-brightness-contrast"),
                OsString::from(format!("{brightness:.0}x{contrast:.0}")),
                OsString::from("-modulate"),
                OsString::from(format!("100,{saturation:.0},100")),
                OsString::from("-gamma"),
                OsString::from(format!("{gamma:.2}")),
            ]);
        }
        "image-sharpen" => {
            let amount = parameter_number(parameters, "amount", 1.0, 0.1, 5.0);
            args.extend([
                OsString::from("-sharpen"),
                OsString::from(format!("0x{amount:.2}")),
            ]);
        }
        "image-blur" => {
            let radius = parameter_number(parameters, "radius", 2.0, 0.1, 20.0);
            args.extend([
                OsString::from("-blur"),
                OsString::from(format!("0x{radius:.2}")),
            ]);
        }
        "image-noise-reduce" => args.push(OsString::from("-despeckle")),
        "image-threshold" => {
            let threshold = parameter_number(parameters, "threshold", 50.0, 0.0, 100.0);
            args.extend([
                OsString::from("-threshold"),
                OsString::from(format!("{threshold:.0}%")),
            ]);
        }
        "image-posterize" => {
            let levels = parameter_number(parameters, "levels", 6.0, 2.0, 32.0).round() as u32;
            args.extend([
                OsString::from("-posterize"),
                OsString::from(levels.to_string()),
            ]);
        }
        "image-pixelate" => {
            let percent = parameter_number(parameters, "pixelPercent", 8.0, 1.0, 50.0);
            let restore = (10000.0 / percent).round().clamp(200.0, 10000.0);
            args.extend([
                OsString::from("-scale"),
                OsString::from(format!("{percent:.0}%")),
                OsString::from("-scale"),
                OsString::from(format!("{restore:.0}%")),
            ]);
        }
        "image-flatten" => args.extend([
            OsString::from("-background"),
            OsString::from(parameter_string(parameters, "background", "white", 32)),
            OsString::from("-alpha"),
            OsString::from("remove"),
            OsString::from("-alpha"),
            OsString::from("off"),
        ]),
        "image-trim" => args.extend([OsString::from("-trim"), OsString::from("+repage")]),
        "image-crop-center" => {
            let width = parameter_number(parameters, "width", 1200.0, 1.0, 20000.0).round() as u32;
            let height =
                parameter_number(parameters, "height", 1200.0, 1.0, 20000.0).round() as u32;
            args.extend([
                OsString::from("-gravity"),
                OsString::from("center"),
                OsString::from("-crop"),
                OsString::from(format!("{width}x{height}+0+0")),
                OsString::from("+repage"),
            ]);
        }
        "image-resize-exact" => {
            let width = parameter_number(parameters, "width", 1920.0, 1.0, 20000.0).round() as u32;
            let height =
                parameter_number(parameters, "height", 1080.0, 1.0, 20000.0).round() as u32;
            let mode = parameter_string(parameters, "fit", "contain", 12);
            let geometry = match mode.as_str() {
                "stretch" => format!("{width}x{height}!"),
                "fill" => format!("{width}x{height}^"),
                _ => format!("{width}x{height}>"),
            };
            args.extend([OsString::from("-resize"), OsString::from(geometry)]);
            if mode == "fill" {
                args.extend([
                    OsString::from("-gravity"),
                    OsString::from("center"),
                    OsString::from("-extent"),
                    OsString::from(format!("{width}x{height}")),
                ]);
            }
        }
        "image-crop-custom" => {
            let width = parameter_number(parameters, "width", 1200.0, 1.0, 20000.0).round() as u32;
            let height =
                parameter_number(parameters, "height", 1200.0, 1.0, 20000.0).round() as u32;
            let x = parameter_number(parameters, "x", 0.0, 0.0, 20000.0).round() as u32;
            let y = parameter_number(parameters, "y", 0.0, 0.0, 20000.0).round() as u32;
            args.extend([
                OsString::from("-crop"),
                OsString::from(format!("{width}x{height}+{x}+{y}")),
                OsString::from("+repage"),
            ]);
        }
        "image-canvas" => {
            let width = parameter_number(parameters, "width", 1920.0, 1.0, 20000.0).round() as u32;
            let height =
                parameter_number(parameters, "height", 1080.0, 1.0, 20000.0).round() as u32;
            let color = parameter_string(parameters, "background", "white", 32);
            args.extend([
                OsString::from("-background"),
                OsString::from(color),
                OsString::from("-gravity"),
                OsString::from("center"),
                OsString::from("-extent"),
                OsString::from(format!("{width}x{height}")),
            ]);
        }
        "image-auto-gamma" => args.push(OsString::from("-auto-gamma")),
        "image-contrast-stretch" => {
            let black = parameter_number(parameters, "blackPoint", 0.5, 0.0, 20.0);
            let white = parameter_number(parameters, "whitePoint", 0.5, 0.0, 20.0);
            args.extend([
                OsString::from("-contrast-stretch"),
                OsString::from(format!("{black:.2}%x{white:.2}%")),
            ]);
        }
        "image-colorspace-srgb" => {
            args.extend([OsString::from("-colorspace"), OsString::from("sRGB")])
        }
        "image-set-dpi" => {
            let dpi = parameter_number(parameters, "dpi", 300.0, 36.0, 2400.0).round() as u32;
            args.extend([
                OsString::from("-units"),
                OsString::from("PixelsPerInch"),
                OsString::from("-density"),
                OsString::from(dpi.to_string()),
            ]);
        }
        "image-perspective" => {
            let x0 = parameter_number(parameters, "x0", 0.0, 0.0, 20000.0);
            let y0 = parameter_number(parameters, "y0", 0.0, 0.0, 20000.0);
            let x1 = parameter_number(parameters, "x1", 1200.0, 0.0, 20000.0);
            let y1 = parameter_number(parameters, "y1", 0.0, 0.0, 20000.0);
            let x2 = parameter_number(parameters, "x2", 1200.0, 0.0, 20000.0);
            let y2 = parameter_number(parameters, "y2", 1200.0, 0.0, 20000.0);
            let x3 = parameter_number(parameters, "x3", 0.0, 0.0, 20000.0);
            let y3 = parameter_number(parameters, "y3", 1200.0, 0.0, 20000.0);
            let width = parameter_number(parameters, "width", 1200.0, 1.0, 20000.0);
            let height = parameter_number(parameters, "height", 1200.0, 1.0, 20000.0);
            let mapping = format!(
                "{x0},{y0} 0,0 {x1},{y1} {width},0 {x2},{y2} {width},{height} {x3},{y3} 0,{height}"
            );
            args.extend([
                OsString::from("-virtual-pixel"),
                OsString::from("background"),
                OsString::from("-distort"),
                OsString::from("Perspective"),
                OsString::from(mapping),
                OsString::from("+repage"),
            ]);
        }
        "image-border" => {
            let pixels = parameter_number(parameters, "pixels", 16.0, 1.0, 500.0).round() as u32;
            let color = parameter_string(parameters, "color", "white", 32);
            args.extend([
                OsString::from("-bordercolor"),
                OsString::from(color),
                OsString::from("-border"),
                OsString::from(format!("{pixels}x{pixels}")),
            ]);
        }
        "image-vignette" => {
            let radius = parameter_number(parameters, "radius", 12.0, 0.0, 100.0);
            args.extend([
                OsString::from("-vignette"),
                OsString::from(format!("0x{radius:.0}")),
            ]);
        }
        "image-watermark" => {
            let text = parameter_string(parameters, "text", "FileFlow", 120);
            let size = parameter_number(parameters, "fontSize", 28.0, 8.0, 200.0).round() as u32;
            args.extend([
                OsString::from("-gravity"),
                OsString::from("southeast"),
                OsString::from("-pointsize"),
                OsString::from(size.to_string()),
                OsString::from("-fill"),
                OsString::from("rgba(255,255,255,0.72)"),
                OsString::from("-stroke"),
                OsString::from("rgba(0,0,0,0.35)"),
                OsString::from("-strokewidth"),
                OsString::from("1"),
                OsString::from("-annotate"),
                OsString::from("+24+24"),
                OsString::from(text),
            ]);
        }
        _ => return Err(ExecutionError::UnsupportedAction(action_id.into())),
    }
    args.push(output.as_os_str().into());
    run_process(engine, &args, cancellation).await
}

async fn run_exiftool_strip(
    engine: &Path,
    input: &Path,
    output: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    run_process(
        engine,
        &[
            OsString::from("-all="),
            OsString::from("-o"),
            output.as_os_str().into(),
            input.as_os_str().into(),
        ],
        cancellation,
    )
    .await
}

async fn run_exiftool_json(
    engine: &Path,
    input: &Path,
    output: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let json = capture_process(
        engine,
        &[
            OsString::from("-j"),
            OsString::from("-G"),
            OsString::from("-n"),
            input.as_os_str().into(),
        ],
        cancellation,
    )
    .await?;
    tokio::fs::write(output, json).await?;
    Ok(())
}

async fn run_office_convert(
    engine: &Path,
    input: &Path,
    plan: &OutputPlan,
    target_format: &str,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let staging = plan
        .destination_directory
        .join(format!(".fileflow-office-{}", Uuid::new_v4().simple()));
    tokio::fs::create_dir_all(&staging).await?;
    let profile = staging.join("profile");
    tokio::fs::create_dir_all(&profile).await?;
    let profile_arg = format!("-env:UserInstallation={}", file_url(&profile));
    let result = run_process(
        engine,
        &[
            OsString::from(profile_arg),
            OsString::from("--headless"),
            OsString::from("--invisible"),
            OsString::from("--nologo"),
            OsString::from("--nodefault"),
            OsString::from("--nofirststartwizard"),
            OsString::from("--nolockcheck"),
            OsString::from("--convert-to"),
            OsString::from(target_format),
            OsString::from("--outdir"),
            staging.as_os_str().into(),
            input.as_os_str().into(),
        ],
        cancellation,
    )
    .await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(error);
    }
    let generated = staging.join(format!(
        "{}.{}",
        input
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("document"),
        target_format.trim_start_matches('.')
    ));
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

fn is_qpdf_action(action_id: &str) -> bool {
    matches!(
        action_id,
        "pdf-rotate-pages"
            | "pdf-select-pages"
            | "pdf-linearize"
            | "pdf-optimize-lossless"
            | "pdf-repair"
            | "pdf-flatten-rotation"
            | "pdf-flatten-annotations"
    )
}

fn sanitize_pdf_pages(value: &str) -> String {
    let filtered: String = value
        .chars()
        .filter(|character| {
            character.is_ascii_digit() || matches!(character, ',' | '-' | 'z' | 'Z')
        })
        .take(128)
        .collect();
    if filtered.is_empty() {
        "1-z".into()
    } else {
        filtered.to_ascii_lowercase()
    }
}

async fn run_qpdf_action(
    engine: &Path,
    action_id: &str,
    input: &Path,
    output: &Path,
    parameters: &HashMap<String, serde_json::Value>,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let mut args = vec![input.as_os_str().into(), output.as_os_str().into()];
    match action_id {
        "pdf-rotate-pages" => {
            let angle = parameter_number(parameters, "angle", 90.0, -270.0, 270.0).round() as i32;
            let angle = match angle {
                -270 | -180 | -90 | 90 | 180 | 270 => angle,
                _ => 90,
            };
            let pages = sanitize_pdf_pages(&parameter_string(parameters, "pages", "1-z", 128));
            args.push(OsString::from(format!("--rotate={angle}:{pages}")));
        }
        "pdf-select-pages" => {
            let pages = sanitize_pdf_pages(&parameter_string(parameters, "pages", "1-z", 128));
            args.extend([
                OsString::from("--pages"),
                input.as_os_str().into(),
                OsString::from(pages),
                OsString::from("--"),
            ]);
        }
        "pdf-linearize" => args.push(OsString::from("--linearize")),
        "pdf-optimize-lossless" => args.extend([
            OsString::from("--object-streams=generate"),
            OsString::from("--compress-streams=y"),
            OsString::from("--recompress-flate"),
        ]),
        "pdf-repair" => args.push(OsString::from("--warning-exit-0")),
        "pdf-flatten-rotation" => args.push(OsString::from("--flatten-rotation")),
        "pdf-flatten-annotations" => args.push(OsString::from("--flatten-annotations=all")),
        _ => return Err(ExecutionError::UnsupportedAction(action_id.into())),
    }
    run_process(engine, &args, cancellation).await
}

async fn run_pdf_compress(
    engine: &Path,
    input: &Path,
    output: &Path,
    quality: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let profile = match quality {
        Some("small") => "/screen",
        Some("high") => "/prepress",
        _ => "/ebook",
    };
    run_process(
        engine,
        &[
            OsString::from("-sDEVICE=pdfwrite"),
            OsString::from("-dCompatibilityLevel=1.4"),
            OsString::from(format!("-dPDFSETTINGS={profile}")),
            OsString::from("-dNOPAUSE"),
            OsString::from("-dQUIET"),
            OsString::from("-dBATCH"),
            OsString::from(format!("-sOutputFile={}", output.to_string_lossy())),
            input.as_os_str().into(),
        ],
        cancellation,
    )
    .await
}

async fn run_pdf_ocr(
    engine: &Path,
    input: &Path,
    output: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    run_process(
        engine,
        &[
            OsString::from("--skip-text"),
            OsString::from("--deskew"),
            OsString::from("--optimize"),
            OsString::from("1"),
            input.as_os_str().into(),
            output.as_os_str().into(),
        ],
        cancellation,
    )
    .await
}

async fn run_tesseract(
    engine: &Path,
    input: &Path,
    output: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let base = output.with_extension("");
    run_process(
        engine,
        &[
            input.as_os_str().into(),
            base.as_os_str().into(),
            OsString::from("-l"),
            OsString::from("fra+eng"),
            OsString::from("txt"),
        ],
        cancellation,
    )
    .await?;
    let generated = base.with_extension("txt");
    if generated != output {
        tokio::fs::rename(generated, output).await?;
    }
    Ok(())
}

// Internal run_ffmpeg boundary: explicit parameters keep execution context visible and avoid opaque mutable state.
#[allow(clippy::too_many_arguments)]
async fn run_ffmpeg(
    engine: &Path,
    action_id: &str,
    input: &Path,
    output: &Path,
    quality: Option<&str>,
    parameters: &HashMap<String, serde_json::Value>,
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
                    OsString::from("-c:v"),
                    OsString::from("libx264"),
                    OsString::from("-preset"),
                    OsString::from("medium"),
                    OsString::from("-crf"),
                    OsString::from("23"),
                    OsString::from("-c:a"),
                    OsString::from("aac"),
                    OsString::from("-b:a"),
                    OsString::from("160k"),
                    OsString::from("-movflags"),
                    OsString::from("+faststart"),
                ]);
            }
        }
        "video-convert" => {
            let extension = output
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("mp4")
                .to_ascii_lowercase();
            let crf = if quality == Some("small") {
                "30"
            } else if quality == Some("high") {
                "20"
            } else {
                "24"
            };
            if extension == "webm" {
                args.extend([
                    OsString::from("-c:v"),
                    OsString::from("libvpx-vp9"),
                    OsString::from("-crf"),
                    OsString::from(crf),
                    OsString::from("-b:v"),
                    OsString::from("0"),
                    OsString::from("-c:a"),
                    OsString::from("libopus"),
                    OsString::from("-b:a"),
                    OsString::from("128k"),
                ]);
            } else {
                args.extend([
                    OsString::from("-c:v"),
                    OsString::from("libx264"),
                    OsString::from("-preset"),
                    OsString::from("medium"),
                    OsString::from("-crf"),
                    OsString::from(crf),
                    OsString::from("-c:a"),
                    OsString::from("aac"),
                    OsString::from("-b:a"),
                    OsString::from("160k"),
                ]);
                if matches!(extension.as_str(), "mp4" | "mov") {
                    args.extend([OsString::from("-movflags"), OsString::from("+faststart")]);
                }
            }
        }
        "media-compress" if source_audio => {
            args.extend([OsString::from("-vn"), OsString::from("-b:a")]);
            args.push(OsString::from(if quality == Some("small") {
                "96k"
            } else if quality == Some("high") {
                "192k"
            } else {
                "128k"
            }));
        }
        "media-compress" => {
            let crf = if quality == Some("small") {
                "30"
            } else if quality == Some("high") {
                "20"
            } else {
                "26"
            };
            args.extend([
                OsString::from("-c:v"),
                OsString::from("libx264"),
                OsString::from("-preset"),
                OsString::from("medium"),
                OsString::from("-crf"),
                OsString::from(crf),
                OsString::from("-c:a"),
                OsString::from("aac"),
                OsString::from("-b:a"),
                OsString::from("128k"),
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
        "video-rotate" => {
            let direction = parameter_string(parameters, "direction", "right", 12);
            let filter = match direction.as_str() {
                "left" => "transpose=2",
                "180" => "hflip,vflip",
                _ => "transpose=1",
            };
            args.extend([
                OsString::from("-vf"),
                OsString::from(filter),
                OsString::from("-c:a"),
                OsString::from("copy"),
            ]);
        }
        "video-resize" => {
            let width = parameter_number(parameters, "width", 1920.0, 16.0, 7680.0).round() as u32;
            let height =
                parameter_number(parameters, "height", 1080.0, 16.0, 4320.0).round() as u32;
            args.extend([
                OsString::from("-vf"),
                OsString::from(format!("scale=w={width}:h={height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2")),
                OsString::from("-c:v"), OsString::from("libx264"), OsString::from("-crf"), OsString::from("23"),
                OsString::from("-c:a"), OsString::from("aac"),
            ]);
        }
        "video-mute" => {
            args.push(OsString::from("-an"));
            args.extend([OsString::from("-c:v"), OsString::from("copy")]);
        }
        "video-thumbnail" => {
            let second = parameter_number(parameters, "second", 1.0, 0.0, 86400.0);
            args.extend([
                OsString::from("-ss"),
                OsString::from(format!("{second:.3}")),
                OsString::from("-frames:v"),
                OsString::from("1"),
                OsString::from("-q:v"),
                OsString::from("2"),
            ]);
        }
        "media-trim" => {
            let start = parameter_number(parameters, "start", 0.0, 0.0, 86400.0);
            let duration = parameter_number(parameters, "duration", 30.0, 0.1, 86400.0);
            args.extend([
                OsString::from("-ss"),
                OsString::from(format!("{start:.3}")),
                OsString::from("-t"),
                OsString::from(format!("{duration:.3}")),
            ]);
        }
        "audio-normalize" => {
            args.extend([
                OsString::from("-vn"),
                OsString::from("-af"),
                OsString::from("loudnorm=I=-16:LRA=11:TP=-1.5"),
            ]);
            push_audio_codec(&mut args, output);
        }
        "audio-gain" => {
            let gain = parameter_number(parameters, "gainDb", 0.0, -30.0, 30.0);
            args.extend([
                OsString::from("-vn"),
                OsString::from("-af"),
                OsString::from(format!("volume={gain:.1}dB")),
            ]);
            push_audio_codec(&mut args, output);
        }
        "audio-mono" => {
            args.extend([
                OsString::from("-vn"),
                OsString::from("-ac"),
                OsString::from("1"),
            ]);
            push_audio_codec(&mut args, output);
        }
        _ => {}
    }
    args.push(output.as_os_str().into());
    run_process(engine, &args, cancellation).await
}

fn push_audio_codec(args: &mut Vec<OsString>, output: &Path) {
    match output
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp3") => args.extend([
            OsString::from("-c:a"),
            OsString::from("libmp3lame"),
            OsString::from("-q:a"),
            OsString::from("2"),
        ]),
        Some("opus") => args.extend([
            OsString::from("-c:a"),
            OsString::from("libopus"),
            OsString::from("-b:a"),
            OsString::from("128k"),
        ]),
        Some("ogg") => args.extend([
            OsString::from("-c:a"),
            OsString::from("libvorbis"),
            OsString::from("-q:a"),
            OsString::from("5"),
        ]),
        Some("flac") => args.extend([OsString::from("-c:a"), OsString::from("flac")]),
        Some("wav") => args.extend([OsString::from("-c:a"), OsString::from("pcm_s16le")]),
        _ => args.extend([
            OsString::from("-c:a"),
            OsString::from("aac"),
            OsString::from("-b:a"),
            OsString::from("192k"),
        ]),
    }
}

async fn run_zstd_compress(
    engine: &Path,
    input: &Path,
    output: &Path,
    quality: Option<&str>,
    thread_count: usize,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    if !input.is_file() {
        return Err(ExecutionError::InvalidInput(
            "Zstandard compresse un fichier à la fois. Pour un dossier, créez d’abord une archive."
                .into(),
        ));
    }
    let level = match quality {
        Some("small") => "-15",
        Some("high") => "-1",
        _ => "-3",
    };
    run_process(
        engine,
        &[
            OsString::from("-q"),
            OsString::from("-f"),
            OsString::from(format!("-T{}", thread_count.max(1))),
            OsString::from(level),
            input.as_os_str().into(),
            OsString::from("-o"),
            output.as_os_str().into(),
        ],
        cancellation,
    )
    .await
}

fn validate_pandoc_ebook_input(input: &Path) -> Result<(), ExecutionError> {
    let extension = input
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "epub" | "fb2") {
        return Ok(());
    }

    Err(ExecutionError::InvalidInput(format!(
        "Le format .{} est reconnu comme livre numérique, mais la conversion directe est actuellement limitée aux fichiers EPUB et FB2.",
        if extension.is_empty() {
            "?"
        } else {
            &extension
        }
    )))
}

async fn run_pandoc(
    engine: &Path,
    input: &Path,
    output: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    run_process(
        engine,
        &[
            input.as_os_str().into(),
            OsString::from("--sandbox"),
            OsString::from("--standalone"),
            OsString::from("--output"),
            output.as_os_str().into(),
        ],
        cancellation,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_zstd_decompress(
    input: &Path,
    source_root: Option<&Path>,
    policy: &OutputPolicy,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
    resolver: OutputResolver,
) -> Result<(PathBuf, bool), ExecutionError> {
    let extension = input
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "zst" | "zstd" | "tzst") {
        return Err(ExecutionError::InvalidInput(
            "Cette action attend un fichier .zst, .zstd ou .tzst.".into(),
        ));
    }
    let engine = engines.get("zstd")?;
    let profile = ResourceProfile::ARCHIVE;
    let _lease = scheduler.acquire("zstd", profile, &cancellation).await?;
    let preferred_name = zstd_decompressed_name(input);
    let request = OutputRequest {
        source: input.to_path_buf(),
        source_root: source_root.map(Path::to_path_buf),
        desired_extension: None,
        operation_suffix: Some("decompresse".into()),
        policy: policy.clone(),
    };
    let plan = resolver.plan_named(&request, &preferred_name)?;
    if plan.skipped {
        return Ok((plan.final_path, true));
    }
    resolver.prepare(&plan).await?;
    let result = run_process(
        engine,
        &[
            OsString::from("-q"),
            OsString::from("-f"),
            OsString::from("-d"),
            input.as_os_str().into(),
            OsString::from("-o"),
            plan.temporary_path.as_os_str().into(),
        ],
        &cancellation,
    )
    .await;
    if let Err(error) = result {
        resolver.cleanup(&plan).await;
        return Err(error);
    }
    resolver.finalize(&plan).await?;
    Ok((plan.final_path, false))
}

fn zstd_decompressed_name(input: &Path) -> String {
    let extension = input
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("decompresse");
    if extension == "tzst" {
        format!("{stem}.tar")
    } else {
        stem.to_owned()
    }
}

async fn run_lz4_compress(
    engine: &Path,
    input: &Path,
    output: &Path,
    quality: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    if !input.is_file() {
        return Err(ExecutionError::InvalidInput(
            "LZ4 compresse un fichier à la fois. Pour un dossier, créez d’abord une archive."
                .into(),
        ));
    }
    let level = match quality {
        Some("small") => "-9",
        _ => "-1",
    };
    run_process(
        engine,
        &[
            OsString::from("-q"),
            OsString::from("-f"),
            OsString::from("-z"),
            OsString::from(level),
            input.as_os_str().into(),
            output.as_os_str().into(),
        ],
        cancellation,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_lz4_decompress(
    input: &Path,
    source_root: Option<&Path>,
    policy: &OutputPolicy,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
    resolver: OutputResolver,
) -> Result<(PathBuf, bool), ExecutionError> {
    let extension = input
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "lz4" {
        return Err(ExecutionError::InvalidInput(
            "Cette action attend un fichier .lz4.".into(),
        ));
    }
    let engine = engines.get("lz4")?;
    let _lease = scheduler
        .acquire("lz4", ResourceProfile::ARCHIVE, &cancellation)
        .await?;
    let preferred_name = lz4_decompressed_name(input);
    let request = OutputRequest {
        source: input.to_path_buf(),
        source_root: source_root.map(Path::to_path_buf),
        desired_extension: None,
        operation_suffix: Some("decompresse".into()),
        policy: policy.clone(),
    };
    let plan = resolver.plan_named(&request, &preferred_name)?;
    if plan.skipped {
        return Ok((plan.final_path, true));
    }
    resolver.prepare(&plan).await?;
    let result = run_process(
        engine,
        &[
            OsString::from("-q"),
            OsString::from("-f"),
            OsString::from("-d"),
            input.as_os_str().into(),
            plan.temporary_path.as_os_str().into(),
        ],
        &cancellation,
    )
    .await;
    if let Err(error) = result {
        resolver.cleanup(&plan).await;
        return Err(error);
    }
    resolver.finalize(&plan).await?;
    Ok((plan.final_path, false))
}

fn lz4_decompressed_name(input: &Path) -> String {
    input
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("decompresse")
        .to_owned()
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
    let _lease = scheduler
        .acquire("archive", ResourceProfile::ARCHIVE, &cancellation)
        .await?;
    validate_archive(engine, input, &cancellation).await?;
    let plan = prepare_directory_output(input, source_root, policy, "extrait").await?;
    if plan.skipped {
        return Ok((plan.final_path, true));
    }
    let output_arg = OsString::from(format!("-o{}", plan.temporary_path.to_string_lossy()));
    let result = run_process(
        engine,
        &[
            OsString::from("x"),
            input.as_os_str().into(),
            output_arg,
            OsString::from("-y"),
        ],
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
    let _lease = scheduler
        .acquire("qpdf", ResourceProfile::PDF, &cancellation)
        .await?;
    let plan = prepare_directory_output(input, source_root, policy, "pages").await?;
    if plan.skipped {
        return Ok((plan.final_path, true));
    }
    let pattern = plan.temporary_path.join("page-%d.pdf");
    let result = run_process(
        engine,
        &[
            OsString::from("--split-pages"),
            input.as_os_str().into(),
            pattern.as_os_str().into(),
        ],
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
    let _lease = scheduler
        .acquire("poppler", ResourceProfile::PDF, &cancellation)
        .await?;
    let plan = prepare_directory_output(input, source_root, policy, "images").await?;
    if plan.skipped {
        return Ok((plan.final_path, true));
    }
    let prefix = plan.temporary_path.join("page");
    let result = run_process(
        engine,
        &[
            OsString::from("-png"),
            OsString::from("-r"),
            OsString::from("150"),
            input.as_os_str().into(),
            prefix.as_os_str().into(),
        ],
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
        fileflow_domain::DestinationPolicy::SameFolder
        | fileflow_domain::DestinationPolicy::AskEveryTime => source_parent.to_path_buf(),
        fileflow_domain::DestinationPolicy::Subfolder => {
            source_parent.join(safe_folder_name(&policy.subfolder_name))
        }
        fileflow_domain::DestinationPolicy::CustomFolder => policy
            .custom_directory
            .clone()
            .ok_or(fileflow_output::OutputError::MissingCustomDirectory)?,
    };
    if policy.preserve_tree
        && let Some(root) = source_root
        && let Ok(relative) = input.strip_prefix(root)
        && let Some(relative_parent) = relative
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
    {
        parent = parent.join(relative_parent);
    }
    tokio::fs::create_dir_all(&parent).await?;
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
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
    Ok(DirectoryOutputPlan {
        final_path: target,
        temporary_path,
        skipped,
    })
}

async fn finalize_directory_output(plan: &DirectoryOutputPlan) -> Result<(), ExecutionError> {
    if plan.skipped {
        return Ok(());
    }
    tokio::fs::rename(&plan.temporary_path, &plan.final_path).await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveFamilySummary {
    pub family: FormatFamily,
    pub count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntryPreview {
    pub path: String,
    pub size_bytes: u64,
    pub family: FormatFamily,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveInspection {
    pub entries: usize,
    pub files: usize,
    pub directories: usize,
    pub total_unpacked_bytes: u64,
    pub families: Vec<ArchiveFamilySummary>,
    pub samples: Vec<ArchiveEntryPreview>,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
}

pub async fn extract_archive_entry_preview(
    engine: &Path,
    input: &Path,
    entry_path: &str,
    cancellation: &CancellationToken,
) -> Result<PathBuf, ExecutionError> {
    validate_archive_path(entry_path)?;
    validate_archive(engine, input, cancellation).await?;
    let root = std::env::temp_dir().join("fileflow-previews");
    cleanup_preview_cache(&root);
    let destination = root.join(Uuid::new_v4().simple().to_string());
    tokio::fs::create_dir_all(&destination).await?;
    let out_arg = format!("-o{}", destination.to_string_lossy());
    let result = run_process(
        engine,
        &[
            OsString::from("x"),
            OsString::from("-y"),
            OsString::from(out_arg),
            input.as_os_str().into(),
            OsString::from(entry_path),
        ],
        cancellation,
    )
    .await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_dir_all(&destination).await;
        return Err(error);
    }
    let normalized_entry = entry_path.replace('\\', "/");
    let extracted = normalized_entry
        .split('/')
        .filter(|part| !part.is_empty())
        .fold(destination.clone(), |base, part| base.join(part));
    let metadata = tokio::fs::metadata(&extracted).await.map_err(|_| {
        ExecutionError::InvalidInput("L’entrée de l’archive n’a pas pu être prévisualisée.".into())
    })?;
    if !metadata.is_file() || metadata.len() > 256 * 1024 * 1024 {
        let _ = tokio::fs::remove_dir_all(&destination).await;
        return Err(ExecutionError::InvalidInput(
            "Cette entrée est trop volumineuse pour un aperçu local.".into(),
        ));
    }
    Ok(extracted)
}

fn cleanup_preview_cache(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > Duration::from_secs(24 * 60 * 60));
        if stale {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

pub async fn inspect_archive(
    engine: &Path,
    input: &Path,
    offset: usize,
    limit: usize,
    cancellation: &CancellationToken,
) -> Result<ArchiveInspection, ExecutionError> {
    let listing = capture_process(
        engine,
        &[
            OsString::from("l"),
            OsString::from("-slt"),
            input.as_os_str().into(),
        ],
        cancellation,
    )
    .await?;
    validate_archive_listing(&listing)?;
    parse_archive_listing(&listing, offset, limit)
}

#[derive(Default)]
struct ArchiveListingEntry {
    path: Option<String>,
    size: u64,
    folder: bool,
    attributes: Option<String>,
}

struct ArchiveParseState {
    registry: FormatRegistry,
    family_counts: HashMap<FormatFamily, (usize, u64)>,
    samples: Vec<ArchiveEntryPreview>,
    sample_offset: usize,
    sample_limit: usize,
    files: usize,
    directories: usize,
    unpacked: u64,
}

impl ArchiveParseState {
    fn new(sample_offset: usize, sample_limit: usize) -> Self {
        Self {
            registry: FormatRegistry,
            family_counts: HashMap::new(),
            samples: Vec::new(),
            sample_offset,
            sample_limit,
            files: 0,
            directories: 0,
            unpacked: 0,
        }
    }

    fn flush(&mut self, entry: &mut ArchiveListingEntry) -> Result<(), ExecutionError> {
        let Some(path) = entry.path.take() else {
            *entry = ArchiveListingEntry::default();
            return Ok(());
        };
        validate_archive_path(&path)?;
        let is_directory = entry.folder
            || path.ends_with('/')
            || entry
                .attributes
                .as_deref()
                .is_some_and(|attributes| attributes.starts_with('D'));
        if is_directory {
            self.directories = self.directories.saturating_add(1);
        } else {
            let file_index = self.files;
            self.files = self.files.saturating_add(1);
            self.unpacked = self.unpacked.saturating_add(entry.size);
            let detected = self.registry.detect(Path::new(&path), &[]);
            let family = detected.family;
            let family_entry = self.family_counts.entry(family).or_default();
            family_entry.0 = family_entry.0.saturating_add(1);
            family_entry.1 = family_entry.1.saturating_add(entry.size);
            if file_index >= self.sample_offset
                && file_index < self.sample_offset.saturating_add(self.sample_limit)
            {
                self.samples.push(ArchiveEntryPreview {
                    path,
                    size_bytes: entry.size,
                    family,
                });
            }
        }
        *entry = ArchiveListingEntry::default();
        Ok(())
    }
}

fn parse_archive_listing(
    listing: &str,
    offset: usize,
    limit: usize,
) -> Result<ArchiveInspection, ExecutionError> {
    let mut in_entries = false;
    let mut entry = ArchiveListingEntry::default();
    let mut state = ArchiveParseState::new(offset, limit);

    for raw_line in listing.lines() {
        let line = raw_line.trim();
        if line.starts_with("----------") {
            in_entries = true;
            continue;
        }
        if !in_entries {
            continue;
        }
        if line.is_empty() {
            state.flush(&mut entry)?;
            continue;
        }
        if let Some(value) = line.strip_prefix("Path = ") {
            if entry.path.is_some() {
                state.flush(&mut entry)?;
            }
            entry.path = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("Size = ") {
            entry.size = value.trim().parse::<u64>().unwrap_or(0);
        } else if let Some(value) = line.strip_prefix("Folder = ") {
            entry.folder = matches!(value.trim(), "+" | "1" | "true");
        } else if let Some(value) = line.strip_prefix("Attributes = ") {
            entry.attributes = Some(value.to_owned());
        }
    }
    state.flush(&mut entry)?;

    let ArchiveParseState {
        family_counts,
        samples,
        files,
        directories,
        unpacked: total_unpacked_bytes,
        ..
    } = state;

    let mut families = family_counts
        .into_iter()
        .map(|(family, (count, total_bytes))| ArchiveFamilySummary {
            family,
            count,
            total_bytes,
        })
        .collect::<Vec<_>>();
    families.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.family.cmp(&right.family))
    });

    Ok(ArchiveInspection {
        entries: files.saturating_add(directories),
        files,
        directories,
        total_unpacked_bytes,
        families,
        samples,
        offset,
        limit,
        has_more: files > offset.saturating_add(limit),
    })
}

const MAX_ARCHIVE_ENTRIES: usize = 20_000;
const MAX_ARCHIVE_UNPACKED_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_RATIO: u64 = 500;

async fn validate_archive(
    engine: &Path,
    input: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let output = capture_process(
        engine,
        &[
            OsString::from("l"),
            OsString::from("-slt"),
            input.as_os_str().into(),
        ],
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
                return Err(ExecutionError::UnsafeArchive(format!(
                    "plus de {MAX_ARCHIVE_ENTRIES} entrées"
                )));
            }
            validate_archive_path(value)?;
        } else if let Some(value) = line.strip_prefix("Size = ") {
            unpacked = unpacked.saturating_add(value.trim().parse::<u64>().unwrap_or(0));
            if unpacked > MAX_ARCHIVE_UNPACKED_BYTES {
                return Err(ExecutionError::UnsafeArchive(
                    "taille décompressée supérieure à 20 Gio".into(),
                ));
            }
        } else if line.starts_with("Symbolic Link = ") || line.starts_with("Hard Link = ") {
            return Err(ExecutionError::UnsafeArchive(
                "les liens contenus dans une archive ne sont pas extraits automatiquement".into(),
            ));
        }
    }

    if physical_size > 0 && unpacked > 1024 * 1024 * 1024 {
        let ratio = unpacked / physical_size.max(1);
        if ratio > MAX_ARCHIVE_RATIO {
            return Err(ExecutionError::UnsafeArchive(format!(
                "ratio de compression suspect ({ratio}:1)"
            )));
        }
    }
    Ok(())
}

fn validate_archive_path(value: &str) -> Result<(), ExecutionError> {
    let normalized = value.replace('\\', "/");
    let drive_prefix = normalized
        .as_bytes()
        .get(1)
        .is_some_and(|value| *value == b':');
    if normalized.starts_with('/')
        || normalized.starts_with("//")
        || drive_prefix
        || normalized.split('/').any(|part| part == "..")
    {
        return Err(ExecutionError::UnsafeArchive(format!(
            "chemin non sûr : {value}"
        )));
    }
    Ok(())
}

async fn resolve_directory_conflict(
    base: PathBuf,
    strategy: fileflow_domain::ConflictStrategy,
) -> Result<PathBuf, ExecutionError> {
    if !base.exists()
        || matches!(
            strategy,
            fileflow_domain::ConflictStrategy::Skip | fileflow_domain::ConflictStrategy::Replace
        )
    {
        return Ok(base);
    }
    if strategy == fileflow_domain::ConflictStrategy::Ask {
        return Err(ExecutionError::Destination(format!(
            "la destination existe déjà : {}",
            base.display()
        )));
    }
    let parent = base.parent().unwrap_or_else(|| Path::new("."));
    let stem = base
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("extraction");
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
        .map(|character| match character {
            '/' | '\\' | ':' | '\0' => '-',
            other => other,
        })
        .collect::<String>();
    if cleaned.is_empty() {
        "FileFlow".into()
    } else {
        cleaned
    }
}

fn file_url(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('\\', "/");
    if !value.starts_with('/') {
        value.insert(0, '/');
    }
    let encoded = value
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23")
        .replace('?', "%3F");
    format!("file://{encoded}")
}

fn process_timeout(engine: &Path) -> Duration {
    let name = engine
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.contains("soffice") || name.contains("libreoffice") {
        Duration::from_secs(180)
    } else if name.contains("ffmpeg") || name.contains("ocrmypdf") || name.contains("tesseract") {
        Duration::from_secs(60 * 60)
    } else if name.contains("7z") {
        Duration::from_secs(15 * 60)
    } else {
        Duration::from_secs(10 * 60)
    }
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
    configure_external_command(&mut command);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let program = engine
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("engine")
        .to_owned();
    let future = command.output();
    tokio::pin!(future);
    let output = tokio::select! {
        result = &mut future => result?,
        _ = cancellation.cancelled() => return Err(ExecutionError::Cancelled),
        _ = tokio::time::sleep(process_timeout(engine)) => {
            return Err(ExecutionError::ProcessFailed {
                program: program.clone(),
                message: "délai maximal de traitement dépassé; le processus a été arrêté".into(),
            });
        }
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
    if cancellation.is_cancelled() {
        return Err(ExecutionError::Cancelled);
    }
    let mut command = Command::new(engine);
    configure_external_command(&mut command);
    command
        .args(args)
        .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let program = engine
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("engine")
        .to_owned();
    let future = command.output();
    tokio::pin!(future);
    let output = tokio::select! {
        result = &mut future => result?,
        _ = cancellation.cancelled() => return Err(ExecutionError::Cancelled),
        _ = tokio::time::sleep(process_timeout(engine)) => {
            return Err(ExecutionError::ProcessFailed {
                program: program.clone(),
                message: "délai maximal de traitement dépassé; le processus a été arrêté".into(),
            });
        }
    };
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(ExecutionError::ProcessFailed {
        program,
        message: tail_message(&stderr, output.status),
    })
}

async fn run_process(
    engine: &Path,
    args: &[OsString],
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    if cancellation.is_cancelled() {
        return Err(ExecutionError::Cancelled);
    }
    let mut command = Command::new(engine);
    configure_external_command(&mut command);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let program = engine
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("engine")
        .to_owned();
    let future = command.output();
    tokio::pin!(future);
    let output = tokio::select! {
        result = &mut future => result?,
        _ = cancellation.cancelled() => return Err(ExecutionError::Cancelled),
        _ = tokio::time::sleep(process_timeout(engine)) => {
            return Err(ExecutionError::ProcessFailed {
                program: program.clone(),
                message: "délai maximal de traitement dépassé; le processus a été arrêté".into(),
            });
        }
    };
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(ExecutionError::ProcessFailed {
        program,
        message: tail_message(&stderr, output.status),
    })
}

fn tail_message(stderr: &str, status: std::process::ExitStatus) -> String {
    let message = stderr
        .chars()
        .rev()
        .take(4000)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    if message.trim().is_empty() {
        format!("exit status {status}")
    } else {
        message
    }
}

fn profile_for(engine: &str) -> ResourceProfile {
    match engine {
        "ffmpeg" => ResourceProfile::MEDIA,
        "vips" | "imagemagick" => ResourceProfile::IMAGE,
        "office" => ResourceProfile::OFFICE,
        "ocr" | "tesseract" => ResourceProfile::OCR,
        "archive" | "zstd" | "lz4" => ResourceProfile::ARCHIVE,
        "qpdf" | "ghostscript" | "poppler" => ResourceProfile::PDF,
        "pandoc" => ResourceProfile::LIGHT,
        _ => ResourceProfile::LIGHT,
    }
}

fn is_audio_extension(extension: Option<&str>) -> bool {
    extension.is_some_and(|extension| {
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "mp3"
                | "wav"
                | "aac"
                | "m4a"
                | "flac"
                | "ogg"
                | "opus"
                | "wma"
                | "aiff"
                | "aif"
                | "alac"
                | "ape"
                | "ac3"
                | "eac3"
                | "ec3"
                | "dts"
                | "amr"
        )
    })
}

fn normalize_target_format(
    action_id: &str,
    format: Option<&str>,
) -> Result<Option<String>, ExecutionError> {
    let Some(format) = format else {
        return Ok(None);
    };
    let normalized = format.trim().trim_start_matches('.').to_ascii_lowercase();
    let allowed: &[&str] = match action_id {
        "image-convert" | "image-batch-convert" | "image-optimize" | "image-resize" => &[
            "jpg", "jpeg", "png", "webp", "avif", "heic", "heif", "jxl", "tif", "tiff", "bmp",
            "gif",
        ],
        "audio-convert" | "extract-audio" => &["mp3", "m4a", "aac", "wav", "flac", "ogg", "opus"],
        "video-convert" => &["mp4", "webm", "mkv", "mov"],
        "office-convert" => &[
            "pdf", "docx", "odt", "rtf", "txt", "html", "xlsx", "ods", "csv", "pptx", "odp",
        ],
        "text-convert" => &["html", "md", "docx", "epub", "txt"],
        "ebook-convert" => &["html", "md", "docx", "txt", "epub"],
        "archive-create" => &["zip", "7z", "tar"],
        "archive-package" => &[
            "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4",
        ],
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
    match quality
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("small") => Some("small".into()),
        Some("high") => Some("high".into()),
        Some("balanced") | Some("standard") => Some("balanced".into()),
        _ => None,
    }
}

pub const EXECUTABLE_ACTIONS: &[&str] = &[
    "smart-to-pdf",
    "collection-to-pdf",
    "images-to-pdf",
    "image-convert",
    "image-batch-convert",
    "image-optimize",
    "image-resize",
    "image-rotate-left",
    "image-rotate-right",
    "image-rotate-180",
    "image-rotate",
    "image-flip-horizontal",
    "image-flip-vertical",
    "image-auto-orient",
    "image-grayscale",
    "image-sepia",
    "image-auto-enhance",
    "image-adjust",
    "image-sharpen",
    "image-blur",
    "image-noise-reduce",
    "image-threshold",
    "image-posterize",
    "image-pixelate",
    "image-flatten",
    "image-trim",
    "image-crop-center",
    "image-resize-exact",
    "image-crop-custom",
    "image-canvas",
    "image-auto-gamma",
    "image-contrast-stretch",
    "image-colorspace-srgb",
    "image-set-dpi",
    "image-perspective",
    "image-border",
    "image-vignette",
    "image-watermark",
    "strip-metadata",
    "extract-metadata",
    "office-to-pdf",
    "office-convert",
    "pdf-merge",
    "pdf-split",
    "pdf-rotate-pages",
    "pdf-select-pages",
    "pdf-linearize",
    "pdf-optimize-lossless",
    "pdf-repair",
    "pdf-flatten-rotation",
    "pdf-flatten-annotations",
    "pdf-protect",
    "pdf-compress",
    "pdf-to-images",
    "pdf-extract-text",
    "pdf-ocr",
    "ocr-image",
    "archive-extract",
    "archive-create",
    "archive-package",
    "tar-zstd-create",
    "tar-lz4-create",
    "zstd-compress",
    "zstd-decompress",
    "lz4-compress",
    "lz4-decompress",
    "media-compatible",
    "video-convert",
    "video-rotate",
    "video-resize",
    "video-mute",
    "video-thumbnail",
    "media-trim",
    "audio-normalize",
    "audio-gain",
    "audio-mono",
    "media-compress",
    "audio-convert",
    "extract-audio",
    "video-to-gif",
    "text-to-pdf",
    "text-convert",
    "ebook-convert",
];

pub fn executable_action_ids() -> Vec<&'static str> {
    EXECUTABLE_ACTIONS.to_vec()
}

pub fn is_supported(action_id: &str) -> bool {
    EXECUTABLE_ACTIONS.contains(&action_id)
}

fn is_collective(action_id: &str) -> bool {
    matches!(
        action_id,
        "smart-to-pdf"
            | "collection-to-pdf"
            | "images-to-pdf"
            | "pdf-merge"
            | "archive-create"
            | "archive-package"
            | "tar-zstd-create"
            | "tar-lz4-create"
    )
}

async fn send_phase(
    events: &mpsc::Sender<ExecutionEvent>,
    job_id: JobId,
    phase: &str,
    completed: usize,
    total: usize,
) -> Result<(), ExecutionError> {
    send(
        events,
        ExecutionEvent::Phase {
            job_id,
            phase: phase.to_owned(),
            completed,
            total,
        },
    )
    .await
}

async fn send(
    events: &mpsc::Sender<ExecutionEvent>,
    event: ExecutionEvent,
) -> Result<(), ExecutionError> {
    events
        .send(event)
        .await
        .map_err(|_| ExecutionError::EventConsumerDisconnected)
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
        assert!(matches!(
            validate_archive_listing(listing),
            Err(ExecutionError::UnsafeArchive(_))
        ));
    }

    #[test]
    fn normalizes_only_supported_target_formats() {
        assert_eq!(
            normalize_target_format("image-convert", Some(".WEBP"))
                .unwrap()
                .as_deref(),
            Some("webp")
        );
        assert!(normalize_target_format("image-convert", Some("../../pdf")).is_err());
        assert_eq!(
            normalize_target_format("pdf-compress", Some("png")).unwrap(),
            None
        );
    }

    #[test]
    fn executor_capability_list_matches_runtime_guard() {
        assert!(is_supported("pdf-merge"));
        assert!(is_supported("video-to-gif"));
        assert!(is_supported("zstd-compress"));
        assert!(is_supported("lz4-decompress"));
        assert!(is_supported("tar-zstd-create"));
        assert!(is_supported("tar-lz4-create"));
        assert!(!is_supported("duplicate-scan"));
        assert!(
            executable_action_ids()
                .iter()
                .all(|action| is_supported(action))
        );
    }
    #[test]
    fn parses_archive_manifest_by_file_family() {
        let listing = "Physical Size = 1000\n----------\nPath = photos\nFolder = +\nSize = 0\n\nPath = photos/a.jpg\nFolder = -\nSize = 120\n\nPath = docs/report.pdf\nFolder = -\nSize = 900\n\n";
        let manifest = parse_archive_listing(listing, 0, 24).unwrap();
        assert_eq!(manifest.files, 2);
        assert_eq!(manifest.directories, 1);
        assert_eq!(manifest.total_unpacked_bytes, 1020);
        assert!(
            manifest
                .families
                .iter()
                .any(|entry| entry.family == FormatFamily::Image && entry.count == 1)
        );
        assert!(
            manifest
                .families
                .iter()
                .any(|entry| entry.family == FormatFamily::Pdf && entry.count == 1)
        );
    }

    #[test]
    fn zstd_output_name_strips_compression_suffix() {
        assert_eq!(
            zstd_decompressed_name(Path::new("backup.tar.zst")),
            "backup.tar"
        );
        assert_eq!(
            zstd_decompressed_name(Path::new("backup.tzst")),
            "backup.tar"
        );
        assert_eq!(zstd_decompressed_name(Path::new("data.zstd")), "data");
    }

    #[test]
    fn lz4_output_name_strips_compression_suffix() {
        assert_eq!(
            lz4_decompressed_name(Path::new("dataset.csv.lz4")),
            "dataset.csv"
        );
        assert_eq!(lz4_decompressed_name(Path::new("backup.lz4")), "backup");
    }

    #[test]
    fn smart_pdf_helpers_accept_only_safe_intermediate_extensions() {
        assert_eq!(safe_conversion_extension("jpeg"), Some("jpg"));
        assert_eq!(safe_conversion_extension("tiff"), Some("tiff"));
        assert_eq!(safe_conversion_extension("docx"), Some("docx"));
        assert_eq!(safe_conversion_extension("../../escape"), None);
    }

    #[test]
    fn smart_pdf_ordering_preserves_selection_or_sorts_by_name() {
        let original = vec![PathBuf::from("z.pdf"), PathBuf::from("A.pdf")];
        let mut selected = original.clone();
        sort_pdf_inputs(&mut selected, Some("selection"));
        assert_eq!(selected, original);

        let mut named = original;
        sort_pdf_inputs(&mut named, Some("name"));
        assert_eq!(named, vec![PathBuf::from("A.pdf"), PathBuf::from("z.pdf")]);
    }

    #[test]
    fn archive_pagination_returns_only_requested_window() {
        let listing = "Physical Size = 1000\n----------\nPath = a.jpg\nFolder = -\nSize = 10\n\nPath = b.png\nFolder = -\nSize = 20\n\nPath = c.pdf\nFolder = -\nSize = 30\n\n";
        let page = parse_archive_listing(listing, 1, 1).unwrap();
        assert_eq!(page.files, 3);
        assert_eq!(page.samples.len(), 1);
        assert_eq!(page.samples[0].path, "b.png");
        assert!(page.has_more);
    }

    #[test]
    fn collection_noise_filters_platform_metadata() {
        assert!(is_collection_noise(Path::new("__MACOSX/._photo.jpg")));
        assert!(is_collection_noise(Path::new("folder/.DS_Store")));
        assert!(is_collection_noise(Path::new("Thumbs.db")));
        assert!(!is_collection_noise(Path::new("documents/photo.jpg")));
    }

    #[test]
    fn smart_pdf_actions_are_executable_and_collective_when_expected() {
        assert!(is_supported("smart-to-pdf"));
        assert!(is_supported("collection-to-pdf"));
        assert!(is_supported("pdf-protect"));
        assert!(is_supported("text-to-pdf"));
        assert!(is_collective("smart-to-pdf"));
        assert!(is_collective("collection-to-pdf"));
        assert!(!is_collective("pdf-protect"));
    }

    #[test]
    fn ebook_conversion_accepts_only_pandoc_readable_inputs() {
        assert!(validate_pandoc_ebook_input(Path::new("book.epub")).is_ok());
        assert!(validate_pandoc_ebook_input(Path::new("book.fb2")).is_ok());
        assert!(validate_pandoc_ebook_input(Path::new("book.mobi")).is_err());
        assert!(validate_pandoc_ebook_input(Path::new("comic.cbr")).is_err());
    }

    #[test]
    fn image_pdf_list_is_nul_separated() {
        let inputs = vec![
            ExecutionInput {
                path: PathBuf::from("a.jpg"),
                source_root: None,
            },
            ExecutionInput {
                path: PathBuf::from("b image.png"),
                source_root: None,
            },
        ];
        let bytes = nul_separated_paths(&inputs);
        assert!(bytes.ends_with(&[0]));
        assert_eq!(bytes.iter().filter(|byte| **byte == 0).count(), 2);
    }
}
