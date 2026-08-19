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
            executable_names: vec!["gs".to_string()],
            known_paths: Vec::new(),
            resource_profile: ResourceProfile::PDF,
        }
    }
}
