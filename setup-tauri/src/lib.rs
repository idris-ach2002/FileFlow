mod adapter;
mod cli;

use adapter::{SystemSetupAdapter, latest_setup_version};
use fileflow_setup_core::{
    SetupEvent, SetupPlan, SetupRequest, TransactionEngine, build_plan, probe_system,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::{Emitter, Manager, State};
use tokio::sync::Mutex;
use uuid::Uuid;

pub use cli::run_cli;

#[derive(Clone, Default)]
struct SetupRuntime {
    cancellation: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    active_operation: Arc<Mutex<Option<Uuid>>>,
    launch_mode: String,
}

impl SetupRuntime {
    fn from_args() -> Self {
        let arguments = std::env::args().collect::<Vec<_>>();
        let launch_mode = arguments
            .windows(2)
            .find(|pair| pair[0] == "--mode")
            .map(|pair| pair[1].clone())
            .filter(|mode| matches!(mode.as_str(), "install" | "repair" | "uninstall" | "doctor"))
            .unwrap_or_else(|| "auto".into());
        Self {
            cancellation: Arc::default(),
            active_operation: Arc::default(),
            launch_mode,
        }
    }
}

#[tauri::command]
fn setup_context(runtime: State<'_, SetupRuntime>) -> String {
    runtime.launch_mode.clone()
}

#[tauri::command]
async fn setup_update_status() -> Result<serde_json::Value, String> {
    let current = env!("CARGO_PKG_VERSION");
    let latest = latest_setup_version().await?;
    let tuple = |value: &str| {
        let mut parts = value
            .split(['-', '+'])
            .next()
            .unwrap_or_default()
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0));
        (
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
        )
    };
    let update_available = tuple(&latest) > tuple(current);
    Ok(serde_json::json!({
        "current": current,
        "latest": latest,
        "updateAvailable": update_available,
    }))
}

#[tauri::command]
async fn setup_open_download_portal() -> Result<(), String> {
    let url = "https://fileflow.idris-achabou.fit/#download";
    let mut command = if cfg!(target_os = "windows") {
        let mut command = tokio::process::Command::new("cmd.exe");
        command.args(["/C", "start", "", url]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = tokio::process::Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = tokio::process::Command::new("xdg-open");
        command.arg(url);
        command
    };
    command.spawn().map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn setup_probe() -> Result<fileflow_setup_core::SystemSnapshot, String> {
    probe_system().map_err(|error| error.to_string())
}

#[tauri::command]
async fn setup_plan(request: SetupRequest) -> Result<SetupPlan, String> {
    let snapshot = probe_system().map_err(|error| error.to_string())?;
    Ok(build_plan(&snapshot, request))
}

#[tauri::command]
async fn setup_start(
    app: tauri::AppHandle,
    runtime: State<'_, SetupRuntime>,
    plan: SetupPlan,
) -> Result<Uuid, String> {
    let mut active = runtime.active_operation.lock().await;
    if let Some(operation_id) = *active {
        return Err(format!("une opération est déjà active: {operation_id}"));
    }

    let cancellation = Arc::new(AtomicBool::new(false));
    *runtime.cancellation.lock().await = Some(cancellation.clone());
    *active = Some(plan.operation_id);
    drop(active);

    let runtime_handle = app.state::<SetupRuntime>().inner().clone();
    let operation_id = plan.operation_id;
    tauri::async_runtime::spawn(async move {
        let result = execute_plan(&app, &plan, cancellation).await;
        if let Err(message) = result
            && message != "opération annulée"
        {
            let _ = app.emit(
                "fileflow://setup-event",
                SetupEvent {
                    operation_id,
                    sequence: u64::MAX,
                    timestamp: chrono::Utc::now(),
                    event_type: "operation-error".into(),
                    level: fileflow_setup_core::EventLevel::Error,
                    step_id: None,
                    message,
                    completed: None,
                    total: None,
                    unit: None,
                    detail: serde_json::Value::Null,
                },
            );
        }
        *runtime_handle.active_operation.lock().await = None;
        *runtime_handle.cancellation.lock().await = None;
    });
    Ok(operation_id)
}

#[tauri::command]
async fn setup_cancel(runtime: State<'_, SetupRuntime>) -> Result<(), String> {
    let cancellation = runtime.cancellation.lock().await;
    let Some(cancellation) = cancellation.as_ref() else {
        return Err("aucune opération active".into());
    };
    cancellation.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
async fn setup_open_fileflow() -> Result<(), String> {
    let snapshot = probe_system().map_err(|error| error.to_string())?;
    let path = snapshot
        .application
        .path
        .ok_or_else(|| "FileFlow n’est pas installé".to_string())?;
    let mut command = match snapshot.platform {
        fileflow_setup_core::Platform::Macos => {
            let mut value = tokio::process::Command::new("open");
            value.arg(path);
            value
        }
        fileflow_setup_core::Platform::Linux => tokio::process::Command::new(path),
        fileflow_setup_core::Platform::Windows => tokio::process::Command::new(path),
    };
    command.spawn().map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn setup_smoke_ready() -> Result<(), String> {
    if std::env::var("FILEFLOW_SETUP_SMOKE_TEST").as_deref() != Ok("1") {
        return Ok(());
    }
    let path = std::env::var_os("FILEFLOW_SETUP_SMOKE_HEALTH_FILE")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "chemin du health-check Setup absent".to_string())?;
    if path.file_name().and_then(|name| name.to_str()) != Some("setup-health.json") {
        return Err("nom du health-check Setup invalide".into());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "chemin du health-check Setup invalide".to_string())?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let temp_root = std::env::temp_dir()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !parent.starts_with(&temp_root) {
        return Err("le health-check Setup doit rester dans le dossier temporaire".into());
    }
    let snapshot = probe_system().map_err(|error| error.to_string())?;
    let payload = serde_json::json!({
        "schemaVersion": 1,
        "app": "FileFlow Setup",
        "version": env!("CARGO_PKG_VERSION"),
        "backend": true,
        "frontend": true,
        "platform": snapshot.platform,
        "architecture": snapshot.architecture,
    });
    let temporary = path.with_extension("json.writing");
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(&payload).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    std::fs::rename(temporary, path).map_err(|error| error.to_string())
}

async fn execute_plan(
    app: &tauri::AppHandle,
    plan: &SetupPlan,
    cancellation: Arc<AtomicBool>,
) -> Result<(), String> {
    let snapshot = probe_system().map_err(|error| error.to_string())?;
    let operation_dir = snapshot
        .receipt_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("operations")
        .join(plan.operation_id.to_string());
    let journal_path = operation_dir.join("journal.json");
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|error| error.to_string())?;
    let adapter = SystemSetupAdapter::new(resource_dir, operation_dir, snapshot);
    let app_events = app.clone();
    let sink = move |event: SetupEvent| {
        let _ = app_events.emit("fileflow://setup-event", event);
    };
    TransactionEngine::new(journal_path)
        .execute(plan, &adapter, &sink, cancellation)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .manage(SetupRuntime::from_args())
        .setup(|app| {
            if std::env::var("FILEFLOW_SETUP_SMOKE_TEST").as_deref() == Ok("1")
                && let Some(window) = app.get_webview_window("setup")
            {
                window.hide()?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            setup_probe,
            setup_update_status,
            setup_open_download_portal,
            setup_context,
            setup_plan,
            setup_start,
            setup_cancel,
            setup_open_fileflow,
            setup_smoke_ready
        ])
        .run(tauri::generate_context!())
        .expect("FileFlow Setup failed to start");
}
