//! Bounded job execution, process lifecycle and cancellation.
//!
//! External programs are always launched directly through `Command`; FileFlow
//! never interpolates paths into a shell command. Batch work is windowed and
//! every item must acquire a resource lease from the scheduler first.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
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
    io::{self, SeekFrom},
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use thiserror::Error;
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    process::{Child, Command},
    sync::mpsc,
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

// HTML rendering must remain bounded even when a page contains an infinite
// loop, a blocked resource, a modal dialog or a browser process that refuses
// to exit. This deadline is deliberately independent from the generic engine
// timeout (10 minutes) used by long-running conversions.
const BROWSER_PRINT_TIMEOUT: Duration = Duration::from_secs(30);
const BROWSER_SCRIPT_TIMEOUT: Duration = Duration::from_secs(14);
const BROWSER_STATIC_TIMEOUT: Duration = Duration::from_secs(7);
const BROWSER_TEXT_TIMEOUT: Duration = Duration::from_secs(6);
const PREVIEW_RENDER_TIMEOUT: Duration = Duration::from_secs(20);
const NATIVE_TEXT_PREVIEW_BYTES: u64 = 2 * 1024 * 1024;
const NATIVE_TEXT_PREVIEW_CHARS: usize = 250_000;

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

    // Every engine gets its own process group. Cancellation and timeouts can
    // therefore stop helpers created by Chrome, LibreOffice, FFmpeg, etc.
    // without ever targeting an unrelated user process by executable name.
    command.process_group(0);
}

#[cfg(all(unix, not(target_os = "linux")))]
fn configure_external_command(command: &mut Command) {
    command.process_group(0);
}

#[cfg(windows)]
fn configure_external_command(command: &mut Command) {
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    // Conversion engines are command-line tools. Never let Windows create a
    // console window while FileFlow is running them in the background.
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(any(unix, windows)))]
fn configure_external_command(_command: &mut Command) {}

struct ManagedChild {
    child: Child,
    #[cfg(unix)]
    process_group: Option<i32>,
    #[cfg(windows)]
    job: Option<WindowsJob>,
}

impl ManagedChild {
    fn spawn(command: &mut Command) -> io::Result<Self> {
        command.kill_on_drop(true);
        let child = command.spawn()?;

        #[cfg(unix)]
        let process_group = child
            .id()
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| {
                io::Error::other("le moteur a été lancé sans identifiant de processus valide")
            })?;

        #[cfg(windows)]
        let job = Some(WindowsJob::attach(&child)?);

        Ok(Self {
            child,
            #[cfg(unix)]
            process_group: Some(process_group),
            #[cfg(windows)]
            job,
        })
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    async fn terminate(&mut self) {
        if self.child.try_wait().ok().flatten().is_some() {
            self.cleanup_descendants();
            return;
        }

        #[cfg(unix)]
        if let Some(process_group) = self.process_group {
            // SAFETY: a negative PID targets exactly the process group created
            // for this child. No process name matching or shell is involved.
            unsafe {
                libc::kill(-process_group, libc::SIGTERM);
            }
        }

        #[cfg(windows)]
        if let Some(job) = self.job.as_ref() {
            job.terminate();
        }

        if tokio::time::timeout(Duration::from_millis(700), self.child.wait())
            .await
            .is_err()
        {
            self.force_stop();
            let _ = tokio::time::timeout(Duration::from_secs(1), self.child.wait()).await;
        }
        self.cleanup_descendants();
    }

    fn force_stop(&mut self) {
        #[cfg(unix)]
        if let Some(process_group) = self.process_group {
            // SAFETY: see the SIGTERM call above. SIGKILL is the bounded
            // fallback for engines that ignore a graceful shutdown request.
            unsafe {
                libc::kill(-process_group, libc::SIGKILL);
            }
        }

        #[cfg(windows)]
        if let Some(job) = self.job.as_ref() {
            job.terminate();
        }

        let _ = self.child.start_kill();
    }

    fn cleanup_descendants(&mut self) {
        #[cfg(unix)]
        if let Some(process_group) = self.process_group.take() {
            // The main engine has ended; any process still in its private group
            // is an orphan helper and must not survive the conversion.
            unsafe {
                libc::kill(-process_group, libc::SIGKILL);
            }
        }

        #[cfg(windows)]
        if let Some(job) = self.job.take() {
            job.terminate();
        }
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        self.force_stop();
    }
}

#[cfg(windows)]
struct WindowsJob {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: a Win32 kernel HANDLE can be used and closed from any thread. The
// WindowsJob value owns the handle and never aliases ownership of it.
#[cfg(windows)]
unsafe impl Send for WindowsJob {}

#[cfg(windows)]
impl WindowsJob {
    fn attach(child: &Child) -> io::Result<Self> {
        use std::{mem::size_of, ptr};
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                SetInformationJobObject,
            },
        };

        // SAFETY: the unnamed job has no security descriptor, both pointers
        // are valid for the documented Win32 calls, and the handle is owned by
        // WindowsJob immediately after successful creation.
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            let error = io::Error::last_os_error();
            unsafe {
                CloseHandle(handle);
            }
            return Err(error);
        }

        let process_handle = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("le moteur s’est arrêté avant son confinement"))?
            as windows_sys::Win32::Foundation::HANDLE;
        let assigned = unsafe { AssignProcessToJobObject(handle, process_handle) };
        if assigned == 0 {
            let error = io::Error::last_os_error();
            unsafe {
                CloseHandle(handle);
            }
            return Err(error);
        }
        Ok(Self { handle })
    }

    fn terminate(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        // SAFETY: handle remains owned and valid for this WindowsJob.
        unsafe {
            TerminateJobObject(self.handle, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;

        // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE guarantees that helpers cannot be
        // orphaned if FileFlow itself exits while a conversion is active.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

struct ManagedOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

async fn wait_for_managed_output(
    process: &mut ManagedChild,
    program: &str,
    cancellation: &CancellationToken,
    timeout: Duration,
) -> Result<ManagedOutput, ExecutionError> {
    let mut stdout = process.child.stdout.take();
    let mut stderr = process.child.stderr.take();
    let mut stdout_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        if let Some(pipe) = stdout.as_mut() {
            pipe.read_to_end(&mut bytes).await?;
        }
        Ok::<_, io::Error>(bytes)
    });
    let mut stderr_task = tokio::spawn(async move {
        let mut bytes = Vec::new();
        if let Some(pipe) = stderr.as_mut() {
            pipe.read_to_end(&mut bytes).await?;
        }
        Ok::<_, io::Error>(bytes)
    });

    enum Outcome {
        Completed(io::Result<ExitStatus>),
        Cancelled,
        TimedOut,
    }

    let outcome = tokio::select! {
        result = process.child.wait() => Outcome::Completed(result),
        _ = cancellation.cancelled() => Outcome::Cancelled,
        _ = tokio::time::sleep(timeout) => Outcome::TimedOut,
    };

    match outcome {
        Outcome::Completed(result) => {
            let status = match result {
                Ok(status) => status,
                Err(error) => {
                    process.terminate().await;
                    stdout_task.abort();
                    stderr_task.abort();
                    return Err(error.into());
                }
            };
            process.cleanup_descendants();
            let stdout = finish_pipe_read(&mut stdout_task).await?;
            let stderr = finish_pipe_read(&mut stderr_task).await?;
            Ok(ManagedOutput {
                status,
                stdout,
                stderr,
            })
        }
        Outcome::Cancelled => {
            process.terminate().await;
            stdout_task.abort();
            stderr_task.abort();
            Err(ExecutionError::Cancelled)
        }
        Outcome::TimedOut => {
            process.terminate().await;
            stdout_task.abort();
            stderr_task.abort();
            Err(ExecutionError::ProcessFailed {
                program: program.to_owned(),
                message: format!(
                    "délai maximal de traitement dépassé ({} s); le processus et ses assistants ont été arrêtés",
                    timeout.as_secs()
                ),
            })
        }
    }
}

async fn finish_pipe_read(
    task: &mut tokio::task::JoinHandle<io::Result<Vec<u8>>>,
) -> io::Result<Vec<u8>> {
    match tokio::time::timeout(Duration::from_secs(1), &mut *task).await {
        Ok(result) => result.map_err(io::Error::other)?,
        Err(_) => {
            task.abort();
            Ok(Vec::new())
        }
    }
}

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
    BytesProgress {
        job_id: JobId,
        processed_bytes: u64,
        total_bytes: u64,
        output_bytes: u64,
        bytes_per_second: u64,
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
        "html-to-pdf" => {
            validate_source_extension(input, &["html", "htm"], "HTML")?;
            run_browser_print(engine, input, &plan.temporary_path, true, &cancellation).await
        }
        "email-to-pdf" => {
            validate_source_extension(input, &["eml", "mail"], "EML")?;
            run_eml_to_pdf(engine, input, &plan.temporary_path, &cancellation).await
        }
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
            let job = TemporaryJobWorkspace::create().await?;
            let normalized_root = job.path("normalized-images");
            tokio::fs::create_dir_all(&normalized_root).await?;
            let mut normalized = Vec::with_capacity(request.inputs.len());
            for (index, input) in request.inputs.iter().enumerate() {
                let detected = detect_path(&input.path).await?;
                if detected.family != FormatFamily::Image {
                    resolver.cleanup(&plan).await;
                    return Err(ExecutionError::InvalidInput(format!(
                        "{} n’est pas une image compatible.",
                        input.path.display()
                    )));
                }
                if matches!(detected.id.as_str(), "jpeg" | "png" | "tiff") {
                    normalized.push(input.clone());
                    continue;
                }
                let output = normalized_root.join(format!("{index:05}.png"));
                normalize_image_for_pdf(
                    &input.path,
                    &output,
                    engines,
                    scheduler.clone(),
                    &cancellation,
                )
                .await?;
                normalized.push(ExecutionInput {
                    path: output,
                    source_root: None,
                });
            }
            let _lease = scheduler
                .acquire("img2pdf", ResourceProfile::PDF, &cancellation)
                .await?;
            let list_path = plan.destination_directory.join(format!(
                ".fileflow-img2pdf-{}.list",
                Uuid::new_v4().simple()
            ));
            tokio::fs::write(&list_path, nul_separated_paths(&normalized)).await?;
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
            let lease = scheduler
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
            let args = qpdf_merge_args(
                request.inputs.iter().map(|input| input.path.as_path()),
                &plan.temporary_path,
            );
            if let Err(error) = run_process(engine, &args, &cancellation).await {
                resolver.cleanup(&plan).await;
                return Err(error);
            }
            drop(lease);
            if let Err(error) = validate_pdf_output(
                &plan.temporary_path,
                engines,
                scheduler.clone(),
                &cancellation,
            )
            .await
            {
                resolver.cleanup(&plan).await;
                return Err(error);
            }
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
                job_id,
                &events,
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
                job_id,
                &events,
            )
            .await
        }
        "archive-package" => {
            execute_archive_package(
                request,
                engines,
                scheduler,
                cancellation,
                resolver,
                job_id,
                &events,
            )
            .await
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
            let result = run_vips_copy(
                engine,
                input,
                output,
                scheduler.budget().cpu_tokens.max(1),
                cancellation,
            )
            .await;
            if result.is_ok() {
                return Ok(());
            }
            let vips_error = result.expect_err("the successful branch returned above");
            let Ok(imagemagick) = engines.get("imagemagick") else {
                return Err(vips_error);
            };
            drop(_lease);
            let _fallback_lease = scheduler
                .acquire("imagemagick", ResourceProfile::IMAGE, cancellation)
                .await?;
            let _ = tokio::fs::remove_file(output).await;
            run_process(
                imagemagick,
                &[input.as_os_str().into(), output.as_os_str().into()],
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
        "browser" => match step.from.as_str() {
            "html" => run_browser_print(engine, input, output, true, cancellation).await,
            "eml" => run_eml_to_pdf(engine, input, output, cancellation).await,
            other => Err(ExecutionError::InvalidInput(format!(
                "Le navigateur PDF n’est pas autorisé comme route intermédiaire depuis {other}."
            ))),
        },
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

async fn normalize_image_for_pdf(
    input: &Path,
    output: &Path,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    if let Ok(vips) = engines.get("vips") {
        let _lease = scheduler
            .acquire("vips", ResourceProfile::IMAGE, cancellation)
            .await?;
        if run_vips_copy(
            vips,
            input,
            output,
            scheduler.budget().cpu_tokens.max(1),
            cancellation,
        )
        .await
        .is_ok()
        {
            return Ok(());
        }
        let _ = tokio::fs::remove_file(output).await;
    }
    if let Ok(imagemagick) = engines.get("imagemagick") {
        let _lease = scheduler
            .acquire("imagemagick", ResourceProfile::IMAGE, cancellation)
            .await?;
        return run_process(
            imagemagick,
            &[input.as_os_str().into(), output.as_os_str().into()],
            cancellation,
        )
        .await;
    }
    Err(ExecutionError::MissingEngine("vips ou imagemagick".into()))
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
    let args = qpdf_merge_args(inputs.iter().map(PathBuf::as_path), output);
    run_process(qpdf, &args, cancellation).await
}

fn qpdf_merge_args<'a>(inputs: impl IntoIterator<Item = &'a Path>, output: &Path) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("--warning-exit-0"),
        OsString::from("--empty"),
        OsString::from("--pages"),
    ];
    args.extend(inputs.into_iter().map(|input| input.as_os_str().into()));
    args.extend([OsString::from("--"), output.as_os_str().into()]);
    args
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
    let extension = input
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "html" | "htm") {
        return run_browser_print(engines.get("browser")?, input, output, true, cancellation).await;
    }
    if matches!(extension.as_str(), "eml" | "mail") {
        return run_eml_to_pdf(engines.get("browser")?, input, output, cancellation).await;
    }

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

