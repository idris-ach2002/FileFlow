use fileflow_domain::{Asset, WorkspaceId};
use fileflow_engine::{EngineAdapter, EngineProbe};
use fileflow_intake::{IntakeEvent, IntakeScanner, IntakeStats, IntakeWarning, ScanOptions};
use fileflow_workspace::{AssetPage, AssetQuery, WorkspaceManager, WorkspaceSnapshot};
use parking_lot::RwLock;
use serde::Serialize;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use thiserror::Error;
use tokio::sync::mpsc;

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "event", content = "data")]
pub enum WorkspaceIntakeEvent {
    Started {
        workspace_id: WorkspaceId,
        roots: usize,
    },
    Batch {
        workspace_id: WorkspaceId,
        assets: Vec<Asset>,
        stats: IntakeStats,
    },
    Progress {
        workspace_id: WorkspaceId,
        stats: IntakeStats,
    },
    Warning {
        workspace_id: WorkspaceId,
        warning: IntakeWarning,
        stats: IntakeStats,
    },
    Finished {
        workspace: WorkspaceSnapshot,
    },
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error(transparent)]
    Intake(#[from] fileflow_intake::IntakeError),
    #[error(transparent)]
    Workspace(#[from] fileflow_workspace::WorkspaceError),
    #[error("intake worker failed: {0}")]
    IntakeTask(String),
    #[error("workspace event consumer disconnected")]
    EventConsumerDisconnected,
}

pub struct FileFlowCore {
    pub engines: EngineRegistry,
    pub workspaces: WorkspaceManager,
    intake: IntakeScanner,
}

impl Default for FileFlowCore {
    fn default() -> Self {
        Self {
            engines: EngineRegistry::default(),
            workspaces: WorkspaceManager::default(),
            intake: IntakeScanner::default(),
        }
    }
}

impl FileFlowCore {
    pub async fn create_workspace(
        &self,
        paths: Vec<PathBuf>,
        options: ScanOptions,
        events: mpsc::Sender<WorkspaceIntakeEvent>,
    ) -> Result<WorkspaceSnapshot, CoreError> {
        let workspace = self.workspaces.create(paths.clone());
        let workspace_id = workspace.id;
        let (intake_tx, mut intake_rx) = mpsc::channel(8);
        let scanner = self.intake.clone();

        events
            .send(WorkspaceIntakeEvent::Started {
                workspace_id,
                roots: paths.len(),
            })
            .await
            .map_err(|_| CoreError::EventConsumerDisconnected)?;

        let scan_task = tokio::spawn(async move { scanner.scan(paths, options, intake_tx).await });

        while let Some(event) = intake_rx.recv().await {
            match event {
                IntakeEvent::Started { .. } => {}
                IntakeEvent::Batch { assets, stats, .. } => {
                    self.workspaces.ingest(workspace_id, &assets)?;
                    events
                        .send(WorkspaceIntakeEvent::Batch {
                            workspace_id,
                            assets,
                            stats,
                        })
                        .await
                        .map_err(|_| CoreError::EventConsumerDisconnected)?;
                }
                IntakeEvent::Progress { stats, .. } => {
                    events
                        .send(WorkspaceIntakeEvent::Progress {
                            workspace_id,
                            stats,
                        })
                        .await
                        .map_err(|_| CoreError::EventConsumerDisconnected)?;
                }
                IntakeEvent::Warning { warning, stats, .. } => {
                    events
                        .send(WorkspaceIntakeEvent::Warning {
                            workspace_id,
                            warning,
                            stats,
                        })
                        .await
                        .map_err(|_| CoreError::EventConsumerDisconnected)?;
                }
                IntakeEvent::Finished { .. } => {}
            }
        }

        match scan_task.await {
            Ok(Ok(_)) => {
                let snapshot = self.workspaces.mark_ready(workspace_id)?;
                events
                    .send(WorkspaceIntakeEvent::Finished {
                        workspace: snapshot.clone(),
                    })
                    .await
                    .map_err(|_| CoreError::EventConsumerDisconnected)?;
                Ok(snapshot)
            }
            Ok(Err(error)) => {
                let message = error.to_string();
                let _ = self.workspaces.mark_failed(workspace_id, &message);
                Err(CoreError::Intake(error))
            }
            Err(error) => {
                let message = error.to_string();
                let _ = self.workspaces.mark_failed(workspace_id, &message);
                Err(CoreError::IntakeTask(message))
            }
        }
    }

    pub fn workspace(&self, id: WorkspaceId) -> Result<WorkspaceSnapshot, CoreError> {
        Ok(self.workspaces.snapshot(id)?)
    }

    pub fn list_workspace_assets(
        &self,
        id: WorkspaceId,
        query: AssetQuery,
    ) -> Result<AssetPage, CoreError> {
        Ok(self.workspaces.list_assets(id, query)?)
    }
}
