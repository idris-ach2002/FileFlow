use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use fileflow_engine::{EngineAdapter, EngineDescriptor};
#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn known_paths() -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from(
        "/Applications/LibreOffice.app/Contents/MacOS/soffice",
    )];

    if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(home).join("Applications/LibreOffice.app/Contents/MacOS/soffice"));
    }

    paths
}

#[cfg(not(target_os = "macos"))]
fn known_paths() -> Vec<std::path::PathBuf> {
    Vec::new()
}
