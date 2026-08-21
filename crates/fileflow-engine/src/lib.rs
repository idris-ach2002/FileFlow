use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::{Path, PathBuf},
};

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

    // Explicit per-engine override. Example: FILEFLOW_FFMPEG_PATH=/custom/ffmpeg.
    let env_key = format!(
        "FILEFLOW_{}_PATH",
        executable
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            })
            .collect::<String>()
    );
    if let Some(path) = env::var_os(env_key)
        && is_executable_file(Path::new(&path))
    {
        return Some(PathBuf::from(path));
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

    // Optional extra search path controlled by the installer/user. This is a
    // directory list, not an executable, and never changes the global process PATH.
    if let Some(extra) = env::var_os("FILEFLOW_ENGINE_PATH") {
        directories.extend(env::split_paths(&extra));
    }

    #[cfg(target_os = "macos")]
    {
        // Finder/Dock applications do not inherit an interactive shell PATH.
        directories.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ]);
        if let Some(home) = env::var_os("HOME") {
            let home = PathBuf::from(home);
            directories.push(home.join(".local/bin"));
            let python_root = home.join("Library/Python");
            if let Ok(entries) = std::fs::read_dir(python_root) {
                directories.extend(
                    entries
                        .flatten()
                        .map(|entry| entry.path().join("bin"))
                        .filter(|path| path.is_dir()),
                );
            }
        }
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

    #[cfg(target_os = "windows")]
    {
        if let Some(local) = env::var_os("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            directories.extend([
                local.join("Microsoft/WinGet/Links"),
                local.join("Microsoft/WindowsApps"),
                local.join("Programs/Python/Python312/Scripts"),
            ]);
            add_windows_install_subdirectories(&mut directories, &local.join("Programs"));
        }
        if let Some(profile) = env::var_os("USERPROFILE") {
            directories.push(PathBuf::from(profile).join("scoop/shims"));
        }
        if let Some(choco) = env::var_os("ChocolateyInstall") {
            directories.push(PathBuf::from(choco).join("bin"));
        }
        if let Some(appdata) = env::var_os("APPDATA") {
            directories.push(PathBuf::from(appdata).join("Python/Scripts"));
        }
        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = env::var_os(variable) {
                add_windows_install_subdirectories(&mut directories, &PathBuf::from(root));
            }
        }
    }

    directories
}

#[cfg(target_os = "windows")]
fn add_windows_install_subdirectories(directories: &mut Vec<PathBuf>, root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        directories.push(path.clone());
        directories.push(path.join("bin"));
        directories.push(path.join("program"));

        // Ghostscript and a few portable tools add a version directory between
        // the product root and bin/. One extra level is enough and avoids a
        // recursive filesystem crawl on every engine probe.
        if let Ok(children) = std::fs::read_dir(&path) {
            for child in children.flatten() {
                let child = child.path();
                if child.is_dir() {
                    directories.push(child.join("bin"));
                }
            }
        }
    }
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
