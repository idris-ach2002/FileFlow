use crate::AppState;
use fileflow_engine::EngineProbe;
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
}

#[tauri::command]
pub async fn health_check() -> HealthResponse {
    HealthResponse {
        app: "FileFlow",
        version: env!("CARGO_PKG_VERSION"),
        cpu_threads: std::thread::available_parallelism().map_or(1, usize::from),
        os: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
    }
}

#[tauri::command]
pub async fn probe_engines(state: State<'_, AppState>) -> Result<Vec<EngineProbe>, String> {
    Ok(state.core.engines.probe_all().await)
}
