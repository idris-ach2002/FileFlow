use crate::AppState;
use fileflow_domain::PerformanceMode;
use fileflow_engine::EngineProbe;
use fileflow_planner::{CapabilityCatalog, ConversionPlan};
use fileflow_scheduler::SchedulerSnapshot;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    app: &'static str,
    version: &'static str,
    cpu_threads: usize,
    os: &'static str,
    architecture: &'static str,
    scheduler: SchedulerSnapshot,
}

pub(crate) fn runtime_health(state: &AppState) -> HealthResponse {
    HealthResponse {
        app: "FileFlow",
        version: env!("CARGO_PKG_VERSION"),
        cpu_threads: std::thread::available_parallelism().map_or(1, usize::from),
        os: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        scheduler: state.runtime.read().scheduler.snapshot(),
    }
}

#[tauri::command]
pub async fn health_check(state: State<'_, AppState>) -> Result<HealthResponse, String> {
    Ok(runtime_health(&state))
}

/// CI-only packaged-runtime handshake. The command is a no-op for normal users.
/// A packaged smoke test sets FILEFLOW_SMOKE_HEALTH_FILE, launches the real app,
/// and waits for Angular to invoke this command after authentication bootstrap.
/// That proves the native runtime, WebView, frontend bundle and Tauri IPC are all
/// alive instead of merely checking that a process stayed resident.
#[tauri::command]
pub fn smoke_frontend_ready(state: State<'_, AppState>) -> Result<(), String> {
    if std::env::var("FILEFLOW_SMOKE_TEST").as_deref() != Ok("1") {
        return Ok(());
    }
    let Some(path) = std::env::var_os("FILEFLOW_SMOKE_HEALTH_FILE") else {
        return Ok(());
    };
    let path = std::path::PathBuf::from(path);
    if path.file_name().and_then(|name| name.to_str()) != Some("health.json") {
        return Err("Nom de fichier de health-check invalide.".to_owned());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "Chemin de health-check invalide.".to_owned())?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let temp_root = std::env::temp_dir()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !parent.starts_with(&temp_root) {
        return Err("Le health-check CI doit rester dans le répertoire temporaire.".to_owned());
    }
    let payload = serde_json::json!({
        "schemaVersion": 1,
        "backend": true,
        "frontend": true,
        "pid": std::process::id(),
        "health": runtime_health(&state),
    });
    let temporary = path.with_extension("tmp");
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(&payload).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn probe_engines(state: State<'_, AppState>) -> Result<Vec<EngineProbe>, String> {
    Ok(state.core.engines.probe_all().await)
}

#[tauri::command]
pub fn capability_catalog(state: State<'_, AppState>) -> CapabilityCatalog {
    state.core.capabilities.clone()
}

#[tauri::command]
pub fn executable_actions() -> Vec<String> {
    fileflow_executor::executable_action_ids()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[tauri::command]
pub fn plan_conversion(
    state: State<'_, AppState>,
    input: String,
    output: String,
) -> Result<ConversionPlan, String> {
    state
        .core
        .capabilities
        .conversion_plan(&input, &output)
        .ok_or_else(|| format!("Aucun chemin de conversion de {input} vers {output}."))
}

#[tauri::command]
pub fn scheduler_status(state: State<'_, AppState>) -> SchedulerSnapshot {
    state.runtime.read().scheduler.snapshot()
}

#[tauri::command]
pub fn set_performance_mode(
    state: State<'_, AppState>,
    mode: PerformanceMode,
) -> Result<SchedulerSnapshot, String> {
    if !state.jobs.is_empty() {
        return Err(
            "Attendez la fin des traitements en cours avant de changer le mode de performance."
                .into(),
        );
    }
    let runtime = crate::ExecutionRuntime::new(mode);
    let snapshot = runtime.scheduler.snapshot();
    *state.runtime.write() = runtime;
    Ok(snapshot)
}
