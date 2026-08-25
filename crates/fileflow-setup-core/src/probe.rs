use crate::{
    ApplicationState, Architecture, EngineState, FILEFLOW_ENGINES, InstallReceipt, Platform,
    SystemSnapshot,
};
use std::{env, fs, path::PathBuf, process::Command};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("plateforme non prise en charge")]
    UnsupportedPlatform,
    #[error("architecture non prise en charge")]
    UnsupportedArchitecture,
    #[error("lecture du reçu impossible: {0}")]
    Receipt(#[from] std::io::Error),
    #[error("reçu d’installation invalide: {0}")]
    InvalidReceipt(#[from] serde_json::Error),
}

pub fn probe_system() -> Result<SystemSnapshot, ProbeError> {
    let platform = Platform::current().ok_or(ProbeError::UnsupportedPlatform)?;
    let architecture = Architecture::current().ok_or(ProbeError::UnsupportedArchitecture)?;
    let receipt_path = receipt_path(platform);
    let receipt = read_receipt(&receipt_path)?;
    let application = probe_application(platform, receipt.as_ref());
    let engines = FILEFLOW_ENGINES
        .iter()
        .map(|definition| {
            let executable = definition
                .commands
                .iter()
                .find_map(|command| find_command(command));
            let installed_by_fileflow = receipt.as_ref().is_some_and(|value| {
                value.components.iter().any(|component| {
                    component.id == definition.id && component.installed_by_fileflow
                })
            });
            EngineState {
                id: definition.id.into(),
                label: definition.label.into(),
                installed: executable.is_some(),
                executable,
                version: None,
                installed_by_fileflow,
            }
        })
        .collect();

    Ok(SystemSnapshot {
        platform,
        architecture,
        application,
        engines,
        receipt_path,
        receipt,
        warnings: Vec::new(),
    })
}

pub fn receipt_path(platform: Platform) -> PathBuf {
    match platform {
        Platform::Macos => home_dir()
            .join("Library")
            .join("Application Support")
            .join("FileFlow")
            .join("install-receipt.json"),
        Platform::Linux => env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".local").join("share"))
            .join("fileflow")
            .join("install-receipt.json"),
        Platform::Windows => env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join("AppData").join("Local"))
            .join("FileFlow")
            .join("install-receipt.json"),
    }
}

pub fn write_receipt(path: &std::path::Path, receipt: &InstallReceipt) -> Result<(), ProbeError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.installing");
    fs::write(&temporary, serde_json::to_vec_pretty(receipt)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn read_receipt(path: &std::path::Path) -> Result<Option<InstallReceipt>, ProbeError> {
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
}

fn probe_application(platform: Platform, receipt: Option<&InstallReceipt>) -> ApplicationState {
    let candidates = application_candidates(platform);
    let path = candidates.into_iter().find(|candidate| candidate.exists());
    let version = receipt.map(|value| value.application_version.clone());
    ApplicationState {
        installed: path.is_some(),
        version,
        path,
        running: is_fileflow_running(platform),
    }
}

pub fn application_candidates(platform: Platform) -> Vec<PathBuf> {
    match platform {
        Platform::Macos => vec![
            PathBuf::from("/Applications/FileFlow.app"),
            home_dir().join("Applications").join("FileFlow.app"),
        ],
        Platform::Linux => vec![
            home_dir()
                .join(".local")
                .join("opt")
                .join("fileflow")
                .join("FileFlow.AppImage"),
        ],
        Platform::Windows => {
            let local = env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home_dir().join("AppData").join("Local"));
            let program_files = env::var_os("ProgramFiles")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
            vec![
                local.join("Programs").join("FileFlow").join("FileFlow.exe"),
                local
                    .join("Programs")
                    .join("FileFlow")
                    .join("fileflow-desktop.exe"),
                local.join("FileFlow").join("FileFlow.exe"),
                local.join("FileFlow").join("fileflow-desktop.exe"),
                program_files.join("FileFlow").join("FileFlow.exe"),
                program_files.join("FileFlow").join("fileflow-desktop.exe"),
            ]
        }
    }
}

fn home_dir() -> PathBuf {
    env::var_os(if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    })
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from("."))
}

fn find_command(command: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for directory in env::split_paths(&path) {
        let candidate = directory.join(executable_name(command));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    platform_command_candidates(command)
        .into_iter()
        .find(|candidate| candidate.is_file())
}

fn executable_name(command: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{command}.exe")
    } else {
        command.into()
    }
}

fn platform_command_candidates(command: &str) -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        let mut candidates = vec![
            PathBuf::from("/opt/homebrew/bin").join(command),
            PathBuf::from("/usr/local/bin").join(command),
        ];
        if matches!(command, "soffice" | "libreoffice") {
            candidates.push(PathBuf::from(
                "/Applications/LibreOffice.app/Contents/MacOS/soffice",
            ));
        }
        if matches!(command, "google-chrome" | "chromium") {
            candidates.push(PathBuf::from(
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            ));
        }
        candidates
    } else {
        Vec::new()
    }
}

fn is_fileflow_running(platform: Platform) -> bool {
    match platform {
        Platform::Windows => Command::new("tasklist.exe")
            .args(["/FO", "CSV", "/NH"])
            .output()
            .is_ok_and(|output| {
                output.status.success()
                    && windows_tasklist_contains_fileflow(&String::from_utf8_lossy(&output.stdout))
            }),
        Platform::Macos | Platform::Linux => Command::new("pgrep")
            .args(["-f", "fileflow-desktop"])
            .status()
            .is_ok_and(|status| status.success()),
    }
}

fn windows_tasklist_contains_fileflow(output: &str) -> bool {
    output.lines().any(|line| {
        let line = line.trim().trim_start_matches('\u{feff}').trim();
        let image_name = if let Some(rest) = line.strip_prefix('"') {
            rest.split('"').next().unwrap_or_default()
        } else {
            line.split(',').next().unwrap_or_default().trim_matches('"')
        };
        matches!(
            image_name.to_ascii_lowercase().as_str(),
            "fileflow.exe" | "fileflow-desktop.exe"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::windows_tasklist_contains_fileflow;

    #[test]
    fn windows_process_detection_is_locale_independent_and_ignores_setup() {
        assert!(!windows_tasklist_contains_fileflow(
            "INFO: Aucune tâche en cours ne correspond aux critères spécifiés."
        ));
        assert!(!windows_tasklist_contains_fileflow(
            "\"fileflow-setup.exe\",\"3500\",\"Console\",\"1\",\"42,000 K\""
        ));
        assert!(!windows_tasklist_contains_fileflow(
            "\"FileFlowSetupCLI_x86_64-pc-windows-msvc.exe\",\"3501\",\"Console\",\"1\",\"3,000 K\""
        ));
        assert!(windows_tasklist_contains_fileflow(
            "\"FileFlow.exe\",\"3510\",\"Console\",\"1\",\"120,000 K\""
        ));
        assert!(windows_tasklist_contains_fileflow(
            "\"fileflow-desktop.exe\",\"3511\",\"Console\",\"1\",\"120,000 K\""
        ));
    }
}
