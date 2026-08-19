use chrono::{DateTime, Utc};
use fileflow_domain::{Asset, AssetKind, FormatFamily, WorkspaceId};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
use thiserror::Error;

const MAX_PAGE_SIZE: usize = 500;
const DEFAULT_PAGE_SIZE: usize = 100;

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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AssetQuery {
    pub offset: usize,
    pub limit: usize,
    pub family: Option<FormatFamily>,
    pub kind: Option<AssetKind>,
    pub search: Option<String>,
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
        let workspace = workspaces.get_mut(&id).ok_or(WorkspaceError::NotFound(id))?;

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
        let workspace = workspaces.get_mut(&id).ok_or(WorkspaceError::NotFound(id))?;
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
        let workspace = workspaces.get_mut(&id).ok_or(WorkspaceError::NotFound(id))?;
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

    pub fn list_assets(&self, id: WorkspaceId, query: AssetQuery) -> Result<AssetPage, WorkspaceError> {
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

        let search = search.as_deref();
        let family = query.family;
        let kind = query.kind;

        let total = workspace
            .assets
            .iter()
            .filter(|asset| asset_matches(asset, family, kind, search))
            .count();
        let items = workspace
            .assets
            .iter()
            .filter(|asset| asset_matches(asset, family, kind, search))
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
}


fn asset_matches(
    asset: &Asset,
    family: Option<FormatFamily>,
    kind: Option<AssetKind>,
    search: Option<&str>,
) -> bool {
    family.is_none_or(|family| asset.family() == family)
        && kind.is_none_or(|kind| asset.kind() == kind)
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

#[cfg(test)]
mod tests {
    use super::*;
    use fileflow_domain::{
        AssetCommon, AssetId, DetectedFormat, DetectionConfidence, FileAsset,
    };

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
                extension: None,
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
}
