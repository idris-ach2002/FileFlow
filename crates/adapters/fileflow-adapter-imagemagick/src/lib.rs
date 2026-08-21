use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use fileflow_engine::{EngineAdapter, EngineDescriptor};

pub struct Adapter;

#[async_trait]
impl EngineAdapter for Adapter {
    fn descriptor(&self) -> EngineDescriptor {
        EngineDescriptor {
            id: "imagemagick".to_string(),
            display_name: "ImageMagick".to_string(),
            executable_names: vec!["magick".to_string(), "convert".to_string()],
            known_paths: Vec::new(),
            resource_profile: ResourceProfile::IMAGE,
        }
    }
}
