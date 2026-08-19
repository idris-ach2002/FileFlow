use crate::AppState;
use fileflow_analysis::{DuplicateInput, DuplicateReport};
use fileflow_domain::{ResourceProfile, WorkspaceId};
use tauri::State;
use tokio_util::sync::CancellationToken;

#[tauri::command]
pub async fn confirm_duplicates(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> Result<DuplicateReport, String> {
    let assets = state
        .core
        .workspaces
        .select_assets(workspace_id, &[], &[])
        .map_err(|error| error.to_string())?;
    let inputs = assets
        .into_iter()
        .map(|asset| DuplicateInput {
            asset_id: asset.id(),
            path: asset.common().path.clone(),
            size_bytes: asset.size_bytes(),
        })
        .collect::<Vec<_>>();
    let profile = ResourceProfile {
        cpu_weight: 4,
        memory_mb: 256,
        io_weight: 3,
        internally_threaded: true,
        max_parallel_instances: 1,
    };
    let cancellation = CancellationToken::new();
    let _lease = state
        .scheduler
        .acquire("native-duplicates", profile, &cancellation)
        .await
        .map_err(|error| error.to_string())?;
    let threads = state.scheduler.budget().cpu_tokens.clamp(1, 4);
    tokio::task::spawn_blocking(move || fileflow_analysis::confirm_duplicates(inputs, threads))
        .await
        .map_err(|error| format!("Le worker de détection de doublons a échoué : {error}"))
}
