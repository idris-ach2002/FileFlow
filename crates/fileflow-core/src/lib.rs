use fileflow_engine::{EngineAdapter, EngineProbe};
use parking_lot::RwLock;
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub struct EngineRegistry {
    adapters: RwLock<HashMap<String, Arc<dyn EngineAdapter>>>,
}

impl EngineRegistry {
    pub fn register(&self, adapter: Arc<dyn EngineAdapter>) {
        let id = adapter.descriptor().id;
        self.adapters.write().insert(id, adapter);
    }

    pub async fn probe_all(&self) -> Vec<EngineProbe> {
        let adapters: Vec<_> = self.adapters.read().values().cloned().collect();
        let mut probes = Vec::with_capacity(adapters.len());

        for adapter in adapters {
            match adapter.probe().await {
                Ok(probe) => probes.push(probe),
                Err(error) => {
                    tracing::warn!(engine = %adapter.descriptor().id, %error, "engine probe failed");
                }
            }
        }

        probes.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        probes
    }
}

#[derive(Default)]
pub struct FileFlowCore {
    pub engines: EngineRegistry,
}
