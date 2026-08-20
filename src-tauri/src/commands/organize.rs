use crate::{AppState, commands::account::require_active_session};
use chrono::{DateTime, Datelike, Utc};
use fileflow_analysis::{DuplicateInput, DuplicateReport};
use fileflow_domain::{Asset, AssetId, FormatFamily, ResourceProfile, WorkspaceId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};
use tauri::State;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameRule {
    #[serde(default = "default_rename_template")]
    pub template: String,
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub replace: String,
    #[serde(default = "default_counter_start")]
    pub counter_start: u64,
    #[serde(default = "default_counter_padding")]
    pub counter_padding: usize,
    #[serde(default)]
    pub case_mode: String,
    #[serde(default = "default_true")]
    pub preserve_extension: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewRequest {
    pub workspace_id: WorkspaceId,
    #[serde(default)]
    pub selected_asset_ids: Vec<AssetId>,
    pub rule: RenameRule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreviewItem {
    pub asset_id: AssetId,
    pub source: PathBuf,
    pub target: PathBuf,
    pub changed: bool,
    pub conflict: bool,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreview {
    pub items: Vec<RenamePreviewItem>,
    pub total: usize,
    pub changed: usize,
    pub conflicts: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRenameRequest {
    pub workspace_id: WorkspaceId,
    #[serde(default)]
    pub selected_asset_ids: Vec<AssetId>,
    pub rule: RenameRule,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationPreviewRequest {
    pub workspace_id: WorkspaceId,
    #[serde(default)]
    pub selected_asset_ids: Vec<AssetId>,
    pub destination_root: PathBuf,
    #[serde(default = "default_organization_mode")]
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationPreviewItem {
    pub asset_id: AssetId,
    pub source: PathBuf,
    pub target: PathBuf,
    pub category: String,
    pub conflict_resolved: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationPreview {
    pub items: Vec<OrganizationPreviewItem>,
    pub total: usize,
    pub truncated: bool,
    pub categories: HashMap<String, usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOrganizationRequest {
    pub workspace_id: WorkspaceId,
    #[serde(default)]
    pub selected_asset_ids: Vec<AssetId>,
    pub destination_root: PathBuf,
    #[serde(default = "default_organization_mode")]
    pub mode: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicateCleanupStrategy {
    Newest,
    Oldest,
    ShortestPath,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCleanupRequest {
    pub workspace_id: WorkspaceId,
    #[serde(default)]
    pub selected_asset_ids: Vec<AssetId>,
    pub strategy: DuplicateCleanupStrategy,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCleanupGroup {
    pub hash: String,
    pub size_bytes: u64,
    pub keep_asset_id: AssetId,
    pub keep_path: PathBuf,
    pub quarantine_asset_ids: Vec<AssetId>,
    pub quarantine_paths: Vec<PathBuf>,
    pub reclaimable_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCleanupPlan {
    pub groups: Vec<DuplicateCleanupGroup>,
    pub reclaimable_bytes: u64,
    pub quarantine_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuarantineDuplicatesRequest {
    pub workspace_id: WorkspaceId,
    pub asset_ids: Vec<AssetId>,
    pub destination: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationResult {
    pub processed: usize,
    pub destination: Option<PathBuf>,
    pub warnings: Vec<String>,
}

#[tauri::command]
pub fn preview_batch_rename(
    state: State<'_, AppState>,
    request: RenamePreviewRequest,
) -> Result<RenamePreview, String> {
    require_active_session(&state)?;
    let mut items = build_rename_plan(
        &state,
        request.workspace_id,
        &request.selected_asset_ids,
        &request.rule,
    )?;
    let total = items.len();
    let changed = items.iter().filter(|item| item.changed).count();
    let conflicts = items.iter().filter(|item| item.conflict).count();
    let truncated = total > PREVIEW_ITEM_LIMIT;
    items.truncate(PREVIEW_ITEM_LIMIT);
    Ok(RenamePreview {
        items,
        total,
        changed,
        conflicts,
        truncated,
    })
}

#[tauri::command]
pub async fn apply_batch_rename(
    state: State<'_, AppState>,
    request: ApplyRenameRequest,
) -> Result<FileOperationResult, String> {
    require_active_session(&state)?;
    let items = build_rename_plan(
        &state,
        request.workspace_id,
        &request.selected_asset_ids,
        &request.rule,
    )?;
    if items.iter().any(|item| item.conflict) {
        return Err(
            "Des collisions sont apparues depuis l’aperçu. Régénérez-le avant d’appliquer.".into(),
        );
    }
    let operations = items
        .into_iter()
        .filter(|item| item.changed)
        .map(|item| (item.source, item.target))
        .collect::<Vec<_>>();
    let processed = tokio::task::spawn_blocking(move || transactional_rename(operations))
        .await
        .map_err(|error| format!("Le worker de renommage a échoué : {error}"))??;
    Ok(FileOperationResult {
        processed,
        destination: None,
        warnings: Vec::new(),
    })
}

#[tauri::command]
pub fn preview_organization(
    state: State<'_, AppState>,
    request: OrganizationPreviewRequest,
) -> Result<OrganizationPreview, String> {
    require_active_session(&state)?;
    let (mut items, categories) = build_organization_plan(
        &state,
        request.workspace_id,
        &request.selected_asset_ids,
        &request.destination_root,
        &request.mode,
    )?;
    let total = items.len();
    let truncated = total > PREVIEW_ITEM_LIMIT;
    items.truncate(PREVIEW_ITEM_LIMIT);
    Ok(OrganizationPreview {
        items,
        total,
        truncated,
        categories,
    })
}

#[tauri::command]
pub async fn apply_organization(
    state: State<'_, AppState>,
    request: ApplyOrganizationRequest,
) -> Result<FileOperationResult, String> {
    require_active_session(&state)?;
    let (items, _) = build_organization_plan(
        &state,
        request.workspace_id,
        &request.selected_asset_ids,
        &request.destination_root,
        &request.mode,
    )?;
    let operations = items
        .into_iter()
        .filter(|item| item.source != item.target)
        .map(|item| (item.source, item.target))
        .collect::<Vec<_>>();
    let processed = tokio::task::spawn_blocking(move || move_operations(operations))
        .await
        .map_err(|error| format!("Le worker de classement a échoué : {error}"))??;
    Ok(FileOperationResult {
        processed,
        destination: Some(request.destination_root),
        warnings: Vec::new(),
    })
}

fn build_rename_plan(
    state: &AppState,
    workspace_id: WorkspaceId,
    selected_asset_ids: &[AssetId],
    rule: &RenameRule,
) -> Result<Vec<RenamePreviewItem>, String> {
    let assets = state
        .core
        .workspaces
        .select_assets(workspace_id, selected_asset_ids, &[])
        .map_err(|error| error.to_string())?;
    let mut reserved = HashSet::<PathBuf>::new();
    let mut items = Vec::new();
    let mut counter = rule.counter_start;

    for asset in assets
        .into_iter()
        .filter(|asset| !matches!(asset, Asset::Directory(_)))
    {
        let common = asset.common();
        let source = common.path.clone();
        let parent = source.parent().unwrap_or_else(|| Path::new("."));
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let stem = source
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(&common.name);
        let date = common.modified_at.unwrap_or_else(Utc::now);
        let parent_name = parent
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let mut name = render_name(rule, stem, extension, parent_name, date, counter);
        counter = counter.saturating_add(1);
        if rule.preserve_extension
            && !extension.is_empty()
            && Path::new(&name).extension().is_none()
        {
            name.push('.');
            name.push_str(extension);
        }
        name = sanitize_filename(&name);
        let target = parent.join(name);
        let duplicate_target = !reserved.insert(normalized_path_key(&target));
        let external_collision = target != source && target.exists();
        let conflict = duplicate_target || external_collision;
        items.push(RenamePreviewItem {
            asset_id: common.id,
            changed: target != source,
            source,
            target,
            conflict,
            warning: conflict.then(|| {
                if duplicate_target {
                    "Deux éléments produisent le même nom.".into()
                } else {
                    "Un élément existe déjà avec ce nom.".into()
                }
            }),
        });
    }
    Ok(items)
}

fn build_organization_plan(
    state: &AppState,
    workspace_id: WorkspaceId,
    selected_asset_ids: &[AssetId],
    destination_root: &Path,
    mode: &str,
) -> Result<(Vec<OrganizationPreviewItem>, HashMap<String, usize>), String> {
    if destination_root.as_os_str().is_empty() {
        return Err("Choisissez un dossier de destination.".into());
    }
    let assets = state
        .core
        .workspaces
        .select_assets(workspace_id, selected_asset_ids, &[])
        .map_err(|error| error.to_string())?;
    let mut reserved = HashSet::new();
    let mut categories = HashMap::new();
    let mut items = Vec::new();
    for asset in assets
        .into_iter()
        .filter(|asset| !matches!(asset, Asset::Directory(_)))
    {
        let common = asset.common();
        let category = family_category(asset.family()).to_owned();
        *categories.entry(category.clone()).or_insert(0) += 1;
        let date = common.modified_at.unwrap_or_else(Utc::now);
        let mut directory = destination_root.join(&category);
        if mode == "typeDate" || mode == "date" {
            if mode == "date" {
                directory = destination_root.to_path_buf();
            }
            directory = directory
                .join(format!("{:04}", date.year()))
                .join(format!("{:02}", date.month()));
        }
        let (target, resolved) =
            reserve_incremented_target(directory.join(&common.name), &mut reserved);
        items.push(OrganizationPreviewItem {
            asset_id: common.id,
            source: common.path.clone(),
            target,
            category,
            conflict_resolved: resolved,
        });
    }
    Ok((items, categories))
}

#[tauri::command]
pub async fn duplicate_cleanup_plan(
    state: State<'_, AppState>,
    request: DuplicateCleanupRequest,
) -> Result<DuplicateCleanupPlan, String> {
    require_active_session(&state)?;
    let report =
        duplicate_report(&state, request.workspace_id, &request.selected_asset_ids).await?;
    Ok(build_duplicate_plan(report, request.strategy))
}

#[tauri::command]
pub async fn quarantine_duplicates(
    state: State<'_, AppState>,
    request: QuarantineDuplicatesRequest,
) -> Result<FileOperationResult, String> {
    let account_id = require_active_session(&state)?;
    let selected = state
        .core
        .workspaces
        .select_assets(request.workspace_id, &request.asset_ids, &[])
        .map_err(|error| error.to_string())?;
    let allowed_ids = request.asset_ids.iter().copied().collect::<HashSet<_>>();
    let sources = selected
        .into_iter()
        .filter(|asset| allowed_ids.contains(&asset.id()))
        .filter(|asset| !matches!(asset, Asset::Directory(_)))
        .map(|asset| asset.common().path.clone())
        .collect::<Vec<_>>();
    if sources.len() != allowed_ids.len() {
        return Err("La sélection de doublons ne correspond plus au workspace courant.".into());
    }
    let destination = request
        .destination
        .or_else(|| {
            state
                .storage
                .onboarding(account_id)
                .ok()
                .flatten()
                .and_then(|value| value.storage_directory)
                .map(|root| root.join("Doublons à vérifier"))
        })
        .ok_or_else(|| {
            "Configurez un dossier FileFlow ou choisissez une destination de quarantaine."
                .to_owned()
        })?;
    let destination_for_worker = destination.clone();
    let result =
        tokio::task::spawn_blocking(move || quarantine_files(sources, &destination_for_worker))
            .await
            .map_err(|error| format!("Le worker de quarantaine a échoué : {error}"))??;
    Ok(FileOperationResult {
        processed: result.0,
        destination: Some(destination),
        warnings: result.1,
    })
}

async fn duplicate_report(
    state: &AppState,
    workspace_id: WorkspaceId,
    selected_asset_ids: &[AssetId],
) -> Result<DuplicateReport, String> {
    let assets = state
        .core
        .workspaces
        .select_assets(workspace_id, selected_asset_ids, &[])
        .map_err(|error| error.to_string())?;
    let inputs = assets
        .into_iter()
        .map(|asset| DuplicateInput {
            asset_id: asset.id(),
            path: asset.common().path.clone(),
            size_bytes: asset.size_bytes(),
        })
        .collect::<Vec<_>>();
    let cancellation = CancellationToken::new();
    let scheduler = state.runtime.read().scheduler.clone();
    let _lease = scheduler
        .acquire(
            "native-duplicates",
            ResourceProfile {
                cpu_weight: 4,
                memory_mb: 256,
                io_weight: 3,
                internally_threaded: true,
                max_parallel_instances: 1,
            },
            &cancellation,
        )
        .await
        .map_err(|error| error.to_string())?;
    let threads = scheduler.budget().cpu_tokens.clamp(1, 4);
    tokio::task::spawn_blocking(move || fileflow_analysis::confirm_duplicates(inputs, threads))
        .await
        .map_err(|error| format!("Le worker de doublons a échoué : {error}"))
}

fn build_duplicate_plan(
    report: DuplicateReport,
    strategy: DuplicateCleanupStrategy,
) -> DuplicateCleanupPlan {
    let warnings = report
        .warnings
        .into_iter()
        .map(|warning| format!("{} : {}", warning.path.display(), warning.message))
        .collect();
    let mut groups = Vec::new();
    for group in report.confirmed_groups {
        let mut assets = group.assets;
        assets.sort_by(|left, right| duplicate_order(&left.path, &right.path, strategy));
        let Some(keep) = assets.first().cloned() else {
            continue;
        };
        let quarantine = assets.into_iter().skip(1).collect::<Vec<_>>();
        groups.push(DuplicateCleanupGroup {
            hash: group.hash,
            size_bytes: group.size_bytes,
            keep_asset_id: keep.asset_id,
            keep_path: keep.path,
            quarantine_asset_ids: quarantine.iter().map(|asset| asset.asset_id).collect(),
            quarantine_paths: quarantine.iter().map(|asset| asset.path.clone()).collect(),
            reclaimable_bytes: group.reclaimable_bytes,
        });
    }
    let reclaimable_bytes = groups.iter().map(|group| group.reclaimable_bytes).sum();
    let quarantine_count = groups
        .iter()
        .map(|group| group.quarantine_asset_ids.len())
        .sum();
    DuplicateCleanupPlan {
        groups,
        reclaimable_bytes,
        quarantine_count,
        warnings,
    }
}

fn duplicate_order(
    left: &Path,
    right: &Path,
    strategy: DuplicateCleanupStrategy,
) -> std::cmp::Ordering {
    match strategy {
        DuplicateCleanupStrategy::ShortestPath => left
            .components()
            .count()
            .cmp(&right.components().count())
            .then_with(|| left.cmp(right)),
        DuplicateCleanupStrategy::Newest => modified_key(right)
            .cmp(&modified_key(left))
            .then_with(|| left.cmp(right)),
        DuplicateCleanupStrategy::Oldest => modified_key(left)
            .cmp(&modified_key(right))
            .then_with(|| left.cmp(right)),
    }
}

fn modified_key(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn transactional_rename(operations: Vec<(PathBuf, PathBuf)>) -> Result<usize, String> {
    if operations.is_empty() {
        return Ok(0);
    }
    let mut staged = Vec::with_capacity(operations.len());
    for (source, target) in &operations {
        if target.exists() && target != source {
            return Err(format!("La destination existe déjà : {}", target.display()));
        }
        let parent = source
            .parent()
            .ok_or_else(|| format!("Chemin source invalide : {}", source.display()))?;
        let temporary = parent.join(format!(".fileflow-rename-{}.tmp", Uuid::new_v4()));
        fs::rename(source, &temporary).map_err(|error| {
            rollback_staged(&staged);
            format!("Impossible de préparer {} : {error}", source.display())
        })?;
        staged.push((source.clone(), temporary, target.clone()));
    }
    let mut finalized = 0usize;
    for index in 0..staged.len() {
        let (_, temporary, target) = &staged[index];
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        if let Err(error) = fs::rename(temporary, target) {
            rollback_finalized(&staged, index);
            return Err(format!(
                "Impossible de finaliser {} : {error}",
                target.display()
            ));
        }
        finalized += 1;
    }
    Ok(finalized)
}

fn rollback_staged(staged: &[(PathBuf, PathBuf, PathBuf)]) {
    for (source, temporary, _) in staged.iter().rev() {
        let _ = fs::rename(temporary, source);
    }
}

fn rollback_finalized(staged: &[(PathBuf, PathBuf, PathBuf)], failed_index: usize) {
    for (source, _, target) in staged[..failed_index].iter().rev() {
        let _ = fs::rename(target, source);
    }
    for (source, temporary, _) in staged[failed_index..].iter().rev() {
        if temporary.exists() {
            let _ = fs::rename(temporary, source);
        }
    }
}

fn move_operations(operations: Vec<(PathBuf, PathBuf)>) -> Result<usize, String> {
    let mut completed: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (source, target) in operations {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        if let Err(error) = move_file_safe(&source, &target) {
            for (previous_source, previous_target) in completed.iter().rev() {
                let _ = move_file_safe(previous_target, previous_source);
            }
            return Err(format!(
                "Impossible de déplacer {} : {error}",
                source.display()
            ));
        }
        completed.push((source, target));
    }
    Ok(completed.len())
}

fn move_file_safe(source: &Path, target: &Path) -> Result<(), std::io::Error> {
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(_rename_error) if source.is_file() => {
            fs::copy(source, target)?;
            if let Err(remove_error) = fs::remove_file(source) {
                let _ = fs::remove_file(target);
                return Err(remove_error);
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn quarantine_files(
    sources: Vec<PathBuf>,
    destination: &Path,
) -> Result<(usize, Vec<String>), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    let mut reserved = HashSet::new();
    let mut warnings = Vec::new();
    let mut processed = 0usize;
    for source in sources {
        let name = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("doublon");
        let (target, _) = reserve_incremented_target(destination.join(name), &mut reserved);
        match move_file_safe(&source, &target) {
            Ok(()) => processed += 1,
            Err(error) => warnings.push(format!("{} : {error}", source.display())),
        }
    }
    Ok((processed, warnings))
}

fn render_name(
    rule: &RenameRule,
    stem: &str,
    extension: &str,
    parent: &str,
    date: DateTime<Utc>,
    counter: u64,
) -> String {
    let mut base = if rule.search.is_empty() {
        stem.to_owned()
    } else {
        stem.replace(&rule.search, &rule.replace)
    };
    base = match rule.case_mode.as_str() {
        "lower" => base.to_lowercase(),
        "upper" => base.to_uppercase(),
        "title" => title_case(&base),
        _ => base,
    };
    let counter_text = format!(
        "{:0width$}",
        counter,
        width = rule.counter_padding.clamp(1, 12)
    );
    rule.template
        .replace("{name}", &base)
        .replace("{counter}", &counter_text)
        .replace("{date}", &date.format("%Y-%m-%d").to_string())
        .replace("{year}", &format!("{:04}", date.year()))
        .replace("{month}", &format!("{:02}", date.month()))
        .replace("{day}", &format!("{:02}", date.day()))
        .replace("{parent}", parent)
        .replace("{ext}", extension)
}

fn title_case(value: &str) -> String {
    value
        .split(|character: char| character == '_' || character == '-' || character.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters
                .next()
                .map(|first| {
                    first.to_uppercase().collect::<String>() + &characters.as_str().to_lowercase()
                })
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_filename(value: &str) -> String {
    // Produce names that remain valid when a workflow created on macOS/Linux is
    // later replayed on Windows. Windows rejects < > : " / \ | ? * and all
    // platforms reject NUL/path separators. A portable filename is preferable
    // to silently creating recipes that work only on the machine that authored
    // them.
    let sanitized = value
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\0'
            ) {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let trimmed = sanitized.trim().trim_matches(['.', ' ']);
    if trimmed.is_empty() {
        return "fichier".into();
    }

    let mut portable: String = trimmed.chars().take(240).collect();
    let stem = portable
        .split('.')
        .next()
        .unwrap_or(&portable)
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if reserved {
        portable.insert(0, '_');
    }
    portable
}

fn reserve_incremented_target(base: PathBuf, reserved: &mut HashSet<PathBuf>) -> (PathBuf, bool) {
    let mut target = base.clone();
    let mut index = 2u32;
    let mut resolved = false;
    while target.exists() || !reserved.insert(normalized_path_key(&target)) {
        resolved = true;
        let parent = base.parent().unwrap_or_else(|| Path::new("."));
        let stem = base
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("fichier");
        let extension = base.extension().and_then(|value| value.to_str());
        let mut name = format!("{stem} ({index})");
        if let Some(extension) = extension {
            name.push('.');
            name.push_str(extension);
        }
        target = parent.join(name);
        index = index.saturating_add(1);
    }
    (target, resolved)
}

fn normalized_path_key(path: &Path) -> PathBuf {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        PathBuf::from(path.to_string_lossy().to_lowercase())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        path.to_path_buf()
    }
}

fn family_category(family: FormatFamily) -> &'static str {
    match family {
        FormatFamily::Image => "Images",
        FormatFamily::Pdf => "PDF",
        FormatFamily::Document => "Documents",
        FormatFamily::Spreadsheet => "Tableurs",
        FormatFamily::Presentation => "Présentations",
        FormatFamily::Audio => "Audio",
        FormatFamily::Video => "Vidéos",
        FormatFamily::Archive => "Archives",
        FormatFamily::Ebook => "Livres numériques",
        FormatFamily::Text => "Textes & données",
        FormatFamily::Unknown => "Autres",
    }
}

const PREVIEW_ITEM_LIMIT: usize = 250;

fn default_rename_template() -> String {
    "{name}-{counter}".into()
}
fn default_counter_start() -> u64 {
    1
}
fn default_counter_padding() -> usize {
    3
}
fn default_true() -> bool {
    true
}
fn default_organization_mode() -> String {
    "type".into()
}

#[cfg(test)]
mod portability_tests {
    use super::*;

    #[test]
    fn filename_sanitizer_is_portable_to_windows() {
        assert_eq!(
            sanitize_filename(r#"report<>:"/\|?*.pdf"#),
            "report_________.pdf"
        );
        assert_eq!(sanitize_filename("NUL.txt"), "_NUL.txt");
        assert_eq!(sanitize_filename("NUL.tar.gz"), "_NUL.tar.gz");
        assert_eq!(sanitize_filename("  ...  "), "fichier");
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn collision_keys_follow_case_insensitive_filesystems() {
        assert_eq!(
            normalized_path_key(Path::new("Folder/Report.PDF")),
            normalized_path_key(Path::new("folder/report.pdf"))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn collision_keys_preserve_linux_case_sensitivity() {
        assert_ne!(
            normalized_path_key(Path::new("Folder/Report.PDF")),
            normalized_path_key(Path::new("folder/report.pdf"))
        );
    }
}