fn validate_source_extension(
    input: &Path,
    allowed: &[&str],
    label: &str,
) -> Result<(), ExecutionError> {
    let extension = input
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if allowed.contains(&extension.as_str()) {
        Ok(())
    } else {
        Err(ExecutionError::InvalidInput(format!(
            "Cette action attend un fichier {label}."
        )))
    }
}

async fn run_browser_print(
    browser: &Path,
    input: &Path,
    output: &Path,
    javascript: bool,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let result = tokio::time::timeout(
        BROWSER_PRINT_TIMEOUT,
        run_browser_print_with_fallbacks(browser, input, output, javascript, cancellation),
    )
    .await;
    match result {
        Ok(result) => result,
        Err(_) => {
            // Dropping the bounded future also drops the active Chromium child;
            // `kill_on_drop(true)` guarantees it cannot continue in background.
            let _ = tokio::fs::remove_file(output).await;
            Err(ExecutionError::ProcessFailed {
                program: browser
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("browser")
                    .to_owned(),
                message: format!(
                    "délai global de conversion HTML dépassé ({} s); le processus a été arrêté",
                    BROWSER_PRINT_TIMEOUT.as_secs()
                ),
            })
        }
    }
}

async fn run_browser_print_with_fallbacks(
    browser: &Path,
    input: &Path,
    output: &Path,
    javascript: bool,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let input = tokio::fs::canonicalize(input).await?;
    let preserve_saved_dom = javascript && browser_should_preserve_saved_dom(&input).await?;
    let primary = if preserve_saved_dom {
        run_browser_sanitized_attempt(
            browser,
            &input,
            output,
            true,
            BROWSER_STATIC_TIMEOUT,
            cancellation,
        )
        .await
    } else {
        let timeout = if javascript {
            BROWSER_SCRIPT_TIMEOUT
        } else {
            BROWSER_STATIC_TIMEOUT
        };
        run_browser_print_attempt(browser, &input, output, timeout, cancellation).await
    };
    if primary.is_ok() || !javascript || cancellation.is_cancelled() {
        return primary;
    }

    // A saved Gmail page already contains its visible DOM. Running its scripts
    // can replace that snapshot with Google's temporary-error screen because
    // the local copy no longer owns the original authenticated web session.
    // Such pages therefore use a sanitized copy from the first attempt;
    // ordinary pages retain dynamic-first rendering and then use that copy.
    let primary_error = primary.expect_err("the successful branch returned above");
    let secondary = run_browser_sanitized_attempt(
        browser,
        &input,
        output,
        false,
        BROWSER_STATIC_TIMEOUT,
        cancellation,
    )
    .await;
    if secondary.is_ok() || cancellation.is_cancelled() {
        return secondary;
    }

    // If even the saved DOM references too many remote resources, reduce it to
    // a self-contained readable document. The generated page contains no
    // scripts, frames, stylesheets or network URLs and therefore cannot keep
    // Chromium busy indefinitely.
    let secondary_error = secondary.expect_err("the successful branch returned above");
    let (snapshot_root, snapshot) = create_browser_text_snapshot(&input).await?;
    let text_fallback = run_browser_print_attempt(
        browser,
        &snapshot,
        output,
        BROWSER_TEXT_TIMEOUT,
        cancellation,
    )
    .await;
    let _ = tokio::fs::remove_dir_all(&snapshot_root).await;
    match text_fallback {
        Ok(()) => Ok(()),
        Err(ExecutionError::Cancelled) => Err(ExecutionError::Cancelled),
        Err(text_error) => Err(ExecutionError::ProcessFailed {
            program: browser
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("browser")
                .to_owned(),
            message: format!(
                "le premier rendu a échoué ({primary_error}); le second rendu a échoué ({secondary_error}); le secours textuel a échoué ({text_error})"
            ),
        }),
    }
}

async fn browser_should_preserve_saved_dom(input: &Path) -> Result<bool, ExecutionError> {
    const MAX_SNIFF_BYTES: u64 = 8 * 1024 * 1024;
    let file_name = input
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if file_name.contains("gmail") {
        return Ok(true);
    }

    let file = tokio::fs::File::open(input).await?;
    let mut limited = file.take(MAX_SNIFF_BYTES);
    let mut bytes = Vec::new();
    limited.read_to_end(&mut bytes).await?;
    let source = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    if source.contains("mail.google.com") || (source.contains("<title") && source.contains("gmail"))
    {
        return Ok(true);
    }

    let saved_from_browser = source.contains("saved from url=")
        || source.contains("saved from url =")
        || source.contains("enregistrée depuis l’url")
        || source.contains("enregistree depuis l'url");
    Ok(saved_from_browser && browser_has_companion_resources(input).await)
}

async fn browser_has_companion_resources(input: &Path) -> bool {
    let Some(parent) = input.parent() else {
        return false;
    };
    let Some(stem) = input.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    for suffix in ["_files", "_fichiers", ".files", " files", " fichiers"] {
        let candidate = parent.join(format!("{stem}{suffix}"));
        if tokio::fs::metadata(candidate)
            .await
            .is_ok_and(|metadata| metadata.is_dir())
        {
            return true;
        }
    }
    false
}

async fn run_browser_sanitized_attempt(
    browser: &Path,
    input: &Path,
    output: &Path,
    allow_local_images: bool,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let (snapshot_root, snapshot) =
        create_browser_sanitized_snapshot(input, allow_local_images).await?;
    let result = run_browser_print_attempt(browser, &snapshot, output, timeout, cancellation).await;
    let _ = tokio::fs::remove_dir_all(snapshot_root).await;
    result
}

async fn create_browser_sanitized_snapshot(
    input: &Path,
    allow_local_images: bool,
) -> Result<(PathBuf, PathBuf), ExecutionError> {
    const MAX_HTML_BYTES: u64 = 64 * 1024 * 1024;
    let metadata = tokio::fs::metadata(input).await?;
    if metadata.len() > MAX_HTML_BYTES {
        return Err(ExecutionError::InvalidInput(
            "La page HTML dépasse 64 Mo et ne peut pas être sécurisée localement.".into(),
        ));
    }
    let bytes = tokio::fs::read(input).await?;
    let source = String::from_utf8_lossy(&bytes);
    let document = browser_sanitized_document(
        &source,
        input.parent().unwrap_or_else(|| Path::new(".")),
        allow_local_images,
    );
    let root = std::env::temp_dir()
        .join("fileflow-browser-sanitized")
        .join(Uuid::new_v4().simple().to_string());
    tokio::fs::create_dir_all(&root).await?;
    let snapshot = root.join("snapshot.html");
    tokio::fs::write(&snapshot, document).await?;
    Ok((root, snapshot))
}

fn browser_sanitized_document(
    source: &str,
    source_directory: &Path,
    allow_local_images: bool,
) -> String {
    let document = remove_html_block(source, "script");
    let document = remove_html_block(&document, "iframe");
    let document = remove_html_block(&document, "object");
    let mut base_url = file_url(source_directory);
    if !base_url.ends_with('/') {
        base_url.push('/');
    }
    let image_sources = if allow_local_images {
        "file: data: blob:"
    } else {
        "data:"
    };
    let head = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'none'; connect-src 'none'; frame-src 'none'; object-src 'none'; base-uri file:; img-src {image_sources}; style-src 'unsafe-inline' file: data:; font-src file: data:; media-src file: data: blob:\"><base href=\"{}\">",
        escape_html(&base_url)
    );
    insert_html_head_content(&document, &head)
}

