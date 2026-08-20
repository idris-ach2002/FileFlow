use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use fileflow_engine::{EngineAdapter, EngineDescriptor};

pub struct Adapter;

#[async_trait]
impl EngineAdapter for Adapter {
    fn descriptor(&self) -> EngineDescriptor {
        EngineDescriptor {
            id: "lz4".to_string(),
            display_name: "LZ4".to_string(),
            known_paths: Vec::new(),
            executable_names: vec!["lz4".to_string()],
            resource_profile: ResourceProfile::ARCHIVE,
        }
    }
}
