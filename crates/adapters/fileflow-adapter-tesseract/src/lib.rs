use async_trait::async_trait;
use fileflow_domain::ResourceProfile;
use fileflow_engine::{EngineAdapter, EngineDescriptor};

pub struct Adapter;

#[async_trait]
impl EngineAdapter for Adapter {
    fn descriptor(&self) -> EngineDescriptor {
        EngineDescriptor {
            id: "tesseract".to_string(),
            display_name: "Tesseract".to_string(),
            executable_names: vec!["tesseract".to_string()],
            known_paths: Vec::new(),
            resource_profile: ResourceProfile::OCR,
        }
    }
}
