use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use fileflow_engine::{EngineAdapter, EngineDescriptor};

pub struct Adapter;

#[async_trait]
impl EngineAdapter for Adapter {
    fn descriptor(&self) -> EngineDescriptor {
        EngineDescriptor {
            id: "vips".to_string(),
            display_name: "libvips".to_string(),
            known_paths: Vec::new(),
            executable_names: vec!["vips".to_string()],
            resource_profile: ResourceProfile::IMAGE,
        }
    }
}
