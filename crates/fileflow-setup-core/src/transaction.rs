use crate::{EventLevel, InstallReceipt, PlanStep, SetupEvent, SetupPlan};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use thiserror::Error;
use tokio::fs;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum TransactionError {
    #[error("opération annulée")]
    Cancelled,
    #[error("l’étape {step} a échoué: {message}")]
    Step { step: String, message: String },
    #[error("journal transactionnel inaccessible: {0}")]
    Journal(#[from] std::io::Error),
    #[error("journal transactionnel invalide: {0}")]
    JournalFormat(#[from] serde_json::Error),
}

pub trait EventSink: Send + Sync {
    fn emit(&self, event: SetupEvent);
}

impl<F> EventSink for F
where
    F: Fn(SetupEvent) + Send + Sync,
{
    fn emit(&self, event: SetupEvent) {
        self(event);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JournalStepState {
    Pending,
    Running,
    Completed,
    Failed,
    RolledBack,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalStep {
    pub id: String,
    pub state: JournalStepState,
    pub started_at: Option<chrono::DateTime<Utc>>,
    pub finished_at: Option<chrono::DateTime<Utc>>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionJournal {
    pub schema_version: u32,
    pub operation_id: Uuid,
    pub created_at: chrono::DateTime<Utc>,
    pub finished_at: Option<chrono::DateTime<Utc>>,
    pub succeeded: bool,
    pub steps: Vec<JournalStep>,
}

impl TransactionJournal {
    pub fn for_plan(plan: &SetupPlan) -> Self {
        Self {
            schema_version: 1,
            operation_id: plan.operation_id,
            created_at: Utc::now(),
            finished_at: None,
            succeeded: false,
            steps: plan
                .steps
                .iter()
                .map(|step| JournalStep {
                    id: step.id.clone(),
                    state: JournalStepState::Pending,
                    started_at: None,
                    finished_at: None,
                    message: None,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Default)]
pub struct ActionOutcome {
    pub message: Option<String>,
    pub receipt: Option<InstallReceipt>,
}

#[async_trait]
pub trait SetupActionAdapter: Send + Sync {
    async fn apply(
        &self,
        plan: &SetupPlan,
        step: &PlanStep,
        events: &dyn EventSink,
        cancellation: &AtomicBool,
    ) -> Result<ActionOutcome, String>;

    async fn rollback(
        &self,
        _plan: &SetupPlan,
        _step: &PlanStep,
        _events: &dyn EventSink,
    ) -> Result<(), String> {
        Ok(())
    }
}

pub struct TransactionEngine {
    journal_path: PathBuf,
    sequence: AtomicU64,
}

impl TransactionEngine {
    pub fn new(journal_path: impl Into<PathBuf>) -> Self {
        Self {
            journal_path: journal_path.into(),
            sequence: AtomicU64::new(0),
        }
    }

    pub async fn execute<A: SetupActionAdapter>(
        &self,
        plan: &SetupPlan,
        adapter: &A,
        events: &dyn EventSink,
        cancellation: Arc<AtomicBool>,
    ) -> Result<TransactionJournal, TransactionError> {
        let mut journal = TransactionJournal::for_plan(plan);
        self.persist(&journal).await?;
        self.event(
            events,
            plan.operation_id,
            "operation-started",
            EventLevel::Info,
            None,
            "Opération FileFlow démarrée",
        );

        let mut completed = Vec::new();
        for (index, step) in plan.steps.iter().enumerate() {
            if cancellation.load(Ordering::Relaxed) && step.interruptible {
                journal.steps[index].state = JournalStepState::Skipped;
                self.persist(&journal).await?;
                self.rollback(plan, adapter, events, &mut journal, &completed)
                    .await?;
                self.event(
                    events,
                    plan.operation_id,
                    "operation-cancelled",
                    EventLevel::Warning,
                    Some(&step.id),
                    "Opération annulée à une frontière sûre",
                );
                return Err(TransactionError::Cancelled);
            }

            journal.steps[index].state = JournalStepState::Running;
            journal.steps[index].started_at = Some(Utc::now());
            self.persist(&journal).await?;
            self.event(
                events,
                plan.operation_id,
                "step-started",
                EventLevel::Info,
                Some(&step.id),
                &step.title,
            );

            match adapter
                .apply(plan, step, events, cancellation.as_ref())
                .await
            {
                Ok(outcome) => {
                    journal.steps[index].state = JournalStepState::Completed;
                    journal.steps[index].finished_at = Some(Utc::now());
                    journal.steps[index].message = outcome.message;
                    completed.push(index);
                    self.persist(&journal).await?;
                    self.event(
                        events,
                        plan.operation_id,
                        "step-completed",
                        EventLevel::Success,
                        Some(&step.id),
                        &format!("{} terminé", step.title),
                    );
                }
                Err(message) => {
                    journal.steps[index].state = JournalStepState::Failed;
                    journal.steps[index].finished_at = Some(Utc::now());
                    journal.steps[index].message = Some(message.clone());
                    self.persist(&journal).await?;
                    self.event(
                        events,
                        plan.operation_id,
                        "step-failed",
                        EventLevel::Error,
                        Some(&step.id),
                        &message,
                    );
                    self.rollback(plan, adapter, events, &mut journal, &completed)
                        .await?;
                    return Err(TransactionError::Step {
                        step: step.id.clone(),
                        message,
                    });
                }
            }
        }

        journal.finished_at = Some(Utc::now());
        journal.succeeded = true;
        self.persist(&journal).await?;
        self.event(
            events,
            plan.operation_id,
            "operation-completed",
            EventLevel::Success,
            None,
            "Opération FileFlow terminée et vérifiée",
        );
        Ok(journal)
    }

    async fn rollback<A: SetupActionAdapter>(
        &self,
        plan: &SetupPlan,
        adapter: &A,
        events: &dyn EventSink,
        journal: &mut TransactionJournal,
        completed: &[usize],
    ) -> Result<(), TransactionError> {
        self.event(
            events,
            plan.operation_id,
            "rollback-started",
            EventLevel::Warning,
            None,
            "Restauration de l’état précédent",
        );
        for &index in completed.iter().rev() {
            let step = &plan.steps[index];
            match adapter.rollback(plan, step, events).await {
                Ok(()) => journal.steps[index].state = JournalStepState::RolledBack,
                Err(message) => {
                    journal.steps[index].message = Some(format!("rollback: {message}"));
                }
            }
            self.persist(journal).await?;
        }
        Ok(())
    }

    async fn persist(&self, journal: &TransactionJournal) -> Result<(), TransactionError> {
        if let Some(parent) = self.journal_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let temporary = self.journal_path.with_extension("json.writing");
        fs::write(&temporary, serde_json::to_vec_pretty(journal)?).await?;
        fs::rename(temporary, &self.journal_path).await?;
        Ok(())
    }

    fn event(
        &self,
        events: &dyn EventSink,
        operation_id: Uuid,
        event_type: &str,
        level: EventLevel,
        step_id: Option<&str>,
        message: &str,
    ) {
        events.emit(SetupEvent {
            operation_id,
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            timestamp: Utc::now(),
            event_type: event_type.into(),
            level,
            step_id: step_id.map(str::to_owned),
            message: message.into(),
            completed: None,
            total: None,
            unit: None,
            detail: serde_json::Value::Null,
        });
    }

    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ApplicationState, Architecture, Platform, SetupRequest, SystemSnapshot, build_plan,
    };
    use std::sync::Mutex;

    struct TestAdapter {
        fail: Option<String>,
        applied: Mutex<Vec<String>>,
        rolled_back: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl SetupActionAdapter for TestAdapter {
        async fn apply(
            &self,
            _plan: &SetupPlan,
            step: &PlanStep,
            _events: &dyn EventSink,
            _cancellation: &AtomicBool,
        ) -> Result<ActionOutcome, String> {
            self.applied.lock().unwrap().push(step.id.clone());
            if self.fail.as_ref() == Some(&step.id) {
                Err("synthetic failure".into())
            } else {
                Ok(ActionOutcome::default())
            }
        }

        async fn rollback(
            &self,
            _plan: &SetupPlan,
            step: &PlanStep,
            _events: &dyn EventSink,
        ) -> Result<(), String> {
            self.rolled_back.lock().unwrap().push(step.id.clone());
            Ok(())
        }
    }

    fn plan() -> SetupPlan {
        build_plan(
            &SystemSnapshot {
                platform: Platform::Linux,
                architecture: Architecture::X86_64,
                application: ApplicationState {
                    installed: false,
                    version: None,
                    path: None,
                    running: false,
                },
                engines: vec![],
                receipt_path: PathBuf::from("receipt.json"),
                receipt: None,
                warnings: vec![],
            },
            SetupRequest::default(),
        )
    }

    #[tokio::test]
    async fn rolls_back_completed_steps_after_failure() {
        let temp = std::env::temp_dir().join(format!("fileflow-setup-{}.json", Uuid::new_v4()));
        let engine = TransactionEngine::new(&temp);
        let adapter = TestAdapter {
            fail: Some("application".into()),
            applied: Mutex::new(vec![]),
            rolled_back: Mutex::new(vec![]),
        };
        let events = Mutex::new(Vec::<SetupEvent>::new());
        let sink = |event| events.lock().unwrap().push(event);
        let result = engine
            .execute(&plan(), &adapter, &sink, Arc::new(AtomicBool::new(false)))
            .await;
        assert!(result.is_err());
        assert_eq!(
            adapter.rolled_back.lock().unwrap().as_slice(),
            &["release", "preflight"]
        );
        let _ = std::fs::remove_file(temp);
    }
}
