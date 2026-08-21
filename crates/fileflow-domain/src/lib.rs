use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

uuid_id!(JobId);
uuid_id!(AssetId);
uuid_id!(WorkspaceId);
uuid_id!(IntakeRequestId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FormatFamily {
    Image,
    Pdf,
    Document,
    Spreadsheet,
    Presentation,
    Audio,
    Video,
    Archive,
    Ebook,
    Text,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssetKind {
    File,
    Directory,
    Archive,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DetectionConfidence {
    Unknown,
    Extension,
    Magic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedFormat {
    pub id: String,
    pub extension: Option<String>,
    pub mime_type: Option<String>,
    pub family: FormatFamily,
    pub confidence: DetectionConfidence,
}

impl DetectedFormat {
    pub fn unknown(extension: Option<String>) -> Self {
        Self {
            id: "unknown".into(),
            extension,
            mime_type: None,
            family: FormatFamily::Unknown,
            confidence: DetectionConfidence::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetCommon {
    pub id: AssetId,
    pub root_index: usize,
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub name: String,
    pub hidden: bool,
    pub modified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAsset {
    #[serde(flatten)]
    pub common: AssetCommon,
    pub size_bytes: u64,
    pub format: DetectedFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryAsset {
    #[serde(flatten)]
    pub common: AssetCommon,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveAsset {
    #[serde(flatten)]
    pub common: AssetCommon,
    pub size_bytes: u64,
    pub format: DetectedFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymlinkAsset {
    #[serde(flatten)]
    pub common: AssetCommon,
    pub target: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "data")]
pub enum Asset {
    File(FileAsset),
    Directory(DirectoryAsset),
    Archive(ArchiveAsset),
    Symlink(SymlinkAsset),
}

impl Asset {
    pub fn id(&self) -> AssetId {
        match self {
            Self::File(asset) => asset.common.id,
            Self::Directory(asset) => asset.common.id,
            Self::Archive(asset) => asset.common.id,
            Self::Symlink(asset) => asset.common.id,
        }
    }

    pub fn kind(&self) -> AssetKind {
        match self {
            Self::File(_) => AssetKind::File,
            Self::Directory(_) => AssetKind::Directory,
            Self::Archive(_) => AssetKind::Archive,
            Self::Symlink(_) => AssetKind::Symlink,
        }
    }

    pub fn common(&self) -> &AssetCommon {
        match self {
            Self::File(asset) => &asset.common,
            Self::Directory(asset) => &asset.common,
            Self::Archive(asset) => &asset.common,
            Self::Symlink(asset) => &asset.common,
        }
    }

    pub fn family(&self) -> FormatFamily {
        match self {
            Self::File(asset) => asset.format.family,
            Self::Archive(asset) => asset.format.family,
            Self::Directory(_) | Self::Symlink(_) => FormatFamily::Unknown,
        }
    }

    pub fn size_bytes(&self) -> u64 {
        match self {
            Self::File(asset) => asset.size_bytes,
            Self::Archive(asset) => asset.size_bytes,
            Self::Directory(_) | Self::Symlink(_) => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Queued,
    Preparing,
    WaitingForResources,
    Running,
    Finalizing,
    Completed,
    Failed,
    Cancelling,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceProfile {
    pub cpu_weight: u8,
    pub memory_mb: u32,
    pub io_weight: u8,
    pub internally_threaded: bool,
    pub max_parallel_instances: usize,
}

impl ResourceProfile {
    pub const LIGHT: Self = Self {
        cpu_weight: 1,
        memory_mb: 128,
        io_weight: 1,
        internally_threaded: false,
        max_parallel_instances: 8,
    };

    pub const PDF: Self = Self {
        cpu_weight: 2,
        memory_mb: 384,
        io_weight: 2,
        internally_threaded: false,
        max_parallel_instances: 4,
    };

    pub const IMAGE: Self = Self {
        cpu_weight: 3,
        memory_mb: 512,
        io_weight: 2,
        internally_threaded: true,
        max_parallel_instances: 2,
    };

    pub const ARCHIVE: Self = Self {
        cpu_weight: 3,
        memory_mb: 384,
        io_weight: 3,
        internally_threaded: true,
        max_parallel_instances: 2,
    };

    pub const OFFICE: Self = Self {
        cpu_weight: 2,
        memory_mb: 1024,
        io_weight: 2,
        internally_threaded: false,
        // LibreOffice is intentionally serialized. Parallel soffice instances
        // are expensive and historically caused profile-lock contention.
        max_parallel_instances: 1,
    };

    pub const OCR: Self = Self {
        cpu_weight: 4,
        memory_mb: 1024,
        io_weight: 2,
        internally_threaded: true,
        max_parallel_instances: 2,
    };

    pub const MEDIA: Self = Self {
        cpu_weight: 6,
        memory_mb: 1024,
        io_weight: 3,
        internally_threaded: true,
        max_parallel_instances: 1,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationCategory {
    Convert,
    Pdf,
    Image,
    Document,
    Media,
    Archive,
    Extract,
    Organize,
    Privacy,
    Optimize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionScope {
    Single,
    Batch,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionDescriptor {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: OperationCategory,
    pub scopes: Vec<ActionScope>,
    pub accepts: Vec<FormatFamily>,
    pub output_format: Option<String>,
    pub required_engines: Vec<String>,
    pub batchable: bool,
    pub destructive: bool,
    pub featured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRecommendation {
    pub action_id: String,
    pub score: u16,
    pub reason: String,
    pub affected_assets: u64,
    pub ready: bool,
    pub missing_engines: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DestinationPolicy {
    SameFolder,
    Subfolder,
    CustomFolder,
    AskEveryTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictStrategy {
    Increment,
    Skip,
    Replace,
    Ask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NamingStrategy {
    Original,
    OperationSuffix,
    DateSuffix,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputPolicy {
    pub destination: DestinationPolicy,
    pub custom_directory: Option<PathBuf>,
    pub subfolder_name: String,
    pub preserve_tree: bool,
    pub conflict: ConflictStrategy,
    pub naming: NamingStrategy,
    pub overwrite_original: bool,
}

impl Default for OutputPolicy {
    fn default() -> Self {
        Self {
            destination: DestinationPolicy::Subfolder,
            custom_directory: None,
            subfolder_name: "FileFlow".into(),
            preserve_tree: true,
            conflict: ConflictStrategy::Increment,
            naming: NamingStrategy::Original,
            overwrite_original: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PerformanceMode {
    Eco,
    Balanced,
    Fast,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortDirection {
    Ascending,
    Descending,
}
