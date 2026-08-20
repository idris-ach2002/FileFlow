use crate::{AppState, commands::account::require_active_session};
use chrono::Utc;
use fileflow_domain::{JobId, OutputPolicy};
use fileflow_executor::{EnginePaths, ExecutionInput, ExecutionRequest};
use fileflow_storage::{AutomationJobRecord, RecipeRecord, WatchedFolderRecord};
use fileflow_workflows::{WorkflowDefinition, WorkflowStep};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use tauri::{AppHandle, Emitter, Manager, State, ipc::Channel};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowEvent {
    pub event: String,
    pub job_id: Uuid,
    pub step_id: Option<String>,
    pub completed_steps: usize,
    pub total_steps: usize,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecipeRequest {
    pub recipe_id: Uuid,
    #[serde(default)]
    pub input_paths: Vec<PathBuf>,
    pub workspace_id: Option<fileflow_domain::WorkspaceId>,
    #[serde(default)]
    pub selected_asset_ids: Vec<fileflow_domain::AssetId>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveWatchedFolderRequest {
    pub id: Option<Uuid>,
    pub path: String,
    pub recipe_id: Uuid,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default = "default_stability_seconds")]
    pub stability_seconds: u64,
}

#[tauri::command]
pub async fn run_recipe(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RunRecipeRequest,
    on_event: Channel<WorkflowEvent>,
) -> Result<AutomationJobRecord, String> {
    let account_id = require_active_session(&state)?;
    let source_paths = if let Some(workspace_id) = request.workspace_id {
        state
            .core
            .workspaces
            .select_assets(workspace_id, &request.selected_asset_ids, &[])
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|asset| !matches!(asset, fileflow_domain::Asset::Directory(_)))
            .map(|asset| asset.common().path.clone())
            .collect()
    } else {
        request.input_paths
    };
    let input_paths = sanitize_inputs(source_paths)?;
    let recipe = recipe_for(&state, account_id, request.recipe_id)?;
    execute_recipe_job(
        &app,
        account_id,
        recipe,
        input_paths,
        None,
        Some(on_event),
    )
    .await
}