fn insert_html_head_content(document: &str, content: &str) -> String {
    let lower = document.to_ascii_lowercase();
    if let Some(head_start) = find_html_tag(&lower, "<head", 0)
        && let Some(relative_end) = lower[head_start..].find('>')
    {
        let insertion = head_start + relative_end + 1;
        let mut output = String::with_capacity(document.len() + content.len());
        output.push_str(&document[..insertion]);
        output.push_str(content);
        output.push_str(&document[insertion..]);
        return output;
    }
    if let Some(html_start) = find_html_tag(&lower, "<html", 0)
        && let Some(relative_end) = lower[html_start..].find('>')
    {
        let insertion = html_start + relative_end + 1;
        let mut output = String::with_capacity(document.len() + content.len() + 13);
        output.push_str(&document[..insertion]);
        output.push_str("<head>");
        output.push_str(content);
        output.push_str("</head>");
        output.push_str(&document[insertion..]);
        return output;
    }
    format!("<!doctype html><html><head>{content}</head><body>{document}</body></html>")
}

async fn create_browser_text_snapshot(input: &Path) -> Result<(PathBuf, PathBuf), ExecutionError> {
    const MAX_HTML_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_TEXT_CHARS: usize = 2_000_000;
    let metadata = tokio::fs::metadata(input).await?;
    if metadata.len() > MAX_HTML_BYTES {
        return Err(ExecutionError::InvalidInput(
            "La page HTML dépasse 64 Mo et ne peut pas utiliser le secours textuel.".into(),
        ));
    }
    let bytes = tokio::fs::read(input).await?;
    let source = String::from_utf8_lossy(&bytes);
    let plain = html_to_plain_text(&source);
    let truncated = plain.chars().count() > MAX_TEXT_CHARS;
    let visible = plain.chars().take(MAX_TEXT_CHARS).collect::<String>();
    let title = input
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Page HTML");
    let document = browser_text_snapshot_document(title, &visible, truncated);
    let root = std::env::temp_dir()
        .join("fileflow-browser-snapshot")
        .join(Uuid::new_v4().simple().to_string());
    tokio::fs::create_dir_all(&root).await?;
    let snapshot = root.join("snapshot.html");
    tokio::fs::write(&snapshot, document).await?;
    Ok((root, snapshot))
}

fn browser_text_snapshot_document(title: &str, text: &str, truncated: bool) -> String {
    let truncation = if truncated {
        "<p class=warning>Le contenu extrêmement volumineux a été limité à deux millions de caractères.</p>"
    } else {
        ""
    };
    format!(
        "<!doctype html><html lang=fr><head><meta charset=utf-8><title>{}</title><style>@page{{size:A4;margin:17mm}}*{{box-sizing:border-box}}body{{margin:0;color:#172033;font:12px/1.5 -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif}}header{{margin-bottom:18px;padding:14px 16px;border:1px solid #dfe5ef;border-radius:12px;background:#f5f7fb}}h1{{margin:0 0 6px;font-size:16px}}p{{margin:0;color:#5f687a}}.warning{{margin-top:8px;color:#9a5b00}}main{{white-space:pre-wrap;overflow-wrap:anywhere}}</style></head><body><header><h1>Copie lisible créée par FileFlow</h1><p>Les scripts et ressources distantes ont été retirés car la page web originale ne terminait pas son chargement.</p>{truncation}</header><main>{}</main></body></html>",
        escape_html(title),
        escape_html(text)
    )
}

async fn run_browser_print_attempt(
    browser: &Path,
    input: &Path,
    output: &Path,
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let (attempt_root, profile, browser_output, browser_log) = browser_attempt_paths();
    tokio::fs::create_dir_all(&profile).await?;

    let mut args = vec![
        OsString::from("--headless=new"),
        OsString::from("--disable-gpu"),
        OsString::from("--disable-extensions"),
        OsString::from("--disable-sync"),
        OsString::from("--disable-background-networking"),
        OsString::from("--disable-background-timer-throttling"),
        OsString::from("--disable-backgrounding-occluded-windows"),
        OsString::from("--disable-renderer-backgrounding"),
        OsString::from("--disable-component-update"),
        OsString::from("--disable-default-apps"),
        OsString::from("--disable-features=Translate,MediaRouter,OptimizationHints"),
        OsString::from("--no-first-run"),
        OsString::from("--no-default-browser-check"),
        OsString::from("--hide-scrollbars"),
        OsString::from("--run-all-compositor-stages-before-draw"),
        OsString::from("--virtual-time-budget=5000"),
        OsString::from("--host-resolver-rules=MAP * ~NOTFOUND"),
        OsString::from("--proxy-server=socks5://127.0.0.1:9"),
        OsString::from("--proxy-bypass-list=<-loopback>"),
        OsString::from("--no-pdf-header-footer"),
        OsString::from(format!("--user-data-dir={}", profile.display())),
        OsString::from(format!("--print-to-pdf={}", browser_output.display())),
    ];
    args.push(OsString::from(file_url(input)));

    // Keep Chromium's OS sandbox enabled. If the host installation cannot run
    // sandboxed, FileFlow fails explicitly instead of weakening isolation.
    // Chromium receives a visible, conventional `render.pdf` path. FileFlow's
    // final temporary output is intentionally hidden on Unix; passing that
    // dotfile directly to Chrome can make it exit successfully without writing
    // anything. The completed PDF is copied to FileFlow's destination below.
    let render_result = run_browser_process(
        browser,
        &args,
        &browser_output,
        &browser_log,
        cancellation,
        timeout,
    )
    .await;
    let result = match render_result {
        Ok(()) => match browser_complete_pdf_size(&browser_output).await {
            Ok(Some(_)) => tokio::fs::copy(&browser_output, output)
                .await
                .map(|_| ())
                .map_err(ExecutionError::from),
            Ok(None) => Err(ExecutionError::InvalidInput(
                "Le navigateur n’a produit aucun PDF complet.".into(),
            )),
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    };
    let _ = tokio::fs::remove_dir_all(&attempt_root).await;
    if result.is_err() {
        // Chromium can create a partial PDF before hanging. Never expose that
        // incomplete file as a successful conversion or preview.
        let _ = tokio::fs::remove_file(output).await;
    }
    result?;
    if !output.is_file() {
        return Err(ExecutionError::InvalidInput(
            "Le navigateur n’a produit aucun PDF.".into(),
        ));
    }
    Ok(())
}

fn browser_attempt_paths() -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir()
        .join("fileflow-browser")
        .join(Uuid::new_v4().simple().to_string());
    let profile = root.join("profile");
    let output = root.join("render.pdf");
    let log = root.join("browser.log");
    (root, profile, output, log)
}

async fn run_browser_process(
    browser: &Path,
    args: &[OsString],
    browser_output: &Path,
    browser_log: &Path,
    cancellation: &CancellationToken,
    timeout: Duration,
) -> Result<(), ExecutionError> {
    if cancellation.is_cancelled() {
        return Err(ExecutionError::Cancelled);
    }

    // Chrome helper processes may inherit stderr. `Command::output()` then
    // waits for every inherited pipe to close and can report a false timeout
    // after Chrome has already written the PDF. A regular log file preserves
    // diagnostics without tying process completion to pipe ownership.
    let log = std::fs::File::create(browser_log)?;
    let mut command = Command::new(browser);
    configure_external_command(&mut command);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .kill_on_drop(true);
    let program = browser
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("browser")
        .to_owned();
    let mut child = ManagedChild::spawn(&mut command)?;
    let started = Instant::now();
    let mut last_complete_size = None;

    loop {
        if cancellation.is_cancelled() {
            stop_browser_child(&mut child).await;
            return Err(ExecutionError::Cancelled);
        }

        let complete_size = browser_complete_pdf_size(browser_output).await?;
        if let Some(size) = complete_size {
            // `%%EOF` plus an unchanged size on two consecutive polls means
            // the PDF is complete. Do not wait for an unrelated Chrome helper
            // process that keeps the parent alive.
            if last_complete_size == Some(size) {
                stop_browser_child(&mut child).await;
                return Ok(());
            }
            last_complete_size = Some(size);
        } else {
            last_complete_size = None;
        }

        if let Some(status) = child.try_wait()? {
            if status.success() && complete_size.is_some() {
                return Ok(());
            }
            if status.success() {
                return Err(ExecutionError::InvalidInput(
                    "Le navigateur n’a produit aucun PDF.".into(),
                ));
            }
            return Err(ExecutionError::ProcessFailed {
                program,
                message: browser_log_tail(browser_log, status).await,
            });
        }

        if started.elapsed() >= timeout {
            stop_browser_child(&mut child).await;
            return Err(ExecutionError::ProcessFailed {
                program,
                message: format!(
                    "délai maximal de traitement dépassé ({} s); le processus a été arrêté",
                    timeout.as_secs()
                ),
            });
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn stop_browser_child(child: &mut ManagedChild) {
    child.terminate().await;
}

async fn browser_complete_pdf_size(path: &Path) -> Result<Option<u64>, ExecutionError> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.len() < 10 {
        return Ok(None);
    }

    let mut file = tokio::fs::File::open(path).await?;
    let mut header = [0_u8; 5];
    if let Err(error) = file.read_exact(&mut header).await {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            return Ok(None);
        }
        return Err(error.into());
    }
    if &header != b"%PDF-" {
        return Ok(None);
    }

    let tail_size = metadata.len().min(2048) as usize;
    file.seek(SeekFrom::End(-(tail_size as i64))).await?;
    let mut tail = vec![0_u8; tail_size];
    if let Err(error) = file.read_exact(&mut tail).await {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            return Ok(None);
        }
        return Err(error.into());
    }
    if tail
        .windows(b"%%EOF".len())
        .any(|window| window == b"%%EOF")
    {
        Ok(Some(metadata.len()))
    } else {
        Ok(None)
    }
}

async fn browser_log_tail(log: &Path, status: std::process::ExitStatus) -> String {
    match tokio::fs::read(log).await {
        Ok(bytes) => tail_message(&String::from_utf8_lossy(&bytes), status),
        Err(_) => format!("exit status {status}"),
    }
}

