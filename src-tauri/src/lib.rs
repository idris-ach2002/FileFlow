mod commands;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use fileflow_core::FileFlowCore;
use fileflow_domain::{JobId, PerformanceMode};
use fileflow_executor::ActionExecutor;
use fileflow_scheduler::{ResourceScheduler, SchedulerSettings};
use fileflow_storage::Storage;
use parking_lot::RwLock;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
};
use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(crate) struct LoginAttempt {
    pub(crate) failures: u32,
    pub(crate) blocked_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionRecord {
    pub(crate) token: String,
    pub(crate) account_id: Uuid,
    pub(crate) expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub(crate) struct RecentOutputs {
    pub(crate) sequence: u64,
    pub(crate) paths: Vec<PathBuf>,
}

#[derive(Clone)]
pub(crate) struct ExecutionRuntime {
    pub(crate) scheduler: Arc<ResourceScheduler>,
    pub(crate) executor: Arc<ActionExecutor>,
}

impl ExecutionRuntime {
    pub(crate) fn new(mode: PerformanceMode) -> Self {
        let scheduler = Arc::new(ResourceScheduler::new(SchedulerSettings {
            mode,
            custom_budget: None,
        }));
        let executor = Arc::new(ActionExecutor::new(scheduler.clone()));
        Self {
            scheduler,
            executor,
        }
    }
}

pub(crate) struct AppState {
    pub(crate) core: Arc<FileFlowCore>,
    pub(crate) runtime: RwLock<ExecutionRuntime>,
    pub(crate) jobs: DashMap<JobId, CancellationToken>,
    pub(crate) storage: Arc<Storage>,
    pub(crate) session: RwLock<Option<SessionRecord>>,
    pub(crate) login_attempts: DashMap<String, LoginAttempt>,
    pub(crate) data_dir: PathBuf,
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
    let convert = MenuItem::with_id(
        app,
        "tray-convert",
        "Nouvelle conversion",
        true,
        None::<&str>,
    )?;
    let history = MenuItem::with_id(app, "tray-history", "Historique", true, None::<&str>)?;
    let automations = MenuItem::with_id(
        app,
        "tray-automations",
        "Automatisations",
        true,
        None::<&str>,
    )?;
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
    core.engines
        .register(Arc::new(fileflow_adapter_ffmpeg::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_vips::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_qpdf::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_office::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_ocr::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_archive::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_metadata::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_imagemagick::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_img2pdf::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_poppler::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_ghostscript::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_tesseract::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_pandoc::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_zstd::Adapter));
    core.engines
        .register(Arc::new(fileflow_adapter_lz4::Adapter));
    core
}

fn stored_performance_mode(storage: &Storage) -> PerformanceMode {
    storage
        .get_json::<serde_json::Value>("app.preferences.v2")
        .ok()
        .flatten()
        .and_then(|value| {
            value
                .get("performanceMode")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .map(|mode| match mode.as_str() {
            "eco" => PerformanceMode::Eco,
            "fast" => PerformanceMode::Fast,
            _ => PerformanceMode::Balanced,
        })
        .unwrap_or(PerformanceMode::Balanced)
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
            let tray = build_tray(app)?;
            let data_dir = app.path().app_data_dir()?;
            let storage = Arc::new(Storage::open(&data_dir.join("fileflow.sqlite3"))?);
            let performance_mode = stored_performance_mode(&storage);
            let runtime = ExecutionRuntime::new(performance_mode);
            tracing::info!(budget = ?runtime.scheduler.budget(), database = %data_dir.display(), "FileFlow runtime initialized");
            app.manage(AppState {
                core: build_core(),
                runtime: RwLock::new(runtime),
                jobs: DashMap::new(),
                storage,
                session: RwLock::new(None),
                login_attempts: DashMap::new(),
                data_dir: data_dir.clone(),
                recent_outputs: DashMap::new(),
                output_sequence: AtomicU64::new(0),
                _tray: tray,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::account::account_bootstrap,
            commands::account::create_account,
            commands::account::login,
            commands::account::change_password,
            commands::account::logout,
            commands::account::current_session,
            commands::account::save_onboarding,
            commands::account::update_profile,
            commands::account::choose_profile_avatar,
            commands::account::profile_avatar,
            commands::account::default_storage_directory,
            commands::system::health_check,
            commands::system::probe_engines,
            commands::system::capability_catalog,
            commands::system::executable_actions,
            commands::system::plan_conversion,
            commands::system::scheduler_status,
            commands::system::set_performance_mode,
            commands::workspace::create_workspace,
            commands::workspace::get_workspace,
            commands::workspace::list_workspace_assets,
            commands::workspace::workspace_insights,
            commands::workspace::workspace_recommendations,
            commands::analysis::confirm_duplicates,
            commands::analysis::inspect_archive,
            commands::execution::execute_action,
            commands::execution::cancel_job,
            commands::execution::open_job_output,
            commands::execution::reveal_job_output,
            commands::execution::save_job_output_copy,
            commands::storage::load_app_preferences,
            commands::storage::save_app_preferences,
            commands::storage::history,
            commands::storage::favorites,
            commands::storage::set_favorite,
            commands::storage::recipes,
            commands::storage::save_recipe,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FileFlow");
}
