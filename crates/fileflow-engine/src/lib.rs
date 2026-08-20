use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::{Path, PathBuf},
    sync::{OnceLock, RwLock},
};

static BUNDLED_ENGINE_ROOT: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

pub fn set_bundled_engine_root(root: PathBuf) {
    let slot = BUNDLED_ENGINE_ROOT.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(root);
    }
}

fn bundled_engine_root() -> Option<PathBuf> {
    BUNDLED_ENGINE_ROOT
        .get()
        .and_then(|slot| slot.read().ok())
        .and_then(|guard| guard.as_ref().cloned())
}

fn executable_variants(executable: &str) -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        if executable.to_ascii_lowercase().ends_with(".exe") {
            vec![executable.to_owned()]
        } else {
            vec![executable.to_owned(), format!("{executable}.exe")]
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        vec![executable.to_owned()]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineDescriptor {
    pub id: String,
    pub display_name: String,
    pub executable_names: Vec<String>,
    #[serde(default)]
    pub known_paths: Vec<PathBuf>,
    pub resource_profile: ResourceProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProbe {
    pub id: String,
    pub display_name: String,
    pub available: bool,
    pub executable: Option<PathBuf>,
    pub resource_profile: ResourceProfile,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("engine executable is not available: {0}")]
    Unavailable(String),
    #[error("engine error: {0}")]
    Other(String),
}

#[async_trait]
pub trait EngineAdapter: Send + Sync {
    fn descriptor(&self) -> EngineDescriptor;

    async fn probe(&self) -> Result<EngineProbe, EngineError> {
        let descriptor = self.descriptor();
        let executable = descriptor
            .known_paths
            .iter()
            .find(|path| is_executable_file(path))
            .cloned()
            .or_else(|| {
                descriptor
                    .executable_names
                    .iter()
                    .find_map(|name| find_executable(name))
            });

        Ok(EngineProbe {
            id: descriptor.id,
            display_name: descriptor.display_name,
            available: executable.is_some(),
            executable,
            resource_profile: descriptor.resource_profile,
        })
    }
}

pub fn find_executable(executable: &str) -> Option<PathBuf> {
    let variants = executable_variants(executable);
    let candidate = Path::new(executable);
    if candidate.components().count() > 1
        && let Some(found) = variants
            .iter()
            .map(PathBuf::from)
            .find(|path| is_executable_file(path))
    {
        return Some(found);
    }

    // Packaged engines always win so a release behaves identically regardless
    // of the user's shell PATH or package manager configuration.
    if let Some(root) = bundled_engine_root()
        && let Some(found) = variants
            .iter()
            .map(|name| root.join(name))
            .find(|candidate| is_executable_file(candidate))
    {
        return Some(found);
    }

    if let Some(path) = env::var_os("PATH")
        && let Some(found) = env::split_paths(&path)
            .flat_map(|directory| variants.iter().map(move |name| directory.join(name)))
            .find(|candidate| is_executable_file(candidate))
    {
        return Some(found);
    }

    platform_search_directories()
        .into_iter()
        .flat_map(|directory| variants.iter().map(move |name| directory.join(name)))
        .find(|candidate| is_executable_file(candidate))
}

fn platform_search_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();

    #[cfg(target_os = "macos")]
    {
        // Finder/Dock launched GUI applications do not necessarily inherit the
        // interactive shell PATH. Cover the standard Apple Silicon and Intel
        // Homebrew prefixes explicitly.
        directories.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ]);
    }

    #[cfg(target_os = "linux")]
    {
        directories.extend([
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/snap/bin"),
            PathBuf::from("/home/linuxbrew/.linuxbrew/bin"),
        ]);

        if let Some(home) = env::var_os("HOME") {
            directories.push(PathBuf::from(home).join(".local/bin"));
        }
    }

    directories
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
