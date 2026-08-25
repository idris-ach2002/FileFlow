use crate::AppState;
use fileflow_domain::PerformanceMode;
use fileflow_engine::EngineProbe;
use fileflow_planner::{CapabilityCatalog, ConversionPlan};
use fileflow_scheduler::SchedulerSnapshot;
use serde::Serialize;
use tauri::State;

#[tauri::command]
pub fn launch_fileflow_setup(mode: String) -> Result<String, String> {
    if !matches!(mode.as_str(), "install" | "repair" | "uninstall" | "doctor") {
        return Err("Mode de maintenance invalide.".into());
    }
    let home = std::env::var_os(if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    })
    .map(std::path::PathBuf::from)
    .ok_or_else(|| "Dossier utilisateur introuvable.".to_string())?;
    let maintenance = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/FileFlow/maintenance/FileFlowSetup.app")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("FileFlow/maintenance/FileFlowSetup.exe")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("fileflow/maintenance/FileFlowSetup.AppImage")
    };
    if !maintenance.exists() {
        return Err(
            "Le centre de maintenance FileFlow Setup n’est pas installé. Téléchargez-le depuis le portail FileFlow."
                .into(),
        );
    }
    let mut command = if cfg!(target_os = "macos") {
        let mut value = std::process::Command::new("open");
        value
            .arg("-n")
            .arg(&maintenance)
            .args(["--args", "--mode"])
            .arg(&mode);
        value
    } else {
        let mut value = std::process::Command::new(&maintenance);
        value.args(["--mode", &mode]);
        value
    };
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .spawn()
        .map_err(|error| format!("Impossible d’ouvrir FileFlow Setup : {error}"))?;
    Ok(maintenance.to_string_lossy().into_owned())
}

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

/// Packaged-runtime handshake used by release CI and FileFlow Setup post-checks.
/// The command is a no-op during a normal launch. A controlled smoke process sets
/// FILEFLOW_SMOKE_HEALTH_FILE and waits for Angular to invoke this command after
/// authentication bootstrap, proving that the native runtime, WebView, frontend
/// bundle and Tauri IPC are alive without revealing the application window.
#[tauri::command]
pub fn smoke_frontend_ready(state: State<'_, AppState>) -> Result<bool, String> {
    if std::env::var("FILEFLOW_SMOKE_TEST").as_deref() != Ok("1") {
        return Ok(false);
    }
    let Some(path) = std::env::var_os("FILEFLOW_SMOKE_HEALTH_FILE") else {
        return Ok(false);
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
    std::fs::rename(&temporary, &path).map_err(|error| error.to_string())?;
    Ok(true)
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