#[tauri::command]
pub async fn resume_automation_job(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: Uuid,
    on_event: Channel<WorkflowEvent>,
) -> Result<AutomationJobRecord, String> {
    let account_id = require_active_session(&state)?;
    let previous = state
        .storage
        .automation_job_for(account_id, job_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Job d’automatisation introuvable.".to_owned())?;
    if !matches!(previous.status.as_str(), "interrupted" | "failed" | "cancelled") {
        return Err("Ce job n’est pas dans un état reprenable.".into());
    }
    let recipe_id = previous
        .recipe_id
        .ok_or_else(|| "La recette liée à ce job n’existe plus.".to_owned())?;
    let recipe = recipe_for(&state, account_id, recipe_id)?;
    let inputs = previous.input_paths.iter().map(PathBuf::from).collect();
    execute_recipe_job(
        &app,
        account_id,
        recipe,
        inputs,
        Some(previous),
        Some(on_event),
    )
    .await
}

#[tauri::command]
pub fn automation_jobs(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<AutomationJobRecord>, String> {
    let account_id = require_active_session(&state)?;
    state
        .storage
        .automation_jobs_for(account_id, limit.unwrap_or(100))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn cancel_automation_job(state: State<'_, AppState>, job_id: Uuid) -> bool {
    if require_active_session(&state).is_err() {
        return false;
    }
    state.jobs.get(&JobId(job_id)).is_some_and(|entry| {
        entry.value().cancel();
        true
    })
}

#[tauri::command]
pub fn watched_folders(state: State<'_, AppState>) -> Result<Vec<WatchedFolderRecord>, String> {
    let account_id = require_active_session(&state)?;
    state
        .storage
        .watched_folders_for(account_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_watched_folder(
    state: State<'_, AppState>,
    request: SaveWatchedFolderRequest,
) -> Result<WatchedFolderRecord, String> {
    let account_id = require_active_session(&state)?;
    let path = PathBuf::from(request.path.trim());
    let metadata = std::fs::metadata(&path)
        .map_err(|_| "Le dossier surveillé est introuvable ou inaccessible.".to_owned())?;
    if !metadata.is_dir() {
        return Err("Un dossier est requis pour créer une surveillance.".into());
    }
    if state
        .storage
        .recipes_for(account_id)
        .map_err(|error| error.to_string())?
        .iter()
        .all(|recipe| recipe.id != request.recipe_id || !recipe.enabled)
    {
        return Err("Choisissez une recette existante et active.".into());
    }

    let normalized_extensions = request
        .extensions
        .into_iter()
        .map(|value| value.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|value| !value.is_empty() && value.len() <= 16)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let now = Utc::now();
    let record = WatchedFolderRecord {
        id: request.id.unwrap_or_else(Uuid::new_v4),
        path: path.to_string_lossy().into_owned(),
        recipe_id: request.recipe_id,
        enabled: request.enabled,
        recursive: request.recursive,
        extensions: normalized_extensions,
        stability_seconds: request.stability_seconds.clamp(1, 300),
        last_scan_at: None,
        created_at: now,
        updated_at: now,
    };
    state
        .storage
        .save_watched_folder_for(account_id, &record)
        .map_err(|error| error.to_string())?;
    Ok(record)
}

#[tauri::command]
pub fn delete_watched_folder(
    state: State<'_, AppState>,
    watch_id: Uuid,
) -> Result<(), String> {
    let account_id = require_active_session(&state)?;
    state
        .storage
        .delete_watched_folder_for(account_id, watch_id)
        .map_err(|error| error.to_string())
}

pub async fn watch_loop(app: AppHandle) {
    let mut ticker = tokio::time::interval(Duration::from_secs(4));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let account_id = {
            let state = app.state::<AppState>();
            match require_active_session(&state) {
                Ok(account_id) => account_id,
                Err(_) => continue,
            }
        };
        let watches = {
            let state = app.state::<AppState>();
            match state.storage.watched_folders_for(account_id) {
                Ok(watches) => watches,
                Err(error) => {
                    tracing::warn!(%error, "unable to load watched folders");
                    continue;
                }
            }
        };
        for watch in watches.into_iter().filter(|watch| watch.enabled) {
            if let Err(error) = process_watch(&app, account_id, watch).await {
                tracing::warn!(%error, "watched folder pass failed");
            }
        }
    }
}

async fn process_watch(
    app: &AppHandle,
    account_id: Uuid,
    watch: WatchedFolderRecord,
) -> Result<(), String> {
    let watch_for_scan = watch.clone();
    let candidates = tokio::task::spawn_blocking(move || scan_watch(&watch_for_scan))
        .await
        .map_err(|error| format!("Le worker de surveillance a échoué : {error}"))??;

    for candidate in candidates.into_iter().take(128) {
        let path_text = candidate.path.to_string_lossy().into_owned();
        let already_seen = {
            let state = app.state::<AppState>();
            state
                .storage
                .watch_seen_signature(account_id, watch.id, &path_text)
                .map_err(|error| error.to_string())?
                .is_some_and(|signature| signature == candidate.signature)
        };
        if already_seen {
            continue;
        }

        let recipe = {
            let state = app.state::<AppState>();
            match recipe_for(&state, account_id, watch.recipe_id) {
                Ok(recipe) => recipe,
                Err(error) => {
                    tracing::warn!(%error, watch_id=%watch.id, "watched recipe missing");
                    break;
                }
            }
        };
        let result = execute_recipe_job(
            app,
            account_id,
            recipe,
            vec![candidate.path.clone()],
            None,
            None,
        )
        .await;
        if result.is_ok() {
            let state = app.state::<AppState>();
            state
                .storage
                .mark_watch_seen(account_id, watch.id, &path_text, &candidate.signature)
                .map_err(|error| error.to_string())?;
        }
        let _ = app.emit(
            "fileflow://watch-activity",
            serde_json::json!({
                "watchId": watch.id,
                "path": path_text,
                "success": result.is_ok(),
            }),
        );
    }

    let scanned_at = Utc::now();
    let state = app.state::<AppState>();
    state
        .storage
        .mark_watched_folder_scanned(account_id, watch.id, scanned_at)
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn execute_recipe_job(
    app: &AppHandle,
    account_id: Uuid,
    recipe: RecipeRecord,
    input_paths: Vec<PathBuf>,
    previous: Option<AutomationJobRecord>,
    on_event: Option<Channel<WorkflowEvent>>,
) -> Result<AutomationJobRecord, String> {
    let definition = workflow_from_recipe(&recipe)?;
    let plan = definition.validate().map_err(|error| error.to_string())?;
    let job_id = previous.as_ref().map_or_else(Uuid::new_v4, |job| job.id);
    let domain_job_id = JobId(job_id);
    let cancellation = CancellationToken::new();

    let (storage, core, executor) = {
        let state = app.state::<AppState>();
        state.jobs.insert(domain_job_id, cancellation.clone());
        (
            state.storage.clone(),
            state.core.clone(),
            state.runtime.read().executor.clone(),
        )
    };

    let mut outputs_by_step = previous
        .as_ref()
        .map(|job| job.outputs_by_step.clone())
        .unwrap_or_default();
    let completed_before = previous
        .as_ref()
        .map(|job| job.current_step as usize)
        .unwrap_or(0)
        .min(plan.order.len());
    let now = Utc::now();
    let mut job = AutomationJobRecord {
        id: job_id,
        recipe_id: Some(recipe.id),
        status: "running".into(),
        current_step: completed_before as u64,
        total_steps: plan.order.len() as u64,
        input_paths: input_paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        outputs_by_step: outputs_by_step.clone(),
        error: None,
        created_at: previous.as_ref().map_or(now, |value| value.created_at),
        updated_at: now,
    };
    storage
        .save_automation_job_for(account_id, &job)
        .map_err(|error| error.to_string())?;
    emit_workflow(
        on_event.as_ref(),
        WorkflowEvent {
            event: "started".into(),
            job_id,
            step_id: None,
            completed_steps: completed_before,
            total_steps: plan.order.len(),
            message: Some(recipe.name.clone()),
        },
    );

    let probes = core.engines.probe_all().await;
    let engine_paths = probes
        .into_iter()
        .filter(|probe| probe.available)
        .filter_map(|probe| probe.executable.map(|path| (probe.id, path)))
        .collect::<HashMap<_, _>>();

    for (index, step_id) in plan.order.iter().enumerate().skip(completed_before) {
        if cancellation.is_cancelled() {
            job.status = "cancelled".into();
            job.error = Some("Workflow annulé.".into());
            break;
        }
        let step = definition
            .step(step_id)
            .ok_or_else(|| format!("Étape introuvable : {step_id}"))?;
        if !fileflow_executor::is_supported(&step.action_id) {
            job.status = "failed".into();
            job.error = Some(format!(
                "L’action « {} » n’est pas reliée à un exécuteur local.",
                step.action_id
            ));
            break;
        }
        let action = core
            .capabilities
            .action(&step.action_id)
            .ok_or_else(|| format!("Action inconnue : {}", step.action_id))?;
        let missing = action
            .required_engines
            .iter()
            .filter(|engine| !engine_paths.contains_key(engine.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            job.status = "failed".into();
            job.error = Some(format!("Moteurs manquants : {}", missing.join(", ")));
            break;
        }

        emit_workflow(
            on_event.as_ref(),
            WorkflowEvent {
                event: "stepStarted".into(),
                job_id,
                step_id: Some(step.id.clone()),
                completed_steps: index,
                total_steps: plan.order.len(),
                message: Some(action.title.clone()),
            },
        );

        let inputs = workflow_step_inputs(step, &input_paths, &outputs_by_step);
        if inputs.is_empty() {
            job.status = "failed".into();
            job.error = Some(format!("L’étape « {} » n’a aucune entrée exploitable.", step.id));
            break;
        }
        let execution_inputs = inputs
            .iter()
            .map(|path| ExecutionInput {
                path: path.clone(),
                source_root: None,
            })
            .collect::<Vec<_>>();
        let (events_tx, mut events_rx) = mpsc::channel(32);
        let drain = tokio::spawn(async move { while events_rx.recv().await.is_some() {} });
        let result = executor
            .execute(
                domain_job_id,
                ExecutionRequest {
                    action_id: step.action_id.clone(),
                    inputs: execution_inputs,
                    output_policy: step.output_policy.clone(),
                    target_format: step.target_format.clone(),
                    quality: step.quality.clone(),
                    parameters: step.parameters.clone(),
                },
                EnginePaths::new(engine_paths.clone()),
                cancellation.clone(),
                events_tx,
            )
            .await;
        let _ = drain.await;

        match result {
            Ok(summary) if summary.state == fileflow_domain::JobState::Completed => {
                let outputs = summary
                    .outputs
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                outputs_by_step.insert(step.id.clone(), outputs);
                job.outputs_by_step = outputs_by_step.clone();
                job.current_step = (index + 1) as u64;
                job.updated_at = Utc::now();
                storage
                    .save_automation_job_for(account_id, &job)
                    .map_err(|error| error.to_string())?;
                emit_workflow(
                    on_event.as_ref(),
                    WorkflowEvent {
                        event: "stepCompleted".into(),
                        job_id,
                        step_id: Some(step.id.clone()),
                        completed_steps: index + 1,
                        total_steps: plan.order.len(),
                        message: None,
                    },
                );
            }
            Ok(summary) => {
                job.status = if summary.state == fileflow_domain::JobState::Cancelled {
                    "cancelled".into()
                } else {
                    "failed".into()
                };
                job.error = summary
                    .failures
                    .first()
                    .map(|failure| failure.message.clone())
                    .or_else(|| Some("Une étape du workflow n’a pas abouti.".into()));
                break;
            }
            Err(error) => {
                job.status = if cancellation.is_cancelled() {
                    "cancelled".into()
                } else {
                    "failed".into()
                };
                job.error = Some(error.to_string());
                break;
            }
        }
    }

    if job.current_step as usize == plan.order.len() && job.status == "running" {
        job.status = "completed".into();
        job.error = None;
    }
    job.updated_at = Utc::now();
    job.outputs_by_step = outputs_by_step;
    storage
        .save_automation_job_for(account_id, &job)
        .map_err(|error| error.to_string())?;
    {
        let state = app.state::<AppState>();
        state.jobs.remove(&domain_job_id);
    }
    emit_workflow(
        on_event.as_ref(),
        WorkflowEvent {
            event: "finished".into(),
            job_id,
            step_id: None,
            completed_steps: job.current_step as usize,
            total_steps: plan.order.len(),
            message: job.error.clone(),
        },
    );
    Ok(job)
}

fn workflow_step_inputs(
    step: &WorkflowStep,
    initial_inputs: &[PathBuf],
    outputs: &HashMap<String, Vec<String>>,
) -> Vec<PathBuf> {
    if step.depends_on.is_empty() {
        return initial_inputs.to_vec();
    }
    let mut seen = HashSet::new();
    step.depends_on
        .iter()
        .filter_map(|dependency| outputs.get(dependency))
        .flat_map(|paths| paths.iter())
        .filter(|path| seen.insert((*path).clone()))
        .map(PathBuf::from)
        .collect()
}

fn workflow_from_recipe(recipe: &RecipeRecord) -> Result<WorkflowDefinition, String> {
    if let Ok(definition) = serde_json::from_str::<WorkflowDefinition>(&recipe.steps_json) {
        return Ok(definition);
    }
    let legacy: Value = serde_json::from_str(&recipe.steps_json)
        .map_err(|error| format!("Recette invalide : {error}"))?;
    let actions = legacy
        .get("actions")
        .and_then(Value::as_array)
        .ok_or_else(|| "Cette recette ne contient aucune action exécutable.".to_owned())?;
    let mut previous: Option<String> = None;
    let mut steps = Vec::new();
    for (index, action) in actions.iter().filter_map(Value::as_str).enumerate() {
        let id = format!("step-{}", index + 1);
        steps.push(WorkflowStep {
            id: id.clone(),
            action_id: action.into(),
            depends_on: previous.iter().cloned().collect(),
            target_format: None,
            quality: None,
            parameters: HashMap::new(),
            output_policy: OutputPolicy::default(),
        });
        previous = Some(id);
    }
    Ok(WorkflowDefinition {
        version: 1,
        name: recipe.name.clone(),
        description: recipe.description.clone(),
        steps,
    })
}

fn recipe_for(state: &AppState, account_id: Uuid, recipe_id: Uuid) -> Result<RecipeRecord, String> {
    state
        .storage
        .recipes_for(account_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|recipe| recipe.id == recipe_id && recipe.enabled)
        .ok_or_else(|| "Recette introuvable ou désactivée.".to_owned())
}

fn sanitize_inputs(inputs: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let mut unique = HashSet::new();
    let sanitized = inputs
        .into_iter()
        .filter(|path| path.exists())
        .filter(|path| unique.insert(path.clone()))
        .take(10_000)
        .collect::<Vec<_>>();
    if sanitized.is_empty() {
        return Err("Aucun fichier ou dossier d’entrée n’est disponible.".into());
    }
    Ok(sanitized)
}

fn emit_workflow(channel: Option<&Channel<WorkflowEvent>>, event: WorkflowEvent) {
    if let Some(channel) = channel {
        let _ = channel.send(event);
    }
}

#[derive(Debug)]
struct WatchCandidate {
    path: PathBuf,
    signature: String,
}

fn scan_watch(watch: &WatchedFolderRecord) -> Result<Vec<WatchCandidate>, String> {
    let root = Path::new(&watch.path);
    let max_depth = if watch.recursive { 64 } else { 1 };
    let minimum_age = Duration::from_secs(watch.stability_seconds.max(1));
    let now = SystemTime::now();
    let created_cutoff = SystemTime::UNIX_EPOCH
        + Duration::from_secs(watch.created_at.timestamp().max(0) as u64);
    let allowed = watch
        .extensions
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let mut candidates = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(Result::ok)
        .take(10_000)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !allowed.is_empty() && !allowed.contains(&extension) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        // A watched folder reacts to arrivals/changes after activation. Existing
        // old files are not silently processed on the first watcher pass.
        if modified < created_cutoff {
            continue;
        }
        if now.duration_since(modified).unwrap_or_default() < minimum_age {
            continue;
        }
        let modified_ns = modified
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        candidates.push(WatchCandidate {
            path: path.to_path_buf(),
            signature: format!("{}:{modified_ns}", metadata.len()),
        });
    }
    Ok(candidates)
}

fn default_true() -> bool {
    true
}

fn default_stability_seconds() -> u64 {
    3
}