async fn run_eml_to_pdf(
    browser: &Path,
    input: &Path,
    output: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    const MAX_EMAIL_BYTES: u64 = 64 * 1024 * 1024;
    let metadata = tokio::fs::metadata(input).await?;
    if metadata.len() > MAX_EMAIL_BYTES {
        return Err(ExecutionError::InvalidInput(
            "Cet e-mail dépasse 64 Mo. Enregistrez d’abord ses pièces jointes séparément.".into(),
        ));
    }
    let bytes = tokio::fs::read(input).await?;
    let raw = String::from_utf8_lossy(&bytes);
    let (header_block, body_block) = split_header_body(&raw);
    let headers = parse_mail_headers(header_block);
    let body = extract_mail_text(&headers, body_block, 0);
    let subject = decoded_header(&headers, "subject").unwrap_or_else(|| "Sans objet".into());
    let sender = decoded_header(&headers, "from").unwrap_or_else(|| "—".into());
    let recipients = decoded_header(&headers, "to").unwrap_or_else(|| "—".into());
    let copy = decoded_header(&headers, "cc");
    let date = decoded_header(&headers, "date").unwrap_or_else(|| "—".into());
    let cc_row = copy.map_or_else(String::new, |value| {
        format!(
            "<div class=\"meta-row\"><span>Copie</span><strong>{}</strong></div>",
            escape_html(&value)
        )
    });
    let document = format!(
        "<!doctype html><html lang=\"fr\"><head><meta charset=\"utf-8\"><style>@page{{size:A4;margin:18mm}}*{{box-sizing:border-box}}body{{margin:0;color:#182033;font:14px/1.55 -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif}}.mail{{border:1px solid #dfe5ef;border-radius:18px;overflow:hidden}}header{{padding:26px 28px;background:linear-gradient(135deg,#f4f8ff,#f7f5ff)}}.kicker{{color:#687086;font-size:11px;font-weight:800;letter-spacing:.12em;text-transform:uppercase}}h1{{margin:8px 0 0;font-size:25px;line-height:1.15}}.meta{{padding:17px 28px;border-bottom:1px solid #e7ebf2}}.meta-row{{display:grid;grid-template-columns:76px 1fr;gap:12px;padding:5px 0}}.meta-row span{{color:#7b8498}}.meta-row strong{{font-weight:650;overflow-wrap:anywhere}}article{{padding:28px;white-space:pre-wrap;overflow-wrap:anywhere}}footer{{margin-top:14px;color:#8b93a3;font-size:10px;text-align:center}}</style></head><body><main class=\"mail\"><header><div class=\"kicker\">E-mail archivé par FileFlow</div><h1>{}</h1></header><section class=\"meta\"><div class=\"meta-row\"><span>De</span><strong>{}</strong></div><div class=\"meta-row\"><span>À</span><strong>{}</strong></div>{}<div class=\"meta-row\"><span>Date</span><strong>{}</strong></div></section><article>{}</article></main><footer>Les contenus distants et scripts de l’e-mail ne sont pas exécutés.</footer></body></html>",
        escape_html(&subject),
        escape_html(&sender),
        escape_html(&recipients),
        cc_row,
        escape_html(&date),
        escape_html(body.trim()),
    );
    let html = output
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".fileflow-email-{}.html", Uuid::new_v4().simple()));
    tokio::fs::write(&html, document).await?;
    let result = run_browser_print(browser, &html, output, false, cancellation).await;
    let _ = tokio::fs::remove_file(&html).await;
    result
}

type MailHeaders = HashMap<String, String>;

fn split_header_body(value: &str) -> (&str, &str) {
    value
        .split_once("\r\n\r\n")
        .or_else(|| value.split_once("\n\n"))
        .unwrap_or((value, ""))
}

fn parse_mail_headers(block: &str) -> MailHeaders {
    let mut unfolded = Vec::<String>::new();
    for line in block.replace("\r\n", "\n").lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = unfolded.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
        } else {
            unfolded.push(line.to_owned());
        }
    }
    unfolded
        .into_iter()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect()
}

fn decoded_header(headers: &MailHeaders, name: &str) -> Option<String> {
    headers.get(name).map(|value| decode_rfc2047(value))
}

fn extract_mail_text(headers: &MailHeaders, body: &str, depth: usize) -> String {
    if depth > 6 {
        return "[Contenu MIME trop profondément imbriqué]".into();
    }
    let content_type_header = headers
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| "text/plain".into());
    let content_type = content_type_header.to_ascii_lowercase();
    if content_type.starts_with("multipart/")
        && let Some(boundary) = mail_parameter(&content_type_header, "boundary")
    {
        let marker = format!("--{boundary}");
        let mut html_fallback = None;
        for part in body.split(&marker).skip(1) {
            let part = part.trim_start_matches(&['\r', '\n'][..]).trim_end();
            if part.is_empty() || part.starts_with("--") {
                continue;
            }
            let (part_headers_raw, part_body) = split_header_body(part);
            let part_headers = parse_mail_headers(part_headers_raw);
            if part_headers
                .get("content-disposition")
                .is_some_and(|value| value.to_ascii_lowercase().contains("attachment"))
            {
                continue;
            }
            let part_type_header = part_headers
                .get("content-type")
                .cloned()
                .unwrap_or_else(|| "text/plain".into());
            let part_type = part_type_header.to_ascii_lowercase();
            if part_type.starts_with("multipart/") {
                let nested = extract_mail_text(&part_headers, part_body, depth + 1);
                if !nested.trim().is_empty() {
                    return nested;
                }
            } else if part_type.starts_with("text/plain") {
                return decode_mail_body(&part_headers, part_body);
            } else if part_type.starts_with("text/html") {
                html_fallback = Some(html_to_plain_text(&decode_mail_body(
                    &part_headers,
                    part_body,
                )));
            }
        }
        return html_fallback.unwrap_or_else(|| "[Aucun corps de message texte]".into());
    }

    let decoded = decode_mail_body(headers, body);
    if content_type.starts_with("text/html") {
        html_to_plain_text(&decoded)
    } else {
        decoded
    }
}

fn mail_parameter(header: &str, name: &str) -> Option<String> {
    header.split(';').skip(1).find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().trim_matches('"').trim_matches('\'').to_owned())
    })
}

fn decode_mail_body(headers: &MailHeaders, body: &str) -> String {
    let encoding = headers
        .get("content-transfer-encoding")
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let bytes = match encoding.as_str() {
        "base64" => BASE64_STANDARD
            .decode(
                body.chars()
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>(),
            )
            .unwrap_or_else(|_| body.as_bytes().to_vec()),
        "quoted-printable" => decode_quoted_printable(body.as_bytes()),
        _ => body.as_bytes().to_vec(),
    };
    String::from_utf8_lossy(&bytes).into_owned()
}

