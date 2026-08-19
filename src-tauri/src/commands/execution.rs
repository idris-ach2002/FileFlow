use crate::{AppState, RecentOutputs};
use chrono::Utc;
use fileflow_domain::{AssetId, JobId, OutputPolicy, WorkspaceId};
use fileflow_executor::{EnginePaths, ExecutionEvent, ExecutionInput, ExecutionRequest, ExecutionSummary};
use fileflow_storage::HistoryEntry;
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::Ordering,
        Arc,
    },
};
use tauri::{ipc::Channel, AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteWorkspaceAction {
    pub workspace_id: WorkspaceId,
    pub action_id: String,
    #[serde(default)]
    pub selected_asset_ids: Vec<AssetId>,
    #[serde(default)]
    pub output_policy: OutputPolicy,
    pub target_format: Option<String>,
    pub quality: Option<String>,
}

#[tauri::command]
pub async fn execute_action(
    state: State<'_, AppState>,
    request: ExecuteWorkspaceAction,
    on_event: Channel<ExecutionEvent>,
) -> Result<ExecutionSummary, String> {
    if !fileflow_executor::is_supported(&request.action_id) {
        return Err(format!("L’action « {} » n’est pas encore reliée à un exécuteur local.", request.action_id));
    }

    let action = state
        .core
        .capabilities
        .action(&request.action_id)
        .cloned()
        .ok_or_else(|| format!("Action inconnue : {}", request.action_id))?;
    let workspace = state
        .core
        .workspace(request.workspace_id)
        .map_err(|error| error.to_string())?;
    let assets = state
        .core
        .workspaces
        .select_assets(request.workspace_id, &request.selected_asset_ids, &action.accepts)
        .map_err(|error| error.to_string())?;
    if assets.is_empty() {
        return Err("Aucun élément compatible n’est sélectionné pour cette opération.".into());
    }

    let probes = state.core.engines.probe_all().await;
    let engine_paths = probes
        .into_iter()
        .filter_map(|probe| probe.available.then_some((probe.id, probe.executable)).and_then(|(id, path)| path.map(|path| (id, path))))
        .collect::<HashMap<String, PathBuf>>();
    let missing = action
        .required_engines
        .iter()
        .filter(|engine| !engine_paths.contains_key(engine.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!("Moteur{} manquant{} : {}", if missing.len() > 1 { "s" } else { "" }, if missing.len() > 1 { "s" } else { "" }, missing.join(", ")));
    }

    let inputs = assets
        .iter()
        .map(|asset| ExecutionInput {
            path: asset.common().path.clone(),
            source_root: workspace.roots.get(asset.common().root_index).cloned(),
        })
        .collect::<Vec<_>>();
    let input_bytes = assets.iter().map(|asset| asset.size_bytes()).sum::<u64>();
    let job_id = JobId::new();
    let cancellation = CancellationToken::new();
    state.jobs.insert(job_id, cancellation.clone());

    let (event_tx, mut event_rx) = mpsc::channel(64);
    let forwarder = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if on_event.send(event).is_err() {
                break;
            }
        }
    });

    let execution = state
        .executor
        .execute(
            job_id,
            ExecutionRequest {
                action_id: request.action_id.clone(),
                inputs,
                output_policy: request.output_policy,
                target_format: request.target_format,
                quality: request.quality,
            },
            EnginePaths::new(engine_paths),
            cancellation,
            event_tx,
        )
        .await;
    state.jobs.remove(&job_id);
    let _ = forwarder.await;

    let summary = execution.map_err(|error| error.to_string())?;
    remember_outputs(&state, &summary);
    record_history(state.storage.clone(), &summary, input_bytes).await;
    Ok(summary)
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>, job_id: JobId) -> bool {
    state.jobs.get(&job_id).is_some_and(|entry| {
        entry.value().cancel();
        true
    })
}


const RECENT_OUTPUT_JOB_LIMIT: usize = 64;

fn remember_outputs(state: &AppState, summary: &ExecutionSummary) {
    if summary.outputs.is_empty() {
        return;
    }
    let sequence = state.output_sequence.fetch_add(1, Ordering::Relaxed) + 1;
    state.recent_outputs.insert(
        summary.job_id,
        RecentOutputs {
            sequence,
            paths: summary.outputs.clone(),
        },
    );
    if state.recent_outputs.len() <= RECENT_OUTPUT_JOB_LIMIT {
        return;
    }
    let oldest = state
        .recent_outputs
        .iter()
        .min_by_key(|entry| entry.value().sequence)
        .map(|entry| *entry.key());
    if let Some(job_id) = oldest {
        state.recent_outputs.remove(&job_id);
    }
}

fn registered_output(state: &AppState, job_id: JobId, index: usize) -> Result<PathBuf, String> {
    let outputs = state
        .recent_outputs
        .get(&job_id)
        .ok_or_else(|| "Les sorties de ce traitement ne sont plus disponibles dans la session courante.".to_owned())?;
    let path = outputs
        .paths
        .get(index)
        .cloned()
        .ok_or_else(|| "Résultat introuvable pour ce traitement.".to_owned())?;
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| "Le résultat n’existe plus à son emplacement d’origine.".to_owned())?;
    if metadata.file_type().is_symlink() {
        return Err("FileFlow refuse d’ouvrir une sortie devenue un lien symbolique.".into());
    }
    Ok(path)
}

