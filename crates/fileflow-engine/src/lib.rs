use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    sync::OnceLock,
};

static BUNDLED_RUNTIME_ROOT: OnceLock<PathBuf> = OnceLock::new();

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeManifest {
    #[allow(dead_code)]
    version: u32,
    engines: HashMap<String, String>,
}

/// Register the packaged FileFlow runtime directory discovered by Tauri.
///
/// The runtime is immutable for the lifetime of the process. Returning `false`
/// means another root had already been registered (normally only possible in a
/// test harness or if setup is accidentally executed twice).
pub fn set_bundled_runtime_root(root: PathBuf) -> bool {
    BUNDLED_RUNTIME_ROOT.set(root).is_ok()
}

pub fn bundled_runtime_root() -> Option<&'static Path> {
    BUNDLED_RUNTIME_ROOT.get().map(PathBuf::as_path)
}

#[async_trait]
pub trait EngineAdapter: Send + Sync {
    fn descriptor(&self) -> EngineDescriptor;

    async fn probe(&self) -> Result<EngineProbe, EngineError> {
        let descriptor = self.descriptor();
        let executable = find_bundled_engine(&descriptor.id)
            .or_else(|| {
                descriptor
                    .known_paths
                    .iter()
                    .find(|path| is_executable_file(path))
                    .cloned()
            })
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

fn find_bundled_engine(engine_id: &str) -> Option<PathBuf> {
    bundled_runtime_root().and_then(|root| find_bundled_engine_in(root, engine_id))
}

fn find_bundled_engine_in(root: &Path, engine_id: &str) -> Option<PathBuf> {
    let manifest_path = root.join("runtime-manifest.json");
    let manifest: RuntimeManifest =
        serde_json::from_slice(&std::fs::read(manifest_path).ok()?).ok()?;
    let relative = Path::new(manifest.engines.get(engine_id)?);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return None;
    }

    let candidate = root.join(relative);
    if !is_executable_file(&candidate) {
        return None;
    }

    // Canonicalization prevents a malicious/accidental symlink in the runtime
    // manifest from escaping the packaged runtime directory.
    let canonical_root = root.canonicalize().ok()?;
    let canonical_candidate = candidate.canonicalize().ok()?;
    canonical_candidate
        .starts_with(&canonical_root)
        .then_some(canonical_candidate)
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

    // Optional user/installer search path. It deliberately wins over PATH but
    // not over an engine-specific override.
    if let Some(extra) = env::var_os("FILEFLOW_ENGINE_PATH")
        && let Some(found) = env::split_paths(&extra)
            .flat_map(|directory| variants.iter().map(move |name| directory.join(name)))
            .find(|candidate| is_executable_file(candidate))
    {
        return Some(found);
    }

    if let Some(path) = env::var_os("PATH")
        && let Some(found) = env::split_paths(&path)
            .filter(|directory| !is_appimage_internal_path(directory))
            .flat_map(|directory| variants.iter().map(move |name| directory.join(name)))
            .find(|candidate| is_executable_file(candidate))
    {
        return Some(found);
    }

    platform_search_directories()
        .into_iter()
        .filter(|directory| !is_appimage_internal_path(directory))
        .flat_map(|directory| variants.iter().map(move |name| directory.join(name)))
        .find(|candidate| is_executable_file(candidate))
}

#[cfg(target_os = "linux")]
fn is_appimage_internal_path(path: &Path) -> bool {
    env::var_os("APPDIR")
        .map(PathBuf::from)
        .is_some_and(|appdir| path.starts_with(appdir))
}

#[cfg(not(target_os = "linux"))]
fn is_appimage_internal_path(_path: &Path) -> bool {
    false
}

fn platform_search_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "fileflow-engine-{label}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }

    #[test]
    fn bundled_manifest_resolves_engine_inside_runtime() {
        let root = temp_dir("manifest");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/vips"), b"stub").unwrap();
        std::fs::write(
            root.join("runtime-manifest.json"),
            br#"{"version":1,"engines":{"vips":"bin/vips"}}"#,
        )
        .unwrap();

        let found = find_bundled_engine_in(&root, "vips").unwrap();
        assert_eq!(found, root.join("bin/vips").canonicalize().unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bundled_manifest_rejects_parent_traversal() {
        let root = temp_dir("traversal");
        std::fs::write(
            root.join("runtime-manifest.json"),
            br#"{"version":1,"engines":{"vips":"../vips"}}"#,
        )
        .unwrap();
        assert!(find_bundled_engine_in(&root, "vips").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}
