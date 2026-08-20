use chrono::{DateTime, Utc};
use fileflow_domain::{Asset, AssetId, AssetKind, FormatFamily, SortDirection, WorkspaceId};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    cmp::{Ordering, Reverse},
    collections::HashMap,
    path::PathBuf,
};
use thiserror::Error;

const MAX_PAGE_SIZE: usize = 500;
const DEFAULT_PAGE_SIZE: usize = 100;
const INSIGHT_TOP_ITEMS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceStatus {
    Scanning,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCounts {
    pub assets: u64,
    pub files: u64,
    pub directories: u64,
    pub archives: u64,
    pub symlinks: u64,
    pub total_bytes: u64,
}

impl WorkspaceCounts {
    fn record(&mut self, asset: &Asset) {
        self.assets += 1;
        self.total_bytes = self.total_bytes.saturating_add(asset.size_bytes());
        match asset.kind() {
            AssetKind::File => self.files += 1,
            AssetKind::Directory => self.directories += 1,
            AssetKind::Archive => self.archives += 1,
            AssetKind::Symlink => self.symlinks += 1,
            AssetKind::Other => {}
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyCount {
    pub family: FormatFamily,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub id: WorkspaceId,
    pub status: WorkspaceStatus,
    pub roots: Vec<PathBuf>,
    pub counts: WorkspaceCounts,
    pub families: Vec<FamilyCount>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AssetSortKey {
    #[default]
    Name,
    Size,
    Modified,
    Format,
    Family,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AssetQuery {
    pub offset: usize,
    pub limit: usize,
    pub family: Option<FormatFamily>,
    pub kind: Option<AssetKind>,
    pub search: Option<String>,
    pub include_hidden: bool,
    pub sort_by: AssetSortKey,
    pub sort_direction: SortDirection,
}

impl Default for AssetQuery {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: DEFAULT_PAGE_SIZE,
            family: None,
            kind: None,
            search: None,
            include_hidden: true,
            sort_by: AssetSortKey::Name,
            sort_direction: SortDirection::Ascending,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetPage {
    pub workspace_id: WorkspaceId,
    pub offset: usize,
    pub limit: usize,
    pub total: usize,
    pub items: Vec<Asset>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCount {
    pub extension: String,
    pub count: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetInsight {
    pub id: AssetId,
    pub name: String,
    pub relative_path: PathBuf,
    pub family: FormatFamily,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateSizeCandidate {
    pub size_bytes: u64,
    pub count: usize,
    pub reclaimable_upper_bound: u64,
    pub samples: Vec<AssetInsight>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInsights {
    pub hidden_assets: u64,
    pub unknown_assets: u64,
    pub extension_count: usize,
    pub extensions: Vec<ExtensionCount>,
    pub largest: Vec<AssetInsight>,
    pub duplicate_size_candidates: Vec<DuplicateSizeCandidate>,
    pub potential_duplicate_bytes: u64,
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("workspace {0} was not found")]
    NotFound(WorkspaceId),
}

struct Workspace {
    id: WorkspaceId,
    status: WorkspaceStatus,
    roots: Vec<PathBuf>,
    assets: Vec<Asset>,
    counts: WorkspaceCounts,
    families: HashMap<FormatFamily, u64>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    error: Option<String>,
}

impl Workspace {
    fn snapshot(&self) -> WorkspaceSnapshot {
        let mut families = self
            .families
            .iter()
            .map(|(family, count)| FamilyCount {
                family: *family,
                count: *count,
            })
            .collect::<Vec<_>>();
        families.sort_by_key(|entry| entry.family);

        WorkspaceSnapshot {
            id: self.id,
            status: self.status,
            roots: self.roots.clone(),
            counts: self.counts.clone(),
            families,
            created_at: self.created_at,
            updated_at: self.updated_at,
            error: self.error.clone(),
        }
    }
}

#[derive(Default)]
pub struct WorkspaceManager {
    workspaces: RwLock<HashMap<WorkspaceId, Workspace>>,
}

impl WorkspaceManager {
    pub fn create(&self, roots: Vec<PathBuf>) -> WorkspaceSnapshot {
        let now = Utc::now();
        let workspace = Workspace {
            id: WorkspaceId::new(),
            status: WorkspaceStatus::Scanning,
            roots,
            assets: Vec::new(),
            counts: WorkspaceCounts::default(),
            families: HashMap::new(),
            created_at: now,
            updated_at: now,
            error: None,
        };
        let snapshot = workspace.snapshot();
        self.workspaces.write().insert(workspace.id, workspace);
        snapshot
    }

    pub fn ingest(&self, id: WorkspaceId, assets: &[Asset]) -> Result<(), WorkspaceError> {
        let mut workspaces = self.workspaces.write();
        let workspace = workspaces
            .get_mut(&id)
            .ok_or(WorkspaceError::NotFound(id))?;

        for asset in assets {
            workspace.counts.record(asset);
            if asset.family() != FormatFamily::Unknown {
                *workspace.families.entry(asset.family()).or_default() += 1;
            }
            workspace.assets.push(asset.clone());
        }
        workspace.updated_at = Utc::now();
        Ok(())
    }

    pub fn mark_ready(&self, id: WorkspaceId) -> Result<WorkspaceSnapshot, WorkspaceError> {
        let mut workspaces = self.workspaces.write();
        let workspace = workspaces
            .get_mut(&id)
            .ok_or(WorkspaceError::NotFound(id))?;
        workspace.status = WorkspaceStatus::Ready;
        workspace.error = None;
        workspace.updated_at = Utc::now();
        Ok(workspace.snapshot())
    }

    pub fn mark_failed(
        &self,
        id: WorkspaceId,
        error: impl Into<String>,
    ) -> Result<WorkspaceSnapshot, WorkspaceError> {
        let mut workspaces = self.workspaces.write();
        let workspace = workspaces
            .get_mut(&id)
            .ok_or(WorkspaceError::NotFound(id))?;
        workspace.status = WorkspaceStatus::Failed;
        workspace.error = Some(error.into());
        workspace.updated_at = Utc::now();
        Ok(workspace.snapshot())
    }

    pub fn snapshot(&self, id: WorkspaceId) -> Result<WorkspaceSnapshot, WorkspaceError> {
        self.workspaces
            .read()
            .get(&id)
            .map(Workspace::snapshot)
            .ok_or(WorkspaceError::NotFound(id))
    }

    pub fn family_counts(
        &self,
        id: WorkspaceId,
    ) -> Result<HashMap<FormatFamily, u64>, WorkspaceError> {
        self.workspaces
            .read()
            .get(&id)
            .map(|workspace| workspace.families.clone())
            .ok_or(WorkspaceError::NotFound(id))
    }

    pub fn list_assets(
        &self,
        id: WorkspaceId,
        query: AssetQuery,
    ) -> Result<AssetPage, WorkspaceError> {
        let workspaces = self.workspaces.read();
        let workspace = workspaces.get(&id).ok_or(WorkspaceError::NotFound(id))?;
        let limit = if query.limit == 0 {
            DEFAULT_PAGE_SIZE
        } else {
            query.limit.min(MAX_PAGE_SIZE)
        };
        let search = query
            .search
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_lowercase);

        let mut filtered = workspace
            .assets
            .iter()
            .filter(|asset| asset_matches(asset, &query, search.as_deref()))
            .collect::<Vec<_>>();
        filtered.sort_by(|left, right| {
            compare_assets(left, right, query.sort_by, query.sort_direction)
        });

        let total = filtered.len();
        let items = filtered
            .into_iter()
            .skip(query.offset)
            .take(limit)
            .cloned()
            .collect();

        Ok(AssetPage {
            workspace_id: id,
            offset: query.offset,
            limit,
            total,
            items,
        })
    }

    pub fn select_assets(
        &self,
        id: WorkspaceId,
        selected_ids: &[AssetId],
        families: &[FormatFamily],
    ) -> Result<Vec<Asset>, WorkspaceError> {
        let workspaces = self.workspaces.read();
        let workspace = workspaces.get(&id).ok_or(WorkspaceError::NotFound(id))?;
        let selected = selected_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        let use_explicit_selection = !selected.is_empty();
        Ok(workspace
            .assets
            .iter()
            .filter(|asset| {
                if use_explicit_selection {
                    !matches!(asset, Asset::Symlink(_)) && selected.contains(&asset.id())
                } else {
                    matches!(asset, Asset::File(_) | Asset::Archive(_))
                        && (families.is_empty() || families.contains(&asset.family()))
                }
            })
            .cloned()
            .collect())
    }

    pub fn insights(&self, id: WorkspaceId) -> Result<WorkspaceInsights, WorkspaceError> {
        let workspaces = self.workspaces.read();
        let workspace = workspaces.get(&id).ok_or(WorkspaceError::NotFound(id))?;
        let mut hidden_assets = 0_u64;
        let mut unknown_assets = 0_u64;
        let mut extensions: HashMap<String, (u64, u64)> = HashMap::new();
        let mut sized_assets = Vec::new();
        let mut by_size: HashMap<u64, Vec<&Asset>> = HashMap::new();

        for asset in &workspace.assets {
            hidden_assets += u64::from(asset.common().hidden);
            unknown_assets += u64::from(asset.family() == FormatFamily::Unknown);
            let size = asset.size_bytes();
            if size > 0 {
                sized_assets.push(asset);
                by_size.entry(size).or_default().push(asset);
            }
            if let Some(extension) = asset_extension(asset) {
                let entry = extensions.entry(extension.to_owned()).or_default();
                entry.0 += 1;
                entry.1 = entry.1.saturating_add(size);
            }
        }

        sized_assets.sort_by_key(|asset| Reverse(asset.size_bytes()));
        let largest = sized_assets
            .into_iter()
            .take(INSIGHT_TOP_ITEMS)
            .map(asset_insight)
            .collect();

        let mut extension_rows = extensions
            .into_iter()
            .map(|(extension, (count, total_bytes))| ExtensionCount {
                extension,
                count,
                total_bytes,
            })
            .collect::<Vec<_>>();
        extension_rows.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.extension.cmp(&b.extension))
        });

        let mut duplicate_size_candidates = by_size
            .into_iter()
            .filter(|(_, assets)| assets.len() > 1)
            .map(|(size_bytes, assets)| DuplicateSizeCandidate {
                size_bytes,
                count: assets.len(),
                reclaimable_upper_bound: size_bytes.saturating_mul((assets.len() - 1) as u64),
                samples: assets.into_iter().take(4).map(asset_insight).collect(),
            })
            .collect::<Vec<_>>();
        duplicate_size_candidates.sort_by(|a, b| {
            b.reclaimable_upper_bound
                .cmp(&a.reclaimable_upper_bound)
                .then_with(|| b.count.cmp(&a.count))
        });
        duplicate_size_candidates.truncate(INSIGHT_TOP_ITEMS);
        let potential_duplicate_bytes = duplicate_size_candidates
            .iter()
            .fold(0_u64, |total, group| {
                total.saturating_add(group.reclaimable_upper_bound)
            });

        Ok(WorkspaceInsights {
            hidden_assets,
            unknown_assets,
            extension_count: extension_rows.len(),
            extensions: extension_rows.into_iter().take(12).collect(),
            largest,
            duplicate_size_candidates,
            potential_duplicate_bytes,
        })
    }
}

fn asset_matches(asset: &Asset, query: &AssetQuery, search: Option<&str>) -> bool {
    query.family.is_none_or(|family| asset.family() == family)
        && query.kind.is_none_or(|kind| asset.kind() == kind)
        && (query.include_hidden || !asset.common().hidden)
        && search.is_none_or(|needle| {
            asset.common().name.to_lowercase().contains(needle)
                || asset
                    .common()
                    .relative_path
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(needle)
        })
}

fn compare_assets(
    left: &Asset,
    right: &Asset,
    key: AssetSortKey,
    direction: SortDirection,
) -> Ordering {
    let ordering = match key {
        AssetSortKey::Name => left
            .common()
            .name
            .to_lowercase()
            .cmp(&right.common().name.to_lowercase()),
        AssetSortKey::Size => left.size_bytes().cmp(&right.size_bytes()),
        AssetSortKey::Modified => left.common().modified_at.cmp(&right.common().modified_at),
        AssetSortKey::Format => asset_format(left).cmp(asset_format(right)),
        AssetSortKey::Family => left.family().cmp(&right.family()),
    }
    .then_with(|| {
        left.common()
            .relative_path
            .cmp(&right.common().relative_path)
    });

    match direction {
        SortDirection::Ascending => ordering,
        SortDirection::Descending => ordering.reverse(),
    }
}

fn asset_format(asset: &Asset) -> &str {
    match asset {
        Asset::File(file) => &file.format.id,
        Asset::Archive(archive) => &archive.format.id,
        Asset::Directory(_) => "directory",
        Asset::Symlink(_) => "symlink",
    }
}

fn asset_extension(asset: &Asset) -> Option<&str> {
    match asset {
        Asset::File(file) => file.format.extension.as_deref(),
        Asset::Archive(archive) => archive.format.extension.as_deref(),
        Asset::Directory(_) | Asset::Symlink(_) => None,
    }
}

fn asset_insight(asset: &Asset) -> AssetInsight {
    AssetInsight {
        id: asset.id(),
        name: asset.common().name.clone(),
        relative_path: asset.common().relative_path.clone(),
        family: asset.family(),
        size_bytes: asset.size_bytes(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fileflow_domain::{AssetCommon, AssetId, DetectedFormat, DetectionConfidence, FileAsset};

    fn file(name: &str, family: FormatFamily, size_bytes: u64) -> Asset {
        Asset::File(FileAsset {
            common: AssetCommon {
                id: AssetId::new(),
                root_index: 0,
                path: PathBuf::from(name),
                relative_path: PathBuf::from(name),
                name: name.into(),
                hidden: false,
                modified_at: None,
            },
            size_bytes,
            format: DetectedFormat {
                id: name.rsplit('.').next().unwrap_or("unknown").into(),
                extension: name.rsplit_once('.').map(|(_, extension)| extension.into()),
                mime_type: None,
                family,
                confidence: DetectionConfidence::Extension,
            },
        })
    }

    #[test]
    fn stores_counts_and_pages_assets() {
        let manager = WorkspaceManager::default();
        let workspace = manager.create(vec![PathBuf::from("/tmp/input")]);
        let assets = vec![
            file("one.jpg", FormatFamily::Image, 10),
            file("two.pdf", FormatFamily::Pdf, 20),
            file("three.jpg", FormatFamily::Image, 30),
        ];
        manager.ingest(workspace.id, &assets).unwrap();
        let snapshot = manager.mark_ready(workspace.id).unwrap();

        assert_eq!(snapshot.counts.files, 3);
        assert_eq!(snapshot.counts.total_bytes, 60);
        assert_eq!(
            snapshot
                .families
                .iter()
                .find(|entry| entry.family == FormatFamily::Image)
                .map(|entry| entry.count),
            Some(2)
        );

        let page = manager
            .list_assets(
                workspace.id,
                AssetQuery {
                    family: Some(FormatFamily::Image),
                    limit: 1,
                    ..AssetQuery::default()
                },
            )
            .unwrap();

        assert_eq!(page.total, 2);
        assert_eq!(page.items.len(), 1);
    }

    #[test]
    fn sorts_and_searches_without_copying_entire_workspace() {
        let manager = WorkspaceManager::default();
        let workspace = manager.create(vec![]);
        manager
            .ingest(
                workspace.id,
                &[
                    file("small.jpg", FormatFamily::Image, 1),
                    file("big.jpg", FormatFamily::Image, 500),
                    file("document.pdf", FormatFamily::Pdf, 20),
                ],
            )
            .unwrap();

        let page = manager
            .list_assets(
                workspace.id,
                AssetQuery {
                    search: Some("jpg".into()),
                    sort_by: AssetSortKey::Size,
                    sort_direction: SortDirection::Descending,
                    ..AssetQuery::default()
                },
            )
            .unwrap();

        assert_eq!(page.total, 2);
        assert_eq!(page.items[0].common().name, "big.jpg");
    }

    #[test]
    fn computes_workspace_insights_and_duplicate_size_candidates() {
        let manager = WorkspaceManager::default();
        let workspace = manager.create(vec![]);
        manager
            .ingest(
                workspace.id,
                &[
                    file("a.jpg", FormatFamily::Image, 100),
                    file("b.jpg", FormatFamily::Image, 100),
                    file("large.pdf", FormatFamily::Pdf, 900),
                ],
            )
            .unwrap();

        let insights = manager.insights(workspace.id).unwrap();
        assert_eq!(insights.largest[0].name, "large.pdf");
        assert_eq!(insights.duplicate_size_candidates.len(), 1);
        assert_eq!(insights.potential_duplicate_bytes, 100);
    }
}
