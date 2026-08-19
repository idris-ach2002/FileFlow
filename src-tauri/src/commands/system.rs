use crate::AppState;
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

#[tauri::command]
pub async fn health_check(state: State<'_, AppState>) -> Result<HealthResponse, String> {
    Ok(HealthResponse {
        app: "FileFlow",
        version: env!("CARGO_PKG_VERSION"),
        cpu_threads: std::thread::available_parallelism().map_or(1, usize::from),
        os: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        scheduler: state.scheduler.snapshot(),
    })
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
    state.scheduler.snapshot()
}
