mod commands;

use fileflow_core::FileFlowCore;
use fileflow_scheduler::ResourceScheduler;
use std::sync::Arc;

pub(crate) struct AppState {
    pub(crate) core: Arc<FileFlowCore>,
    pub(crate) scheduler: Arc<ResourceScheduler>,
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

    let scheduler = Arc::new(ResourceScheduler::default());
    tracing::info!(budget = ?scheduler.budget(), "resource scheduler initialized");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState { core: build_core(), scheduler })
        .invoke_handler(tauri::generate_handler![
            commands::system::health_check,
            commands::system::probe_engines,
            commands::system::capability_catalog,
            commands::system::plan_conversion,
            commands::system::scheduler_status,
            commands::workspace::create_workspace,
            commands::workspace::get_workspace,
            commands::workspace::list_workspace_assets,
            commands::workspace::workspace_insights,
            commands::workspace::workspace_recommendations,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FileFlow");
}
