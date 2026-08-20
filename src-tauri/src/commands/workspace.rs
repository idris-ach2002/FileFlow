use crate::{AppState, commands::account::require_active_session};
use fileflow_core::WorkspaceIntakeEvent;
use fileflow_domain::{ActionRecommendation, WorkspaceId};
use fileflow_intake::ScanOptions;
use fileflow_workspace::{AssetPage, AssetQuery, WorkspaceInsights, WorkspaceSnapshot};
use std::path::PathBuf;
use tauri::{State, ipc::Channel};
use tokio::sync::mpsc;

#[tauri::command]
pub async fn create_workspace(
    state: State<'_, AppState>,
    paths: Vec<PathBuf>,
    options: Option<ScanOptions>,
    on_event: Channel<WorkspaceIntakeEvent>,
) -> Result<WorkspaceSnapshot, String> {
    require_active_session(&state)?;
    if paths.is_empty() {
        return Err("Aucun fichier ou dossier n'a été fourni.".into());
    }

    let (event_tx, mut event_rx) = mpsc::channel(16);
    let forwarder = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if on_event.send(event).is_err() {
                break;
            }
        }
    });

    let result = state
        .core
        .create_workspace(paths, options.unwrap_or_default(), event_tx)
        .await
        .map_err(|error| error.to_string());

    let _ = forwarder.await;
    result
}

#[tauri::command]
pub fn get_workspace(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> Result<WorkspaceSnapshot, String> {
    require_active_session(&state)?;
    state
        .core
        .workspace(workspace_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn list_workspace_assets(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    query: AssetQuery,
) -> Result<AssetPage, String> {
    require_active_session(&state)?;
    state
        .core
        .list_workspace_assets(workspace_id, query)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn workspace_insights(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> Result<WorkspaceInsights, String> {
    require_active_session(&state)?;
    state
        .core
        .workspace_insights(workspace_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn workspace_recommendations(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> Result<Vec<ActionRecommendation>, String> {
    require_active_session(&state)?;
    state
        .core
        .workspace_recommendations(workspace_id)
        .await
        .map_err(|error| error.to_string())
}
