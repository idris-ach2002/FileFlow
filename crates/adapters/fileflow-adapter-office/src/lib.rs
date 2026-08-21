use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use fileflow_engine::{EngineAdapter, EngineDescriptor};
use std::path::PathBuf;

pub struct Adapter;

#[async_trait]
impl EngineAdapter for Adapter {
    fn descriptor(&self) -> EngineDescriptor {
        EngineDescriptor {
            id: "office".to_string(),
            display_name: "LibreOffice".to_string(),
            executable_names: vec!["libreoffice".to_string(), "soffice".to_string()],
            known_paths: known_paths(),
            resource_profile: ResourceProfile::OFFICE,
        }
    }
}

fn known_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from(
            "/Applications/LibreOffice.app/Contents/MacOS/soffice",
        ));
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join("Applications/LibreOffice.app/Contents/MacOS/soffice"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(variable) {
                let root = PathBuf::from(root);
                let candidate = if variable == "LOCALAPPDATA" {
                    root.join("Programs/LibreOffice/program/soffice.exe")
                } else {
                    root.join("LibreOffice/program/soffice.exe")
                };
                paths.push(candidate);
            }
        }
    }

    paths
}
