use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use fileflow_engine::{EngineAdapter, EngineDescriptor};

pub struct Adapter;

#[async_trait]
impl EngineAdapter for Adapter {
    fn descriptor(&self) -> EngineDescriptor {
        EngineDescriptor {
            id: "metadata".to_string(),
            display_name: "ExifTool".to_string(),
            known_paths: Vec::new(),
            executable_names: vec!["exiftool".to_string()],
            resource_profile: ResourceProfile::LIGHT,
        }
    }
}
