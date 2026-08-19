mod commands;

use dashmap::DashMap;
use fileflow_core::FileFlowCore;
use fileflow_domain::JobId;
use fileflow_executor::ActionExecutor;
use fileflow_scheduler::ResourceScheduler;
use fileflow_storage::Storage;
use std::{
    path::PathBuf,
    sync::{
        atomic::AtomicU64,
        Arc,
    },
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub(crate) struct RecentOutputs {
    pub(crate) sequence: u64,
    pub(crate) paths: Vec<PathBuf>,
}

pub(crate) struct AppState {
    pub(crate) core: Arc<FileFlowCore>,
    pub(crate) scheduler: Arc<ResourceScheduler>,
    pub(crate) executor: Arc<ActionExecutor>,
    pub(crate) jobs: DashMap<JobId, CancellationToken>,
    pub(crate) storage: Arc<Storage>,
    pub(crate) recent_outputs: DashMap<JobId, RecentOutputs>,
    pub(crate) output_sequence: AtomicU64,
    pub(crate) _tray: TrayIcon,
}


fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &tauri::App) -> tauri::Result<TrayIcon> {
    let open = MenuItem::with_id(app, "tray-open", "Ouvrir FileFlow", true, None::<&str>)?;
    let convert = MenuItem::with_id(app, "tray-convert", "Nouvelle conversion", true, None::<&str>)?;
    let history = MenuItem::with_id(app, "tray-history", "Historique", true, None::<&str>)?;
    let automations = MenuItem::with_id(app, "tray-automations", "Automatisations", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "tray-settings", "Paramètres", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray-quit", "Quitter FileFlow", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open, &convert, &history, &automations, &settings, &quit],
    )?;

    let mut builder = TrayIconBuilder::with_id("fileflow-main")
        .tooltip("FileFlow — traitement local")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray-open" => show_main_window(app),
            "tray-convert" => {
                show_main_window(app);
                let _ = app.emit("fileflow://navigate", "/workspace");
            }
            "tray-history" => {
                show_main_window(app);
                let _ = app.emit("fileflow://navigate", "/history");
            }
            "tray-automations" => {
                show_main_window(app);
                let _ = app.emit("fileflow://navigate", "/automations");
            }
            "tray-settings" => {
                show_main_window(app);
                let _ = app.emit("fileflow://navigate", "/settings");
            }
            "tray-quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)
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
    core.engines.register(Arc::new(fileflow_adapter_img2pdf::Adapter));
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

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let scheduler = Arc::new(ResourceScheduler::default());
            let tray = build_tray(app)?;
            let data_dir = app.path().app_data_dir()?;
            let storage = Arc::new(Storage::open(&data_dir.join("fileflow.sqlite3"))?);
            tracing::info!(budget = ?scheduler.budget(), database = %data_dir.display(), "FileFlow runtime initialized");
            app.manage(AppState {
                core: build_core(),
                executor: Arc::new(ActionExecutor::new(scheduler.clone())),
                scheduler,
                jobs: DashMap::new(),
                storage,
                recent_outputs: DashMap::new(),
                output_sequence: AtomicU64::new(0),
                _tray: tray,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::health_check,
            commands::system::probe_engines,
            commands::system::capability_catalog,
            commands::system::executable_actions,
            commands::system::plan_conversion,
            commands::system::scheduler_status,
            commands::workspace::create_workspace,
            commands::workspace::get_workspace,
            commands::workspace::list_workspace_assets,
            commands::workspace::workspace_insights,
            commands::workspace::workspace_recommendations,
            commands::analysis::confirm_duplicates,
            commands::execution::execute_action,
            commands::execution::cancel_job,
            commands::execution::open_job_output,
            commands::execution::reveal_job_output,
            commands::execution::save_job_output_copy,
            commands::storage::history,
            commands::storage::favorites,
            commands::storage::set_favorite,
            commands::storage::recipes,
            commands::storage::save_recipe,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FileFlow");
}