#[tauri::command]
pub fn open_job_output(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: JobId,
    index: usize,
) -> Result<(), String> {
    let path = registered_output(&state, job_id, index)?;
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn reveal_job_output(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: JobId,
    index: usize,
) -> Result<(), String> {
    let path = registered_output(&state, job_id, index)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_job_output_copy(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: JobId,
    index: usize,
) -> Result<Option<PathBuf>, String> {
    let source = registered_output(&state, job_id, index)?;
    let metadata = std::fs::metadata(&source).map_err(|error| error.to_string())?;
    let destination = if metadata.is_dir() {
        let Some(folder) = app
            .dialog()
            .file()
            .set_title("Enregistrer une copie du dossier")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let folder = folder.into_path().map_err(|error| error.to_string())?;
        unique_copy_destination(&folder.join(source.file_name().unwrap_or_default()))
    } else {
        let mut dialog = app.dialog().file().set_title("Enregistrer une copie");
        if let Some(name) = source.file_name().and_then(|value| value.to_str()) {
            dialog = dialog.set_file_name(name);
        }
        let Some(file) = dialog.blocking_save_file() else {
            return Ok(None);
        };
        file.into_path().map_err(|error| error.to_string())?
    };

    let source_for_copy = source.clone();
    let destination_for_copy = destination.clone();
    tokio::task::spawn_blocking(move || copy_output(&source_for_copy, &destination_for_copy))
        .await
        .map_err(|error| format!("La copie a été interrompue : {error}"))??;
    Ok(Some(destination))
}

fn unique_copy_destination(base: &Path) -> PathBuf {
    if !base.exists() {
        return base.to_path_buf();
    }
    let parent = base.parent().unwrap_or_else(|| Path::new("."));
    let stem = base.file_stem().and_then(|value| value.to_str()).unwrap_or("copie");
    let extension = base.extension().and_then(|value| value.to_str());
    for index in 1..=10_000 {
        let name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem}-copie"))
}

fn copy_output(source: &Path, destination: &Path) -> Result<(), String> {
    let source_metadata = std::fs::symlink_metadata(source).map_err(|error| error.to_string())?;
    if source_metadata.file_type().is_symlink() {
        return Err("La source est devenue un lien symbolique ; copie annulée.".into());
    }
    if source_metadata.is_file() {
        if source == destination {
            return Err("La copie doit avoir un emplacement différent de l’original.".into());
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::copy(source, destination).map_err(|error| error.to_string())?;
        return Ok(());
    }
    if !source_metadata.is_dir() {
        return Err("Ce type de résultat ne peut pas être copié.".into());
    }

    let source_canonical = std::fs::canonicalize(source).map_err(|error| error.to_string())?;
    if let Some(parent) = destination.parent() {
        if let Ok(parent_canonical) = std::fs::canonicalize(parent) {
            if parent_canonical.starts_with(&source_canonical) {
                return Err("La copie d’un dossier ne peut pas être créée à l’intérieur de lui-même.".into());
            }
        }
    }
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    let mut stack = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((from, to)) = stack.pop() {
        for entry in std::fs::read_dir(&from).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let from_path = entry.path();
            let to_path = to.join(entry.file_name());
            let metadata = std::fs::symlink_metadata(&from_path).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() {
                return Err(format!("Lien symbolique refusé pendant la copie : {}", from_path.display()));
            }
            if metadata.is_dir() {
                std::fs::create_dir_all(&to_path).map_err(|error| error.to_string())?;
                stack.push((from_path, to_path));
            } else if metadata.is_file() {
                std::fs::copy(&from_path, &to_path).map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}

async fn record_history(storage: Arc<fileflow_storage::Storage>, summary: &ExecutionSummary, input_bytes: u64) {
    let output_bytes = output_size(&summary.outputs).await;
    let destination = summary
        .outputs
        .first()
        .and_then(|path| path.parent())
        .map(|path| path.to_string_lossy().into_owned());
    let status = format!("{:?}", summary.state).to_ascii_lowercase();
    let entry = HistoryEntry {
        id: Uuid::new_v4(),
        action_id: summary.action_id.clone(),
        input_count: summary.total as u64,
        output_count: summary.outputs.len() as u64,
        input_bytes,
        output_bytes,
        destination,
        status,
        duration_ms: summary.duration_ms,
        created_at: Utc::now(),
    };
    let _ = tokio::task::spawn_blocking(move || {
        if let Err(error) = storage.record_history(&entry) {
            tracing::warn!(%error, "could not persist operation history");
        }
    })
    .await;
}

async fn output_size(paths: &[PathBuf]) -> u64 {
    let paths = paths.to_vec();
    tokio::task::spawn_blocking(move || paths.iter().map(|path| path_size(path)).sum())
        .await
        .unwrap_or(0)
}

fn path_size(path: &std::path::Path) -> u64 {
    let Ok(metadata) = std::fs::symlink_metadata(path) else { return 0; };
    if metadata.file_type().is_symlink() { return 0; }
    if metadata.is_file() { return metadata.len(); }
    if !metadata.is_dir() { return 0; }

    let mut total = 0_u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else { continue; };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else { continue; };
            if metadata.file_type().is_symlink() { continue; }
            if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            } else if metadata.is_dir() {
                stack.push(path);
            }
        }
    }
    total
}
