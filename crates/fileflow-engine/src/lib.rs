use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::{Path, PathBuf},
};

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
    let candidate = Path::new(executable);
    if candidate.components().count() > 1 && is_executable_file(candidate) {
        return Some(candidate.to_path_buf());
    }

    if let Some(path) = env::var_os("PATH") {
        if let Some(found) = env::split_paths(&path)
            .map(|directory| directory.join(executable))
            .find(|candidate| is_executable_file(candidate))
        {
            return Some(found);
        }
    }

    platform_search_directories()
        .into_iter()
        .map(|directory| directory.join(executable))
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