fn decode_quoted_printable(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'=' {
            if input.get(index + 1) == Some(&b'\r') && input.get(index + 2) == Some(&b'\n') {
                index += 3;
                continue;
            }
            if input.get(index + 1) == Some(&b'\n') {
                index += 2;
                continue;
            }
            if let (Some(high), Some(low)) = (input.get(index + 1), input.get(index + 2))
                && let (Some(high), Some(low)) = (hex_value(*high), hex_value(*low))
            {
                output.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        output.push(input[index]);
        index += 1;
    }
    output
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn decode_rfc2047(value: &str) -> String {
    let mut output = String::new();
    let mut remainder = value;
    while let Some(start) = remainder.find("=?") {
        output.push_str(&remainder[..start]);
        let encoded = &remainder[start + 2..];
        let Some(first) = encoded.find('?') else {
            break;
        };
        let encoded = &encoded[first + 1..];
        let Some(second) = encoded.find('?') else {
            break;
        };
        let encoding = &encoded[..second];
        let payload = &encoded[second + 1..];
        let Some(end) = payload.find("?=") else { break };
        let bytes = if encoding.eq_ignore_ascii_case("b") {
            BASE64_STANDARD.decode(&payload[..end]).ok()
        } else if encoding.eq_ignore_ascii_case("q") {
            Some(decode_quoted_printable(
                payload[..end].replace('_', " ").as_bytes(),
            ))
        } else {
            None
        };
        if let Some(bytes) = bytes {
            output.push_str(&String::from_utf8_lossy(&bytes));
        } else {
            output.push_str(&remainder[start..start + 2 + first + 1 + second + 1 + end + 2]);
        }
        remainder = &payload[end + 2..];
    }
    output.push_str(remainder);
    output.trim().to_owned()
}

fn html_to_plain_text(value: &str) -> String {
    let value = remove_html_block(value, "script");
    let value = remove_html_block(&value, "style");
    let mut output = String::with_capacity(value.len());
    let mut inside_tag = false;
    let mut tag = String::new();
    for character in value.chars() {
        match character {
            '<' if !inside_tag => {
                inside_tag = true;
                tag.clear();
            }
            '>' if inside_tag => {
                inside_tag = false;
                let name = tag
                    .trim()
                    .trim_start_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                if matches!(
                    name.to_ascii_lowercase().as_str(),
                    "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3"
                ) {
                    output.push('\n');
                }
            }
            _ if inside_tag => tag.push(character),
            _ => output.push(character),
        }
    }
    output
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .lines()
        .map(str::trim_end)
        .fold(String::new(), |mut text, line| {
            if !(line.is_empty() && text.ends_with("\n\n")) {
                text.push_str(line);
                text.push('\n');
            }
            text
        })
}

fn remove_html_block(value: &str, element: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let opening = format!("<{element}");
    let closing = format!("</{element}");
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;

    while let Some(start) = find_html_tag(&lower, &opening, cursor) {
        output.push_str(&value[cursor..start]);
        let Some(relative_open_end) = lower[start..].find('>') else {
            cursor = value.len();
            break;
        };
        let open_end = start + relative_open_end + 1;
        if lower[start..open_end]
            .trim_end_matches('>')
            .trim_end()
            .ends_with('/')
        {
            cursor = open_end;
            continue;
        }
        let Some(close_start) = find_html_tag(&lower, &closing, open_end) else {
            cursor = value.len();
            break;
        };
        let Some(relative_close_end) = lower[close_start..].find('>') else {
            cursor = value.len();
            break;
        };
        cursor = close_start + relative_close_end + 1;
        output.push(' ');
    }
    output.push_str(&value[cursor..]);
    output
}

fn find_html_tag(value: &str, needle: &str, mut from: usize) -> Option<usize> {
    while let Some(relative) = value[from..].find(needle) {
        let index = from + relative;
        let boundary = value.as_bytes().get(index + needle.len()).copied();
        if boundary.is_none_or(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/')) {
            return Some(index);
        }
        from = index + needle.len();
    }
    None
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
            OsString::from("--warning-exit-0"),
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

#[derive(Debug)]
struct ArchiveCommandLayout {
    working_directory: PathBuf,
    entries: Vec<String>,
}

fn prepare_archive_inputs(
    inputs: &[ExecutionInput],
) -> Result<Vec<ExecutionInput>, ExecutionError> {
    let mut seen = HashSet::new();
    let mut prepared = Vec::new();
    for input in inputs {
        let metadata = std::fs::symlink_metadata(&input.path)?;
        if metadata.file_type().is_symlink() {
            return Err(ExecutionError::InvalidInput(format!(
                "{} est un lien symbolique et ne peut pas entrer dans l’archive sûre.",
                input.path.display()
            )));
        }
        // A workspace contains both directory assets and every file below them.
        // Passing both to 7-Zip makes each descendant appear twice. Directories
        // are therefore represented by their already enumerated regular files.
        if metadata.is_dir() {
            continue;
        }
        if !metadata.is_file() {
            return Err(ExecutionError::InvalidInput(format!(
                "{} n’est pas un fichier ordinaire.",
                input.path.display()
            )));
        }
        let canonical = std::fs::canonicalize(&input.path)?;
        if seen.insert(canonical) {
            prepared.push(input.clone());
        }
    }
    if prepared.is_empty() {
        return Err(ExecutionError::InvalidInput(
            "Aucun fichier ordinaire à placer dans l’archive.".into(),
        ));
    }
    Ok(prepared)
}

fn archive_base_candidate(input: &ExecutionInput) -> Option<PathBuf> {
    input
        .source_root
        .as_deref()
        .filter(|root| input.path.starts_with(root))
        .and_then(Path::parent)
        .or_else(|| input.path.parent())
        .map(Path::to_path_buf)
}

fn archive_command_layout(
    inputs: &[ExecutionInput],
) -> Result<ArchiveCommandLayout, ExecutionError> {
    let mut working_directory = inputs
        .first()
        .and_then(archive_base_candidate)
        .ok_or_else(|| ExecutionError::InvalidInput("Racine d’archive introuvable.".into()))?;
    for input in inputs.iter().skip(1) {
        let candidate = archive_base_candidate(input).ok_or_else(|| {
            ExecutionError::InvalidInput(format!(
                "Racine d’archive introuvable pour {}.",
                input.path.display()
            ))
        })?;
        while !candidate.starts_with(&working_directory) {
            if !working_directory.pop() {
                return Err(ExecutionError::InvalidInput(
                    "Les sources d’une même archive doivent se trouver sur un volume commun."
                        .into(),
                ));
            }
        }
    }
    if working_directory.as_os_str().is_empty() {
        return Err(ExecutionError::InvalidInput(
            "Racine d’archive commune introuvable.".into(),
        ));
    }

    let mut seen_entries = HashSet::new();
    let mut entries = Vec::with_capacity(inputs.len());
    for input in inputs {
        let relative = input.path.strip_prefix(&working_directory).map_err(|_| {
            ExecutionError::InvalidInput(format!(
                "{} ne peut pas être converti en chemin d’archive relatif.",
                input.path.display()
            ))
        })?;
        let mut components = Vec::new();
        for component in relative.components() {
            match component {
                std::path::Component::Normal(value) => {
                    let value = value.to_str().ok_or_else(|| {
                        ExecutionError::InvalidInput(
                            "Un nom de fichier d’archive n’est pas encodé en UTF-8.".into(),
                        )
                    })?;
                    if value.contains('\n') || value.contains('\r') {
                        return Err(ExecutionError::InvalidInput(
                            "Un nom de fichier contient un saut de ligne incompatible avec les archives."
                                .into(),
                        ));
                    }
                    components.push(value);
                }
                std::path::Component::CurDir => {}
                _ => {
                    return Err(ExecutionError::InvalidInput(
                        "Un chemin d’archive relatif contient une composante interdite.".into(),
                    ));
                }
            }
        }
        if components.is_empty() {
            return Err(ExecutionError::InvalidInput("Nom d’archive vide.".into()));
        }
        let entry = components.join("/");
        let collision_key = if cfg!(windows) {
            entry.to_ascii_lowercase()
        } else {
            entry.clone()
        };
        if !seen_entries.insert(collision_key) {
            return Err(ExecutionError::InvalidInput(format!(
                "Deux sources produisent le même chemin dans l’archive : {entry}."
            )));
        }
        entries.push(entry);
    }
    Ok(ArchiveCommandLayout {
        working_directory,
        entries,
    })
}

async fn create_archive_with_7zip(
    engine: &Path,
    archive_type: &str,
    compression: &str,
    output: &Path,
    inputs: &[ExecutionInput],
    list_directory: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    let layout = archive_command_layout(inputs)?;
    let list_path = list_directory.join(format!(
        ".fileflow-archive-{}.list",
        Uuid::new_v4().simple()
    ));
    let mut list = layout.entries.join("\n");
    list.push('\n');
    tokio::fs::write(&list_path, list).await?;
    let args = [
        OsString::from("a"),
        OsString::from(archive_type),
        OsString::from(compression),
        output.as_os_str().into(),
        OsString::from("-scsUTF-8"),
        OsString::from(format!("@{}", list_path.to_string_lossy())),
    ];
    let result =
        run_process_in_directory(engine, &args, &layout.working_directory, cancellation).await;
    let _ = tokio::fs::remove_file(&list_path).await;
    result
}

async fn execute_archive_package(
    request: &ExecutionRequest,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: CancellationToken,
    resolver: OutputResolver,
    job_id: JobId,
    events: &mpsc::Sender<ExecutionEvent>,
) -> Result<Option<PathBuf>, ExecutionError> {
    let target = match request.target_format.as_deref().unwrap_or("smart") {
        "smart" => "tar.zst",
        target => target,
    };
    if target == "tar.zst" {
        return execute_tar_compressed_archive(
            request,
            "zstd",
            "tar.zst",
            engines,
            scheduler,
            cancellation,
            resolver,
            job_id,
            events,
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
            job_id,
            events,
        )
        .await;
    }
    let archive_inputs = prepare_archive_inputs(&request.inputs)?;
    let archive_engine = engines.get("archive")?;
    let _lease = scheduler
        .acquire("archive", ResourceProfile::ARCHIVE, &cancellation)
        .await?;
    let first = archive_inputs
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
        create_archive_with_7zip(
            archive_engine,
            archive_type,
            match (target, request.quality.as_deref()) {
                ("zip", Some("high")) => "-mx=0",
                ("zip", Some("small")) => "-mx=5",
                ("zip", _) => "-mx=1",
                _ => "-mx=0",
            },
            &plan.temporary_path,
            &archive_inputs,
            &plan.destination_directory,
            &cancellation,
        )
        .await
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
        create_archive_with_7zip(
            archive_engine,
            "-ttar",
            "-mx=0",
            &staging_tar,
            &archive_inputs,
            &plan.destination_directory,
            &cancellation,
        )
        .await?;
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
    job_id: JobId,
    events: &mpsc::Sender<ExecutionEvent>,
) -> Result<Option<PathBuf>, ExecutionError> {
    let archive_inputs = prepare_archive_inputs(&request.inputs)?;
    let archive_engine = engines.get("archive")?;
    let compressor = engines.get(compressor_id)?;
    let first = archive_inputs
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

    if compressor_id == "zstd" {
        send_phase(events, job_id, "preparation", 1, 1).await?;
        let _compression_lease = scheduler
            .acquire("zstd", ResourceProfile::ARCHIVE, &cancellation)
            .await?;
        send_phase(events, job_id, "conversion", 0, 1).await?;
        let result = stream_tar_to_zstd(
            compressor,
            &archive_inputs,
            &plan.temporary_path,
            request.quality.as_deref(),
            scheduler.budget().cpu_tokens.max(1),
            &cancellation,
            job_id,
            events,
        )
        .await;
        if let Err(error) = result {
            resolver.cleanup(&plan).await;
            return Err(error);
        }
        send_phase(events, job_id, "finalisation", 1, 1).await?;
        resolver.finalize(&plan).await?;
        return Ok(Some(plan.final_path));
    }

    let staging_tar = plan
        .destination_directory
        .join(format!(".fileflow-stage-{}.tar", Uuid::new_v4().simple()));

    {
        let _archive_lease = scheduler
            .acquire("archive", ResourceProfile::ARCHIVE, &cancellation)
            .await?;
        if let Err(error) = create_archive_with_7zip(
            archive_engine,
            "-ttar",
            "-mx=0",
            &staging_tar,
            &archive_inputs,
            &plan.destination_directory,
            &cancellation,
        )
        .await
        {
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

#[allow(clippy::too_many_arguments)]
async fn stream_tar_to_zstd(
    zstd: &Path,
    inputs: &[ExecutionInput],
    output: &Path,
    quality: Option<&str>,
    thread_count: usize,
    cancellation: &CancellationToken,
    job_id: JobId,
    events: &mpsc::Sender<ExecutionEvent>,
) -> Result<(), ExecutionError> {
    let mut total_bytes = 0_u64;
    for input in inputs {
        let metadata = tokio::fs::symlink_metadata(&input.path).await?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ExecutionError::InvalidInput(format!(
                "{} n’est pas un fichier ordinaire et ne peut pas entrer dans l’archive sûre.",
                input.path.display()
            )));
        }
        total_bytes = total_bytes.saturating_add(metadata.len());
    }

    let level = match quality {
        Some("small") => "-15",
        Some("high") => "--fast=5",
        _ => "--fast=1",
    };
    let mut command = Command::new(zstd);
    configure_external_command(&mut command);
    command
        .args([
            OsString::from("-q"),
            OsString::from("-f"),
            OsString::from(format!("-T{}", thread_count.max(1))),
            OsString::from(level),
            OsString::from("-"),
            OsString::from("-o"),
            output.as_os_str().into(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = ManagedChild::spawn(&mut command)?;
    let mut writer = child
        .child
        .stdin
        .take()
        .ok_or_else(|| ExecutionError::ProcessFailed {
            program: "zstd".into(),
            message: "entrée standard indisponible".into(),
        })?;
    let started = Instant::now();
    let mut last_event = Instant::now() - Duration::from_secs(1);
    let mut processed = 0_u64;
    let mut buffer = vec![0_u8; 256 * 1024];

    for input in inputs {
        let metadata = tokio::fs::metadata(&input.path).await?;
        let entry_name = safe_tar_entry_name(input)?;
        let header = ustar_file_header(&entry_name, metadata.len(), &metadata)?;
        write_with_cancellation(&mut writer, &header, cancellation).await?;

        let mut source = tokio::fs::File::open(&input.path).await?;
        loop {
            let read = tokio::select! {
                result = source.read(&mut buffer) => result?,
                _ = cancellation.cancelled() => return Err(ExecutionError::Cancelled),
            };
            if read == 0 {
                break;
            }
            write_with_cancellation(&mut writer, &buffer[..read], cancellation).await?;
            processed = processed.saturating_add(read as u64);
            if last_event.elapsed() >= Duration::from_millis(120) {
                send_bytes_progress(events, job_id, processed, total_bytes, output, started)
                    .await?;
                last_event = Instant::now();
            }
        }
        let padding = (512 - metadata.len() % 512) % 512;
        if padding > 0 {
            let zeros = [0_u8; 512];
            write_with_cancellation(&mut writer, &zeros[..padding as usize], cancellation).await?;
        }
    }
    write_with_cancellation(&mut writer, &[0_u8; 1024], cancellation).await?;
    tokio::select! {
        result = writer.shutdown() => result?,
        _ = cancellation.cancelled() => return Err(ExecutionError::Cancelled),
    }
    drop(writer);

    send_bytes_progress(events, job_id, processed, total_bytes, output, started).await?;
    let result = wait_for_managed_output(
        &mut child,
        "zstd",
        cancellation,
        process_timeout(zstd),
    )
    .await?;
    if !result.status.success() {
        return Err(ExecutionError::ProcessFailed {
            program: "zstd".into(),
            message: tail_message(&String::from_utf8_lossy(&result.stderr), result.status),
        });
    }
    send_bytes_progress(events, job_id, total_bytes, total_bytes, output, started).await?;
    Ok(())
}

async fn write_with_cancellation(
    writer: &mut tokio::process::ChildStdin,
    bytes: &[u8],
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    tokio::select! {
        result = writer.write_all(bytes) => Ok(result?),
        _ = cancellation.cancelled() => Err(ExecutionError::Cancelled),
    }
}

async fn send_bytes_progress(
    events: &mpsc::Sender<ExecutionEvent>,
    job_id: JobId,
    processed_bytes: u64,
    total_bytes: u64,
    output: &Path,
    started: Instant,
) -> Result<(), ExecutionError> {
    let output_bytes = tokio::fs::metadata(output)
        .await
        .map(|value| value.len())
        .unwrap_or(0);
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    send(
        events,
        ExecutionEvent::BytesProgress {
            job_id,
            processed_bytes,
            total_bytes,
            output_bytes,
            bytes_per_second: (processed_bytes as f64 / elapsed).min(u64::MAX as f64) as u64,
        },
    )
    .await
}

fn safe_tar_entry_name(input: &ExecutionInput) -> Result<String, ExecutionError> {
    let relative = input
        .source_root
        .as_deref()
        .and_then(|root| input.path.strip_prefix(root).ok())
        .filter(|path| !path.as_os_str().is_empty());
    let mut parts = Vec::new();
    if relative.is_some()
        && let Some(root_name) = input
            .source_root
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
    {
        parts.push(root_name.to_owned());
    }
    let fallback = input
        .path
        .file_name()
        .map(Path::new)
        .ok_or_else(|| ExecutionError::InvalidInput("Nom de fichier introuvable.".into()))?;
    let path = relative.unwrap_or(fallback);
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => {
                let value = value.to_str().ok_or_else(|| {
                    ExecutionError::InvalidInput(
                        "Un nom de fichier n’est pas encodé en UTF-8.".into(),
                    )
                })?;
                if !value.is_empty() {
                    parts.push(value.to_owned());
                }
            }
            std::path::Component::CurDir => {}
            _ => {
                return Err(ExecutionError::InvalidInput(
                    "Un chemin d’archive contient une remontée ou une racine interdite.".into(),
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(ExecutionError::InvalidInput("Nom d’archive vide.".into()));
    }
    Ok(parts.join("/"))
}

fn ustar_file_header(
    entry_name: &str,
    size: u64,
    metadata: &std::fs::Metadata,
) -> Result<[u8; 512], ExecutionError> {
    let (name, prefix) = split_ustar_name(entry_name)?;
    let mut header = [0_u8; 512];
    copy_tar_field(&mut header[0..100], name.as_bytes())?;
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::MetadataExt;
        u64::from(metadata.mode() & 0o7777)
    };
    #[cfg(not(unix))]
    let mode = 0o644;
    write_tar_number(&mut header[100..108], mode);
    write_tar_number(&mut header[108..116], 0);
    write_tar_number(&mut header[116..124], 0);
    write_tar_number(&mut header[124..136], size);
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
        .unwrap_or(0);
    write_tar_number(&mut header[136..148], modified);
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    copy_tar_field(&mut header[265..297], b"fileflow")?;
    copy_tar_field(&mut header[297..329], b"fileflow")?;
    copy_tar_field(&mut header[345..500], prefix.as_bytes())?;
    let checksum = header.iter().map(|value| u64::from(*value)).sum::<u64>();
    let checksum = format!("{checksum:06o}");
    header[148..154].copy_from_slice(checksum.as_bytes());
    header[154] = 0;
    header[155] = b' ';
    Ok(header)
}

fn split_ustar_name(value: &str) -> Result<(&str, &str), ExecutionError> {
    if value.len() <= 100 {
        return Ok((value, ""));
    }
    let split = value
        .match_indices('/')
        .filter(|(index, _)| *index <= 155 && value.len().saturating_sub(*index + 1) <= 100)
        .map(|(index, _)| index)
        .next_back()
        .ok_or_else(|| {
            ExecutionError::InvalidInput(format!(
                "Le chemin « {value} » est trop long pour une archive TAR portable."
            ))
        })?;
    Ok((&value[split + 1..], &value[..split]))
}

fn copy_tar_field(field: &mut [u8], value: &[u8]) -> Result<(), ExecutionError> {
    if value.len() > field.len() {
        return Err(ExecutionError::InvalidInput(
            "Un champ dépasse la taille maximale du format TAR.".into(),
        ));
    }
    field[..value.len()].copy_from_slice(value);
    Ok(())
}

fn write_tar_number(field: &mut [u8], value: u64) {
    let octal = format!("{value:o}");
    if octal.len() < field.len() {
        field.fill(b'0');
        let start = field.len() - octal.len() - 1;
        field[start..start + octal.len()].copy_from_slice(octal.as_bytes());
        let last = field.len() - 1;
        field[last] = 0;
        return;
    }
    field.fill(0);
    let bytes = value.to_be_bytes();
    let copy = field.len().min(bytes.len());
    let field_start = field.len() - copy;
    let bytes_start = bytes.len() - copy;
    field[field_start..].copy_from_slice(&bytes[bytes_start..]);
    field[0] |= 0x80;
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
        "text-to-pdf" => (
            if matches!(
                source_extension.map(str::to_ascii_lowercase).as_deref(),
                Some("html" | "htm" | "eml" | "mail")
            ) {
                "browser"
            } else {
                "pandoc"
            },
            Some("pdf"),
            Some("pdf"),
        ),
        "html-to-pdf" => ("browser", Some("pdf"), Some("pdf")),
        "email-to-pdf" => ("browser", Some("pdf"), Some("pdf")),
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
            OsString::from("--norestore"),
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
    let mut args = vec![
        OsString::from("--warning-exit-0"),
        input.as_os_str().into(),
        output.as_os_str().into(),
    ];
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
        "pdf-repair" => {}
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
        OsString::from("-nostdin"),
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
            OsString::from("--warning-exit-0"),
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedFilePreview {
    pub path: PathBuf,
    pub family: FormatFamily,
    pub generated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

pub async fn prepare_file_preview(
    input: &Path,
    engines: EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: &CancellationToken,
) -> Result<PreparedFilePreview, ExecutionError> {
    let detected = detect_path(input).await?;
    let extension = input
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if detected.family == FormatFamily::Pdf {
        return Ok(PreparedFilePreview {
            path: input.to_path_buf(),
            family: FormatFamily::Pdf,
            generated: false,
            content: None,
        });
    }
    if detected.family == FormatFamily::Image
        && matches!(
            extension.as_str(),
            "jpg" | "jpeg" | "jpe" | "jfif" | "png" | "apng" | "gif" | "webp" | "svg"
        )
    {
        return Ok(PreparedFilePreview {
            path: input.to_path_buf(),
            family: FormatFamily::Image,
            generated: false,
            content: None,
        });
    }

    // Passive previews must never start the user's browser. Text, HTML and
    // e-mail files are converted to bounded plain text and rendered directly
    // by FileFlow's webview. Browser-backed PDF rendering remains available
    // only for an explicit conversion action such as HTML -> PDF.
    if detected.family == FormatFamily::Text {
        return Ok(PreparedFilePreview {
            path: input.to_path_buf(),
            family: FormatFamily::Text,
            generated: false,
            content: Some(native_text_preview(input, &extension).await?),
        });
    }

    let root = std::env::temp_dir().join("fileflow-previews");
    cleanup_preview_cache(&root);
    let destination = root.join(Uuid::new_v4().simple().to_string());
    tokio::fs::create_dir_all(&destination).await?;

    let result = match detected.family {
        FormatFamily::Image => {
            let output = destination.join("preview.png");
            prepare_image_thumbnail(input, &output, &engines, scheduler.clone(), cancellation)
                .await?;
            PreparedFilePreview {
                path: output,
                family: FormatFamily::Image,
                generated: true,
                content: None,
            }
        }
        FormatFamily::Document | FormatFamily::Spreadsheet | FormatFamily::Presentation => {
            let output = destination.join("preview.pdf");
            let office = engines.get("office")?;
            let _lease = scheduler
                .acquire("office", ResourceProfile::OFFICE, cancellation)
                .await?;
            run_office_convert(
                office,
                input,
                &transient_output_plan(&output),
                "pdf",
                cancellation,
            )
            .await?;
            PreparedFilePreview {
                path: output,
                family: FormatFamily::Pdf,
                generated: true,
                content: None,
            }
        }
        FormatFamily::Ebook if matches!(extension.as_str(), "epub" | "fb2") => {
            let output = destination.join("preview.txt");
            let pandoc = engines.get("pandoc")?;
            let _lease = scheduler
                .acquire("pandoc", ResourceProfile::LIGHT, cancellation)
                .await?;
            run_pandoc(pandoc, input, &output, cancellation).await?;
            let content = native_text_preview(&output, "txt").await?;
            PreparedFilePreview {
                path: output,
                family: FormatFamily::Text,
                generated: true,
                content: Some(content),
            }
        }
        FormatFamily::Video => {
            let output = destination.join("preview.jpg");
            let ffmpeg = engines.get("ffmpeg")?;
            let _lease = scheduler
                .acquire("ffmpeg", ResourceProfile::MEDIA, cancellation)
                .await?;
            run_ffmpeg(
                ffmpeg,
                "video-thumbnail",
                input,
                &output,
                Some("balanced"),
                &HashMap::new(),
                scheduler.budget().cpu_tokens.max(1),
                cancellation,
            )
            .await?;
            PreparedFilePreview {
                path: output,
                family: FormatFamily::Image,
                generated: true,
                content: None,
            }
        }
        _ => {
            let _ = tokio::fs::remove_dir_all(&destination).await;
            return Err(ExecutionError::InvalidInput(format!(
                "Le format {} ne possède pas de représentation visuelle fiable.",
                detected.id
            )));
        }
    };
    Ok(result)
}

async fn native_text_preview(input: &Path, extension: &str) -> Result<String, ExecutionError> {
    let metadata = tokio::fs::metadata(input).await?;
    let file = tokio::fs::File::open(input).await?;
    let mut bytes = Vec::new();
    file.take(NATIVE_TEXT_PREVIEW_BYTES + 1)
        .read_to_end(&mut bytes)
        .await?;
    let byte_truncated = metadata.len() > NATIVE_TEXT_PREVIEW_BYTES;
    bytes.truncate(NATIVE_TEXT_PREVIEW_BYTES as usize);
    let raw = String::from_utf8_lossy(&bytes);

    let text = match extension {
        "html" | "htm" | "fb2" => html_to_plain_text(&raw),
        "eml" | "mail" => {
            let (header_block, body_block) = split_header_body(&raw);
            let headers = parse_mail_headers(header_block);
            let subject =
                decoded_header(&headers, "subject").unwrap_or_else(|| "Sans objet".into());
            let sender = decoded_header(&headers, "from").unwrap_or_else(|| "—".into());
            let recipients = decoded_header(&headers, "to").unwrap_or_else(|| "—".into());
            let date = decoded_header(&headers, "date").unwrap_or_else(|| "—".into());
            let body = extract_mail_text(&headers, body_block, 0);
            format!(
                "Objet : {subject}\nDe : {sender}\nÀ : {recipients}\nDate : {date}\n\n{}",
                body.trim()
            )
        }
        _ => raw.into_owned(),
    };
    let char_truncated = text.chars().count() > NATIVE_TEXT_PREVIEW_CHARS;
    let mut visible = text
        .chars()
        .take(NATIVE_TEXT_PREVIEW_CHARS)
        .collect::<String>();
    if visible.trim().is_empty() {
        visible = "[Fichier vide]".into();
    }
    if byte_truncated || char_truncated {
        visible.push_str("\n\n[Aperçu limité par FileFlow]");
    }
    Ok(visible)
}

/// Prepares an extracted archive entry for the desktop viewer.
///
/// PDF plug-ins are not consistently available in embedded Linux webviews, so
/// archive PDFs use a bounded first-page PNG when Poppler is present. Other
/// supported formats reuse the normal preview pipeline. If an optional preview
/// engine is absent or rejects the file, the caller still receives the safely
/// extracted entry and can show an informative format placeholder.
pub async fn prepare_archive_entry_file_preview(
    input: &Path,
    engines: EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: &CancellationToken,
) -> Result<PreparedFilePreview, ExecutionError> {
    let detected = detect_path(input).await?;
    let direct = || PreparedFilePreview {
        path: input.to_path_buf(),
        family: detected.family,
        generated: false,
        content: None,
    };

    if detected.family == FormatFamily::Pdf {
        return match prepare_pdf_first_page_preview(input, &engines, scheduler, cancellation).await
        {
            Ok(preview) => Ok(preview),
            Err(ExecutionError::Cancelled) => Err(ExecutionError::Cancelled),
            Err(_) => Ok(direct()),
        };
    }

    match prepare_file_preview(input, engines, scheduler, cancellation).await {
        Ok(preview) => Ok(preview),
        Err(ExecutionError::Cancelled) => Err(ExecutionError::Cancelled),
        Err(_) => Ok(direct()),
    }
}

async fn prepare_pdf_first_page_preview(
    input: &Path,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: &CancellationToken,
) -> Result<PreparedFilePreview, ExecutionError> {
    let engine = engines.get("poppler")?;
    let _lease = scheduler
        .acquire("poppler-preview", ResourceProfile::PDF, cancellation)
        .await?;
    let root = std::env::temp_dir().join("fileflow-previews");
    cleanup_preview_cache(&root);
    let destination = root.join(Uuid::new_v4().simple().to_string());
    tokio::fs::create_dir_all(&destination).await?;
    let prefix = destination.join("first-page");
    let output = destination.join("first-page.png");
    let result = run_process_with_timeout(
        engine,
        &pdf_first_page_preview_args(input, &prefix),
        cancellation,
        PREVIEW_RENDER_TIMEOUT,
    )
    .await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_dir_all(&destination).await;
        return Err(error);
    }
    let metadata = match tokio::fs::metadata(&output).await {
        Ok(metadata) => metadata,
        Err(_) => {
            let _ = tokio::fs::remove_dir_all(&destination).await;
            return Err(ExecutionError::InvalidInput(
                "Poppler n’a produit aucun aperçu du PDF.".into(),
            ));
        }
    };
    if !metadata.is_file() || metadata.len() == 0 {
        let _ = tokio::fs::remove_dir_all(&destination).await;
        return Err(ExecutionError::InvalidInput(
            "Poppler a produit un aperçu PDF vide.".into(),
        ));
    }
    Ok(PreparedFilePreview {
        path: output,
        family: FormatFamily::Image,
        generated: true,
        content: None,
    })
}

fn pdf_first_page_preview_args(input: &Path, prefix: &Path) -> Vec<OsString> {
    vec![
        OsString::from("-f"),
        OsString::from("1"),
        OsString::from("-l"),
        OsString::from("1"),
        OsString::from("-singlefile"),
        OsString::from("-scale-to"),
        OsString::from("1800"),
        OsString::from("-png"),
        input.as_os_str().into(),
        prefix.as_os_str().into(),
    ]
}

async fn prepare_image_thumbnail(
    input: &Path,
    output: &Path,
    engines: &EnginePaths,
    scheduler: Arc<ResourceScheduler>,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    if let Ok(vips) = engines.get("vips") {
        let _lease = scheduler
            .acquire("vips-preview", ResourceProfile::IMAGE, cancellation)
            .await?;
        if run_vips_thumbnail(
            vips,
            input,
            output,
            Some("balanced"),
            scheduler.budget().cpu_tokens.max(1),
            cancellation,
        )
        .await
        .is_ok()
        {
            return Ok(());
        }
        let _ = tokio::fs::remove_file(output).await;
    }
    let imagemagick = engines.get("imagemagick")?;
    let _lease = scheduler
        .acquire("imagemagick-preview", ResourceProfile::IMAGE, cancellation)
        .await?;
    run_process(
        imagemagick,
        &[
            input.as_os_str().into(),
            OsString::from("-auto-orient"),
            OsString::from("-thumbnail"),
            OsString::from("1800x1800>"),
            OsString::from("-strip"),
            output.as_os_str().into(),
        ],
        cancellation,
    )
    .await
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
    if is_browser_executable_name(&name) {
        BROWSER_PRINT_TIMEOUT
    } else if name.contains("soffice") || name.contains("libreoffice") {
        Duration::from_secs(180)
    } else if name.contains("ffmpeg") || name.contains("ocrmypdf") || name.contains("tesseract") {
        Duration::from_secs(60 * 60)
    } else if name.contains("7z") {
        Duration::from_secs(15 * 60)
    } else {
        Duration::from_secs(10 * 60)
    }
}

fn is_browser_executable_name(name: &str) -> bool {
    name.contains("chrome")
        || name.contains("chromium")
        || name.contains("msedge")
        || name.contains("microsoft edge")
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
    let mut child = ManagedChild::spawn(&mut command)?;
    let output = wait_for_managed_output(
        &mut child,
        &program,
        cancellation,
        process_timeout(engine),
    )
    .await?;
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
    let mut child = ManagedChild::spawn(&mut command)?;
    let output = wait_for_managed_output(
        &mut child,
        &program,
        cancellation,
        process_timeout(engine),
    )
    .await?;
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
    run_process_with_timeout(engine, args, cancellation, process_timeout(engine)).await
}

async fn run_process_in_directory(
    engine: &Path,
    args: &[OsString],
    working_directory: &Path,
    cancellation: &CancellationToken,
) -> Result<(), ExecutionError> {
    run_process_with_timeout_in_directory(
        engine,
        args,
        Some(working_directory),
        cancellation,
        process_timeout(engine),
    )
    .await
}

async fn run_process_with_timeout(
    engine: &Path,
    args: &[OsString],
    cancellation: &CancellationToken,
    timeout: Duration,
) -> Result<(), ExecutionError> {
    run_process_with_timeout_in_directory(engine, args, None, cancellation, timeout).await
}

async fn run_process_with_timeout_in_directory(
    engine: &Path,
    args: &[OsString],
    working_directory: Option<&Path>,
    cancellation: &CancellationToken,
    timeout: Duration,
) -> Result<(), ExecutionError> {
    if cancellation.is_cancelled() {
        return Err(ExecutionError::Cancelled);
    }
    let mut command = Command::new(engine);
    configure_external_command(&mut command);
    if let Some(working_directory) = working_directory {
        command.current_dir(working_directory);
    }
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
    let mut child = ManagedChild::spawn(&mut command)?;
    let output = wait_for_managed_output(&mut child, &program, cancellation, timeout).await?;
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
        "office" | "browser" => ResourceProfile::OFFICE,
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
            "smart", "zip", "7z", "tar", "tar.gz", "tar.xz", "tar.bz2", "tar.zst", "tar.lz4",
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
    "html-to-pdf",
    "email-to-pdf",
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
        assert_eq!(
            normalize_target_format("archive-package", Some("SMART"))
                .unwrap()
                .as_deref(),
            Some("smart")
        );
    }

    #[test]
    fn portable_tar_names_split_without_losing_components() {
        let prefix = "d".repeat(120);
        let value = format!("{prefix}/rapport.pdf");
        let (name, stored_prefix) = split_ustar_name(&value).unwrap();
        assert_eq!(name, "rapport.pdf");
        assert_eq!(stored_prefix, prefix);
        assert!(split_ustar_name(&"x".repeat(101)).is_err());
    }

    #[test]
    fn archive_layout_deduplicates_sources_and_preserves_duplicate_basenames() {
        let root = std::env::temp_dir().join(format!(
            "fileflow-archive-layout-{}",
            Uuid::new_v4().simple()
        ));
        let first = root.join("crates/first/Cargo.toml");
        let second = root.join("crates/second/Cargo.toml");
        std::fs::create_dir_all(first.parent().unwrap()).unwrap();
        std::fs::create_dir_all(second.parent().unwrap()).unwrap();
        std::fs::write(&first, b"[package]\nname = 'first'\n").unwrap();
        std::fs::write(&second, b"[package]\nname = 'second'\n").unwrap();
        let inputs = vec![
            ExecutionInput {
                path: root.clone(),
                source_root: Some(root.clone()),
            },
            ExecutionInput {
                path: first.clone(),
                source_root: Some(root.clone()),
            },
            ExecutionInput {
                path: first,
                source_root: Some(root.clone()),
            },
            ExecutionInput {
                path: second,
                source_root: Some(root.clone()),
            },
        ];
        let prepared = prepare_archive_inputs(&inputs).unwrap();
        assert_eq!(prepared.len(), 2);
        let layout = archive_command_layout(&prepared).unwrap();
        assert_eq!(layout.working_directory, root.parent().unwrap());
        let root_name = root.file_name().unwrap().to_string_lossy();
        assert_eq!(
            layout.entries,
            vec![
                format!("{root_name}/crates/first/Cargo.toml"),
                format!("{root_name}/crates/second/Cargo.toml"),
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ustar_header_contains_a_valid_checksum() {
        let metadata =
            std::fs::metadata(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs")).unwrap();
        let header = ustar_file_header("source/lib.rs", metadata.len(), &metadata).unwrap();
        let stored = std::str::from_utf8(&header[148..154])
            .ok()
            .and_then(|value| u64::from_str_radix(value, 8).ok())
            .unwrap();
        let mut checksum_header = header;
        checksum_header[148..156].fill(b' ');
        let calculated = checksum_header
            .iter()
            .map(|value| u64::from(*value))
            .sum::<u64>();
        assert_eq!(stored, calculated);
        assert_eq!(&header[257..263], b"ustar\0");
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
    fn qpdf_merge_accepts_recoverable_input_warnings() {
        let args = qpdf_merge_args(
            [Path::new("one.pdf"), Path::new("two.pdf")],
            Path::new("merged.pdf"),
        );
        let args = args
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                "--warning-exit-0",
                "--empty",
                "--pages",
                "one.pdf",
                "two.pdf",
                "--",
                "merged.pdf",
            ]
        );
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
    fn archive_pdf_preview_renders_only_a_bounded_first_page() {
        let args = pdf_first_page_preview_args(
            Path::new("inside/archive.pdf"),
            Path::new("preview/first-page"),
        )
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                "-f",
                "1",
                "-l",
                "1",
                "-singlefile",
                "-scale-to",
                "1800",
                "-png",
                "inside/archive.pdf",
                "preview/first-page",
            ]
        );
        assert_eq!(PREVIEW_RENDER_TIMEOUT, Duration::from_secs(20));
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

    #[test]
    fn email_decoding_handles_encoded_headers_and_quoted_printable_bodies() {
        assert_eq!(
            decode_rfc2047("=?UTF-8?B?U3VqZXQgZMOpY29kw6k=?="),
            "Sujet décodé"
        );
        assert_eq!(
            String::from_utf8(decode_quoted_printable(b"Bonjour=20=C3=A0=20tous")).unwrap(),
            "Bonjour à tous"
        );
    }

    #[test]
    fn email_html_preview_neutralizes_scripts_and_markup() {
        let headers = HashMap::from([
            ("content-type".into(), "text/html; charset=utf-8".into()),
            (
                "content-transfer-encoding".into(),
                "quoted-printable".into(),
            ),
        ]);
        let text = extract_mail_text(
            &headers,
            "<h1>Message</h1><script>window.location='https://example.com'</script><p>Corps=20sûr</p>",
            0,
        );
        assert!(text.contains("Message"));
        assert!(text.contains("Corps sûr"));
        assert!(!text.contains("window.location"));
        assert!(!text.contains("<script>"));
    }

    #[test]
    fn email_multipart_boundaries_keep_their_original_case() {
        let headers = HashMap::from([(
            "content-type".into(),
            "multipart/alternative; boundary=FileFlowBoundary".into(),
        )]);
        let body = "--FileFlowBoundary\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nVersion texte\r\n--FileFlowBoundary--\r\n";
        assert_eq!(extract_mail_text(&headers, body, 0).trim(), "Version texte");
    }

    #[test]
    fn browser_timeout_is_strictly_thirty_seconds_on_every_platform() {
        for executable in [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/usr/bin/chromium",
            r"C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
        ] {
            assert_eq!(
                process_timeout(Path::new(executable)),
                Duration::from_secs(30)
            );
        }
    }

    #[test]
    fn browser_render_budget_keeps_dynamic_and_static_attempts_below_global_limit() {
        assert!(
            BROWSER_SCRIPT_TIMEOUT + BROWSER_STATIC_TIMEOUT + BROWSER_TEXT_TIMEOUT
                < BROWSER_PRINT_TIMEOUT
        );
        assert_eq!(BROWSER_SCRIPT_TIMEOUT, Duration::from_secs(14));
        assert_eq!(BROWSER_STATIC_TIMEOUT, Duration::from_secs(7));
        assert_eq!(BROWSER_TEXT_TIMEOUT, Duration::from_secs(6));
    }

    #[test]
    fn browser_sanitized_snapshot_blocks_scripts_and_keeps_saved_content() {
        let source = "<html><head><title>Gmail</title><script>document.body.textContent='Erreur temporaire'</script></head><body><main>KingSpec 512 Go</main><img src='Commande_files/photo.png'></body></html>";
        let document = browser_sanitized_document(source, Path::new("/tmp/Mes messages"), true);
        assert!(document.contains("KingSpec 512 Go"));
        assert!(document.contains("Content-Security-Policy"));
        assert!(document.contains("script-src 'none'"));
        assert!(document.contains("img-src file: data: blob:"));
        assert!(document.contains("file:///tmp/Mes%20messages/"));
        assert!(!document.contains("document.body.textContent"));
        assert!(!document.to_ascii_lowercase().contains("<script"));
    }

    #[tokio::test]
    async fn browser_saved_gmail_uses_dom_preservation_mode() {
        let root = std::env::temp_dir()
            .join("fileflow-gmail-detection")
            .join(Uuid::new_v4().simple().to_string());
        tokio::fs::create_dir_all(&root)
            .await
            .expect("temporary root");
        let gmail = root.join("Commandes - Gmail.htm");
        tokio::fs::write(
            &gmail,
            b"<html><head><title>Commande - Gmail</title></head><body><main>Le message sauvegarde</main><script>document.body.textContent='Erreur temporaire'</script></body></html>",
        )
        .await
        .expect("saved Gmail page");
        assert!(
            browser_should_preserve_saved_dom(&gmail)
                .await
                .expect("Gmail detection")
        );

        let ordinary = root.join("application.html");
        tokio::fs::write(
            &ordinary,
            b"<html><body><div id=app></div><script>renderApplication()</script></body></html>",
        )
        .await
        .expect("ordinary dynamic page");
        assert!(
            !browser_should_preserve_saved_dom(&ordinary)
                .await
                .expect("ordinary page detection")
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[test]
    fn browser_print_uses_a_visible_pdf_staging_path() {
        let (_root, profile, output, log) = browser_attempt_paths();
        assert_eq!(
            output.file_name().and_then(|value| value.to_str()),
            Some("render.pdf")
        );
        assert!(
            !output
                .file_name()
                .and_then(|value| value.to_str())
                .expect("UTF-8 staging filename")
                .starts_with('.')
        );
        assert_eq!(
            profile.file_name().and_then(|value| value.to_str()),
            Some("profile")
        );
        assert_eq!(
            log.file_name().and_then(|value| value.to_str()),
            Some("browser.log")
        );
    }

    #[tokio::test]
    async fn browser_pdf_detection_rejects_partial_files() {
        let (root, _profile, output, _log) = browser_attempt_paths();
        tokio::fs::create_dir_all(&root)
            .await
            .expect("temporary root");
        tokio::fs::write(&output, b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n")
            .await
            .expect("partial PDF");
        assert_eq!(
            browser_complete_pdf_size(&output)
                .await
                .expect("partial PDF check"),
            None
        );

        tokio::fs::write(&output, b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n%%EOF\n")
            .await
            .expect("complete PDF");
        assert!(
            browser_complete_pdf_size(&output)
                .await
                .expect("complete PDF check")
                .is_some()
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[test]
    fn browser_text_snapshot_removes_active_markup() {
        let source = "<h1>Commande</h1><script>while(true){}</script><p>KingSpec 512 Go</p>";
        let plain = html_to_plain_text(source);
        let document = browser_text_snapshot_document("Gmail", &plain, false);
        assert!(document.contains("Commande"));
        assert!(document.contains("KingSpec 512 Go"));
        assert!(!document.contains("while(true)"));
        assert!(!document.contains("<script>"));
    }

    #[tokio::test]
    async fn passive_html_preview_is_native_and_never_needs_a_browser() {
        let root = std::env::temp_dir()
            .join("fileflow-native-preview")
            .join(Uuid::new_v4().simple().to_string());
        tokio::fs::create_dir_all(&root)
            .await
            .expect("temporary preview root");
        let input = root.join("message.html");
        tokio::fs::write(
            &input,
            b"<h1>Contenu FileFlow</h1><script>window.open('https://example.com')</script>",
        )
        .await
        .expect("HTML fixture");

        let preview = native_text_preview(&input, "html")
            .await
            .expect("native HTML preview");
        assert!(preview.contains("Contenu FileFlow"));
        assert!(!preview.contains("window.open"));
        assert!(!preview.contains("<script>"));
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[test]
    #[ignore = "helper process used by browser_process_timeout_stops_a_hung_child"]
    fn timeout_child_process_sleeps() {
        std::thread::sleep(Duration::from_secs(5));
    }

    #[tokio::test]
    async fn browser_process_timeout_stops_a_hung_child() {
        let executable = std::env::current_exe().expect("test executable");
        let cancellation = CancellationToken::new();
        let started = Instant::now();
        let error = run_process_with_timeout(
            &executable,
            &[
                OsString::from("--ignored"),
                OsString::from("--exact"),
                OsString::from("tests::timeout_child_process_sleeps"),
            ],
            &cancellation,
            Duration::from_millis(50),
        )
        .await
        .expect_err("the helper process must time out");

        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(matches!(
            error,
            ExecutionError::ProcessFailed { message, .. }
                if message.contains("délai maximal") && message.contains("0 s")
        ));
    }
}
