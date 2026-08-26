use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

pub const FILEFLOW_ENGINES: &[EngineDefinition] = &[
    EngineDefinition::new("ffmpeg", "FFmpeg", &["ffmpeg"]),
    EngineDefinition::new("vips", "libvips", &["vips"]),
    EngineDefinition::new("imagemagick", "ImageMagick", &["magick", "convert"]),
    EngineDefinition::new("qpdf", "qpdf", &["qpdf"]),
    EngineDefinition::new("img2pdf", "img2pdf", &["img2pdf"]),
    EngineDefinition::new("poppler", "Poppler", &["pdftoppm", "pdftotext"]),
    EngineDefinition::new("ghostscript", "Ghostscript", &["gs"]),
    EngineDefinition::new("tesseract", "Tesseract", &["tesseract"]),
    EngineDefinition::new("ocrmypdf", "OCRmyPDF", &["ocrmypdf"]),
    EngineDefinition::new("libreoffice", "LibreOffice", &["soffice", "libreoffice"]),
    EngineDefinition::new("pandoc", "Pandoc", &["pandoc"]),
    EngineDefinition::new(
        "browser",
        "Navigateur PDF",
        &["google-chrome", "chromium", "msedge"],
    ),
    EngineDefinition::new("exiftool", "ExifTool", &["exiftool"]),
    EngineDefinition::new("sevenzip", "7-Zip", &["7zz", "7z"]),
    EngineDefinition::new("zstd", "Zstandard", &["zstd"]),
    EngineDefinition::new("lz4", "LZ4", &["lz4"]),
];

#[derive(Debug, Clone, Copy)]
pub struct EngineDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub commands: &'static [&'static str],
}

impl EngineDefinition {
    pub const fn new(
        id: &'static str,
        label: &'static str,
        commands: &'static [&'static str],
    ) -> Self {
        Self {
            id,
            label,
            commands,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SetupMode {
    Install,
    Repair,
    Uninstall,
    Doctor,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SetupProfile {
    Standard,
    ApplicationOnly,
    EnginesOnly,
    Custom,
    FullRemoval,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

impl Platform {
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "macos") {
            Some(Self::Macos)
        } else if cfg!(target_os = "windows") {
            Some(Self::Windows)
        } else if cfg!(target_os = "linux") {
            Some(Self::Linux)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Architecture {
    X86_64,
    Aarch64,
}

impl Architecture {
    pub fn current() -> Option<Self> {
        if cfg!(target_arch = "x86_64") {
            Some(Self::X86_64)
        } else if cfg!(target_arch = "aarch64") {
            Some(Self::Aarch64)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentKind {
    Application,
    Engine,
    Integration,
    Cache,
    Settings,
    History,
    Maintenance,
    Verification,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PlannedOperation {
    Inspect,
    Preserve,
    Download,
    Verify,
    Install,
    Repair,
    Remove,
    Stop,
    WriteReceipt,
    Finalize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineState {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub executable: Option<PathBuf>,
    pub version: Option<String>,
    pub installed_by_fileflow: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationState {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationState {
    pub launcher_installed: bool,
    pub icon_installed: bool,
    pub maintenance_installed: bool,
}

impl IntegrationState {
    pub const fn healthy(&self) -> bool {
        self.launcher_installed && self.icon_installed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSnapshot {
    pub platform: Platform,
    pub architecture: Architecture,
    pub application: ApplicationState,
    pub integration: IntegrationState,
    pub engines: Vec<EngineState>,
    pub receipt_path: PathBuf,
    pub receipt: Option<InstallReceipt>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupRequest {
    pub mode: SetupMode,
    pub profile: SetupProfile,
    #[serde(default)]
    pub selected_engines: Vec<String>,
    #[serde(default)]
    pub remove_owned_engines: bool,
    /// Mode expert: autorise le retrait de moteurs préexistants explicitement
    /// sélectionnés par l’utilisateur. Désactivé par défaut pour préserver les
    /// dépendances partagées avec d’autres applications.
    #[serde(default)]
    pub remove_preexisting_engines: bool,
    #[serde(default)]
    pub remove_settings: bool,
    #[serde(default)]
    pub remove_history: bool,
    #[serde(default)]
    pub remove_cache: bool,
    #[serde(default = "default_true")]
    pub preserve_outputs: bool,
    #[serde(default = "default_true")]
    pub launch_after: bool,
    #[serde(default)]
    pub dry_run: bool,
}

const fn default_true() -> bool {
    true
}

impl Default for SetupRequest {
    fn default() -> Self {
        Self {
            mode: SetupMode::Install,
            profile: SetupProfile::Standard,
            selected_engines: Vec::new(),
            remove_owned_engines: false,
            remove_preexisting_engines: false,
            remove_settings: false,
            remove_history: false,
            remove_cache: true,
            preserve_outputs: true,
            launch_after: true,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub id: String,
    pub title: String,
    pub description: String,
    pub component: ComponentKind,
    pub operation: PlannedOperation,
    pub weight: u32,
    pub interruptible: bool,
    pub requires_elevation: bool,
    pub rollback_description: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupPlan {
    pub operation_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub request: SetupRequest,
    pub platform: Platform,
    pub architecture: Architecture,
    pub steps: Vec<PlanStep>,
    pub warnings: Vec<String>,
    pub total_weight: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptComponent {
    pub id: String,
    pub kind: ComponentKind,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub installed_by_fileflow: bool,
    /// Gestionnaire réellement utilisé par FileFlow Setup (`apt`, `brew`,
    /// `winget`, `pipx`…). Absent dans les reçus antérieurs au schéma 2.
    #[serde(default)]
    pub package_manager: Option<String>,
    /// Paquets exacts ajoutés pour ce composant. Cette liste permet de retirer
    /// uniquement ce que FileFlow a effectivement installé.
    #[serde(default)]
    pub packages: Vec<String>,
    pub checksum: Option<String>,
    pub rollback_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReceipt {
    pub schema_version: u32,
    pub operation_id: Uuid,
    pub installed_at: DateTime<Utc>,
    pub application_version: String,
    pub platform: Platform,
    pub architecture: Architecture,
    pub components: Vec<ReceiptComponent>,
    pub outputs_are_user_owned: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EventLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupEvent {
    pub operation_id: Uuid,
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub level: EventLevel,
    pub step_id: Option<String>,
    pub message: String,
    pub completed: Option<u64>,
    pub total: Option<u64>,
    pub unit: Option<String>,
    #[serde(default)]
    pub detail: serde_json::Value,
}

impl SetupEvent {
    pub fn progress_percent(&self) -> Option<f64> {
        match (self.completed, self.total) {
            (Some(completed), Some(total)) if total > 0 => {
                Some((completed as f64 / total as f64 * 100.0).clamp(0.0, 100.0))
            }
            _ => None,
        }
    }
}
