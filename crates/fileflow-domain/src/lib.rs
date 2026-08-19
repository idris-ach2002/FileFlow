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
        max_parallel_instances: 2,
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
