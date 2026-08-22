use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use fileflow_engine::{EngineAdapter, EngineDescriptor};
use std::path::PathBuf;

pub struct Adapter;

#[async_trait]
impl EngineAdapter for Adapter {
    fn descriptor(&self) -> EngineDescriptor {
        EngineDescriptor {
            id: "browser".to_string(),
            display_name: "Navigateur sécurisé (Chromium)".to_string(),
            executable_names: vec![
                "google-chrome".to_string(),
                "google-chrome-stable".to_string(),
                "chromium".to_string(),
                "chromium-browser".to_string(),
                "microsoft-edge".to_string(),
                "msedge".to_string(),
                "chrome".to_string(),
            ],
            known_paths: known_browser_paths(),
            resource_profile: ResourceProfile::OFFICE,
        }
    }
}

fn known_browser_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        paths.extend([
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
            PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
        ]);
        if let Some(home) = std::env::var_os("HOME") {
            let applications = PathBuf::from(home).join("Applications");
            paths.extend([
                applications.join("Google Chrome.app/Contents/MacOS/Google Chrome"),
                applications.join("Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
                applications.join("Chromium.app/Contents/MacOS/Chromium"),
            ]);
        }
    }

    #[cfg(target_os = "windows")]
    {
        for variable in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(variable) {
                let root = PathBuf::from(root);
                paths.extend([
                    root.join("Google/Chrome/Application/chrome.exe"),
                    root.join("Microsoft/Edge/Application/msedge.exe"),
                    root.join("Chromium/Application/chrome.exe"),
                ]);
            }
        }
    }

    #[cfg(target_os = "linux")]
    paths.extend([
        PathBuf::from("/usr/bin/google-chrome"),
        PathBuf::from("/usr/bin/google-chrome-stable"),
        PathBuf::from("/usr/bin/chromium"),
        PathBuf::from("/usr/bin/chromium-browser"),
        PathBuf::from("/usr/bin/microsoft-edge"),
        PathBuf::from("/opt/google/chrome/chrome"),
        PathBuf::from("/opt/microsoft/msedge/msedge"),
    ]);

    paths
}
