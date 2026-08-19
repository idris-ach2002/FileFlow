use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(pub Uuid);

impl JobId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
