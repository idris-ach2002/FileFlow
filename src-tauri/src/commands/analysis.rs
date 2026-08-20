use crate::{AppState, commands::account::require_active_session};
use fileflow_analysis::{DuplicateInput, DuplicateReport};
use fileflow_domain::{Asset, AssetId, ResourceProfile, WorkspaceId};
use fileflow_executor::ArchiveInspection;
use tauri::State;
use tokio_util::sync::CancellationToken;

#[tauri::command]
pub async fn confirm_duplicates(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> Result<DuplicateReport, String> {
    require_active_session(&state)?;
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
    let scheduler = { state.runtime.read().scheduler.clone() };
    let _lease = scheduler
        .acquire("native-duplicates", profile, &cancellation)
        .await
        .map_err(|error| error.to_string())?;
    let threads = scheduler.budget().cpu_tokens.clamp(1, 4);
    tokio::task::spawn_blocking(move || fileflow_analysis::confirm_duplicates(inputs, threads))
        .await
        .map_err(|error| format!("Le worker de détection de doublons a échoué : {error}"))
}

#[tauri::command]
pub async fn inspect_archive(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    asset_id: Option<AssetId>,
) -> Result<ArchiveInspection, String> {
    require_active_session(&state)?;
    let selected = asset_id.into_iter().collect::<Vec<_>>();
    let assets = state
        .core
        .workspaces
        .select_assets(workspace_id, &selected, &[])
        .map_err(|error| error.to_string())?;
    let archive = assets
        .into_iter()
        .find_map(|asset| match asset {
            Asset::Archive(archive) => Some(archive.common.path),
            _ => None,
        })
        .ok_or_else(|| {
            "Aucune archive compatible n’est disponible dans ce workspace.".to_owned()
        })?;

    let probes = state.core.engines.probe_all().await;
    let engine = probes
        .into_iter()
        .find(|probe| probe.id == "archive" && probe.available)
        .and_then(|probe| probe.executable)
        .ok_or_else(|| "7-Zip n’est pas disponible pour inspecter cette archive.".to_owned())?;
    let cancellation = CancellationToken::new();
    let scheduler = { state.runtime.read().scheduler.clone() };
    let _lease = scheduler
        .acquire("archive", ResourceProfile::ARCHIVE, &cancellation)
        .await
        .map_err(|error| error.to_string())?;
    fileflow_executor::inspect_archive(&engine, &archive, &cancellation)
        .await
        .map_err(|error| error.to_string())
}
