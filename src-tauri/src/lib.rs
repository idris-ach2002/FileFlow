use fileflow_core::FileFlowCore;
use fileflow_engine::EngineProbe;
use fileflow_scheduler::ResourceBudget;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

struct AppState {
    core: Arc<FileFlowCore>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    app: &'static str,
    version: &'static str,
    cpu_threads: usize,
    os: &'static str,
    architecture: &'static str,
}

#[tauri::command]
async fn health_check() -> HealthResponse {
    HealthResponse {
        app: "FileFlow",
        version: env!("CARGO_PKG_VERSION"),
        cpu_threads: std::thread::available_parallelism().map_or(1, usize::from),
        os: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
    }
}

#[tauri::command]
async fn probe_engines(
    state: State<'_, AppState>,
) -> Result<Vec<EngineProbe>, String> {
    Ok(state.core.engines.probe_all().await)
}

fn build_core() -> Arc<FileFlowCore> {
    let core = Arc::new(FileFlowCore::default());
    core.engines.register(Arc::new(fileflow_adapter_ffmpeg::Adapter));
    core.engines.register(Arc::new(fileflow_adapter_vips::Adapter));
    core.engines.register(Arc::new(fileflow_adapter_qpdf::Adapter));
    core.engines.register(Arc::new(fileflow_adapter_office::Adapter));
    core.engines.register(Arc::new(fileflow_adapter_ocr::Adapter));
    core.engines.register(Arc::new(fileflow_adapter_archive::Adapter));
    core.engines.register(Arc::new(fileflow_adapter_metadata::Adapter));
    core.engines.register(Arc::new(fileflow_adapter_imagemagick::Adapter));
    core.engines.register(Arc::new(fileflow_adapter_poppler::Adapter));
    core.engines.register(Arc::new(fileflow_adapter_ghostscript::Adapter));
    core.engines.register(Arc::new(fileflow_adapter_tesseract::Adapter));
    core.engines.register(Arc::new(fileflow_adapter_pandoc::Adapter));
    core
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "fileflow=info".into()),
        )
        .init();

    let budget = ResourceBudget::balanced();
    tracing::info!(?budget, "resource budget initialized");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState { core: build_core() })
        .invoke_handler(tauri::generate_handler![health_check, probe_engines])
        .run(tauri::generate_context!())
        .expect("error while running FileFlow");
}
