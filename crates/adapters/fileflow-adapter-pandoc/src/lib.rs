use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use fileflow_engine::{EngineAdapter, EngineDescriptor};

pub struct Adapter;

#[async_trait]
impl EngineAdapter for Adapter {
    fn descriptor(&self) -> EngineDescriptor {
        EngineDescriptor {
            id: "pandoc".to_string(),
            display_name: "Pandoc".to_string(),
            executable_names: vec!["pandoc".to_string()],
            known_paths: Vec::new(),
            resource_profile: ResourceProfile::LIGHT,
        }
    }
}
