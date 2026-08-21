use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use fileflow_engine::{EngineAdapter, EngineDescriptor};

pub struct Adapter;

#[async_trait]
impl EngineAdapter for Adapter {
    fn descriptor(&self) -> EngineDescriptor {
        EngineDescriptor {
            id: "ghostscript".to_string(),
            display_name: "Ghostscript".to_string(),
            executable_names: executable_names(),
            known_paths: Vec::new(),
            resource_profile: ResourceProfile::PDF,
        }
    }
}

fn executable_names() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        vec![
            "gswin64c".to_string(),
            "gswin32c".to_string(),
            "gs".to_string(),
        ]
    }
    #[cfg(not(target_os = "windows"))]
    {
        vec!["gs".to_string()]
    }
}
