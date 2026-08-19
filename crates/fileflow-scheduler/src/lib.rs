use dashmap::DashMap;
use fileflow_domain::{PerformanceMode, ResourceProfile};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use sysinfo::System;
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceBudget {
    pub cpu_tokens: usize,
    pub memory_mb: u64,
    pub io_tokens: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerSettings {
    pub mode: PerformanceMode,
    pub custom_budget: Option<ResourceBudget>,
}

impl Default for SchedulerSettings {
    fn default() -> Self {
        Self {
            mode: PerformanceMode::Balanced,
            custom_budget: None,
        }
    }
}

impl ResourceBudget {
    pub fn detect(mode: PerformanceMode) -> Self {
        let logical = std::thread::available_parallelism().map_or(2, usize::from);
        let system = System::new_all();
        let total_memory_mb = (system.total_memory() / 1024 / 1024).max(1024);

        match mode {
            PerformanceMode::Eco => Self {
                cpu_tokens: (logical / 3).max(1),
                memory_mb: (total_memory_mb / 4).max(1024),
                io_tokens: 2,
            },
            PerformanceMode::Balanced => {
                let reserve = usize::from(logical >= 4) + usize::from(logical >= 8);
                Self {
                    cpu_tokens: logical.saturating_sub(reserve).max(1),
                    memory_mb: (total_memory_mb / 2).max(1536),
                    io_tokens: 4,
                }
            }
            PerformanceMode::Fast => Self {
                cpu_tokens: logical.max(1),
                memory_mb: (total_memory_mb.saturating_mul(3) / 4).max(2048),
                io_tokens: 6,
            },
            PerformanceMode::Custom => Self::detect(PerformanceMode::Balanced),
        }
    }

    pub fn balanced() -> Self {
        Self::detect(PerformanceMode::Balanced)
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerSnapshot {
    pub budget: ResourceBudget,
    pub cpu_available: usize,
    pub memory_mb_available: usize,
    pub io_available: usize,
}

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("resource request was cancelled")]
    Cancelled,
    #[error("scheduler semaphore was closed")]
    Closed,
}

pub struct ResourceLease {
    _cpu: OwnedSemaphorePermit,
    _memory: OwnedSemaphorePermit,
    _io: OwnedSemaphorePermit,
    _engine: OwnedSemaphorePermit,
}

pub struct ResourceScheduler {
    budget: ResourceBudget,
    cpu: Arc<Semaphore>,
    memory: Arc<Semaphore>,
    io: Arc<Semaphore>,
    engines: DashMap<String, Arc<Semaphore>>,
}

impl ResourceScheduler {
    pub fn new(settings: SchedulerSettings) -> Self {
        let budget = match (settings.mode, settings.custom_budget) {
            (PerformanceMode::Custom, Some(custom)) => sanitize_budget(custom),
            (mode, _) => ResourceBudget::detect(mode),
        };
        Self {
            budget,
            cpu: Arc::new(Semaphore::new(budget.cpu_tokens)),
            memory: Arc::new(Semaphore::new(memory_permits(budget.memory_mb))),
            io: Arc::new(Semaphore::new(budget.io_tokens)),
            engines: DashMap::new(),
        }
    }

    pub fn budget(&self) -> ResourceBudget {
        self.budget
    }

    pub fn snapshot(&self) -> SchedulerSnapshot {
        SchedulerSnapshot {
            budget: self.budget,
            cpu_available: self.cpu.available_permits(),
            memory_mb_available: self.memory.available_permits(),
            io_available: self.io.available_permits(),
        }
    }

    pub async fn acquire(
        &self,
        engine_id: &str,
        profile: ResourceProfile,
        cancellation: &CancellationToken,
    ) -> Result<ResourceLease, SchedulerError> {
        let cpu_needed = usize::from(profile.cpu_weight)
            .clamp(1, self.budget.cpu_tokens)
            .min(u32::MAX as usize) as u32;
        let memory_needed = usize::try_from(u64::from(profile.memory_mb).min(self.budget.memory_mb))
            .unwrap_or(usize::MAX)
            .max(1)
            .min(u32::MAX as usize) as u32;
        let io_needed = usize::from(profile.io_weight)
            .clamp(1, self.budget.io_tokens)
            .min(u32::MAX as usize) as u32;

        let engine = self
            .engines
            .entry(engine_id.to_owned())
            .or_insert_with(|| Arc::new(Semaphore::new(profile.max_parallel_instances.max(1))))
            .clone();

        let cpu = acquire_many(self.cpu.clone(), cpu_needed, cancellation).await?;
        let memory = acquire_many(self.memory.clone(), memory_needed, cancellation).await?;
        let io = acquire_many(self.io.clone(), io_needed, cancellation).await?;
        let engine = acquire_many(engine, 1, cancellation).await?;

        Ok(ResourceLease {
            _cpu: cpu,
            _memory: memory,
            _io: io,
            _engine: engine,
        })
    }
}

impl Default for ResourceScheduler {
    fn default() -> Self {
        Self::new(SchedulerSettings::default())
    }
}

fn sanitize_budget(budget: ResourceBudget) -> ResourceBudget {
    ResourceBudget {
        cpu_tokens: budget.cpu_tokens.max(1),
        memory_mb: budget.memory_mb.max(256),
        io_tokens: budget.io_tokens.max(1),
    }
}

fn memory_permits(memory_mb: u64) -> usize {
    usize::try_from(memory_mb.min(usize::MAX as u64))
        .unwrap_or(usize::MAX)
        .max(1)
}

async fn acquire_many(
    semaphore: Arc<Semaphore>,
    permits: u32,
    cancellation: &CancellationToken,
) -> Result<OwnedSemaphorePermit, SchedulerError> {
    tokio::select! {
        _ = cancellation.cancelled() => Err(SchedulerError::Cancelled),
        permit = semaphore.acquire_many_owned(permits) => permit.map_err(|_| SchedulerError::Closed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reserves_and_releases_resources() {
        let scheduler = ResourceScheduler::new(SchedulerSettings {
            mode: PerformanceMode::Custom,
            custom_budget: Some(ResourceBudget {
                cpu_tokens: 4,
                memory_mb: 1024,
                io_tokens: 4,
            }),
        });
        let cancellation = CancellationToken::new();
        let profile = ResourceProfile {
            cpu_weight: 2,
            memory_mb: 256,
            io_weight: 1,
            internally_threaded: false,
            max_parallel_instances: 1,
        };

        let before = scheduler.snapshot();
        let lease = scheduler
            .acquire("test", profile, &cancellation)
            .await
            .unwrap();
        let during = scheduler.snapshot();
        assert_eq!(during.cpu_available, before.cpu_available - 2);
        assert_eq!(during.memory_mb_available, before.memory_mb_available - 256);
        drop(lease);
        assert_eq!(scheduler.snapshot().cpu_available, before.cpu_available);
    }

    #[tokio::test]
    async fn cancellation_stops_waiting_for_resources() {
        let scheduler = ResourceScheduler::new(SchedulerSettings {
            mode: PerformanceMode::Custom,
            custom_budget: Some(ResourceBudget {
                cpu_tokens: 1,
                memory_mb: 256,
                io_tokens: 1,
            }),
        });
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let result = scheduler
            .acquire("test", ResourceProfile::LIGHT, &cancellation)
            .await;
        assert!(matches!(result, Err(SchedulerError::Cancelled)));
    }
}
