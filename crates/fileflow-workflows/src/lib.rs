//! Workflow/DAG model shared by desktop automation, persistence and tests.
//!
//! The crate intentionally performs no filesystem or process work. It validates
//! a declarative graph and provides a deterministic topological execution order.

use fileflow_domain::OutputPolicy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowDefinition {
    #[serde(default = "default_version")]
    pub version: u16,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStep {
    pub id: String,
    pub action_id: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub target_format: Option<String>,
    pub quality: Option<String>,
    #[serde(default)]
    pub parameters: HashMap<String, Value>,
    #[serde(default)]
    pub output_policy: OutputPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowPlan {
    pub order: Vec<String>,
    pub roots: Vec<String>,
    pub leaves: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowProgress {
    pub job_id: Uuid,
    pub recipe_id: Option<Uuid>,
    pub status: String,
    pub current_step: usize,
    pub total_steps: usize,
    pub completed_steps: Vec<String>,
    pub failed_step: Option<String>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WorkflowError {
    #[error("Le workflow ne contient aucune étape.")]
    Empty,
    #[error("L’identifiant d’étape « {0} » est vide ou invalide.")]
    InvalidId(String),
    #[error("L’étape « {0} » est déclarée plusieurs fois.")]
    DuplicateStep(String),
    #[error("L’étape « {step} » dépend d’une étape inconnue « {dependency} ».")]
    UnknownDependency { step: String, dependency: String },
    #[error("L’étape « {0} » dépend d’elle-même.")]
    SelfDependency(String),
    #[error("Le workflow contient un cycle de dépendances.")]
    Cycle,
}

impl WorkflowDefinition {
    pub fn validate(&self) -> Result<WorkflowPlan, WorkflowError> {
        if self.steps.is_empty() {
            return Err(WorkflowError::Empty);
        }

        let mut ids = HashSet::with_capacity(self.steps.len());
        for step in &self.steps {
            let id = step.id.trim();
            if id.is_empty()
                || !id
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
            {
                return Err(WorkflowError::InvalidId(step.id.clone()));
            }
            if !ids.insert(step.id.clone()) {
                return Err(WorkflowError::DuplicateStep(step.id.clone()));
            }
        }

        let mut incoming = HashMap::<String, usize>::new();
        let mut outgoing = HashMap::<String, Vec<String>>::new();
        for step in &self.steps {
            incoming.insert(step.id.clone(), step.depends_on.len());
            for dependency in &step.depends_on {
                if dependency == &step.id {
                    return Err(WorkflowError::SelfDependency(step.id.clone()));
                }
                if !ids.contains(dependency) {
                    return Err(WorkflowError::UnknownDependency {
                        step: step.id.clone(),
                        dependency: dependency.clone(),
                    });
                }
                outgoing
                    .entry(dependency.clone())
                    .or_default()
                    .push(step.id.clone());
            }
        }

        let roots = self
            .steps
            .iter()
            .filter(|step| step.depends_on.is_empty())
            .map(|step| step.id.clone())
            .collect::<Vec<_>>();
        let leaves = self
            .steps
            .iter()
            .filter(|step| outgoing.get(&step.id).is_none_or(Vec::is_empty))
            .map(|step| step.id.clone())
            .collect::<Vec<_>>();

        let mut ready = VecDeque::from(roots.clone());
        let mut order = Vec::with_capacity(self.steps.len());
        while let Some(id) = ready.pop_front() {
            order.push(id.clone());
            if let Some(children) = outgoing.get(&id) {
                for child in children {
                    let Some(count) = incoming.get_mut(child) else {
                        continue;
                    };
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        ready.push_back(child.clone());
                    }
                }
            }
        }

        if order.len() != self.steps.len() {
            return Err(WorkflowError::Cycle);
        }

        Ok(WorkflowPlan {
            order,
            roots,
            leaves,
        })
    }

    pub fn step(&self, id: &str) -> Option<&WorkflowStep> {
        self.steps.iter().find(|step| step.id == id)
    }
}

fn default_version() -> u16 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, depends_on: &[&str]) -> WorkflowStep {
        WorkflowStep {
            id: id.into(),
            action_id: format!("action-{id}"),
            depends_on: depends_on.iter().map(|value| (*value).into()).collect(),
            target_format: None,
            quality: None,
            parameters: HashMap::new(),
            output_policy: OutputPolicy::default(),
        }
    }

    #[test]
    fn validates_branching_dag() {
        let workflow = WorkflowDefinition {
            version: 1,
            name: "Photos".into(),
            description: String::new(),
            steps: vec![
                step("convert", &[]),
                step("resize", &["convert"]),
                step("privacy", &["convert"]),
                step("archive", &["resize", "privacy"]),
            ],
        };
        let plan = workflow.validate().unwrap();
        assert_eq!(plan.roots, vec!["convert"]);
        assert_eq!(plan.leaves, vec!["archive"]);
        assert_eq!(plan.order.first().map(String::as_str), Some("convert"));
        assert_eq!(plan.order.last().map(String::as_str), Some("archive"));
    }

    #[test]
    fn validates_large_linear_pipeline_without_recursion() {
        let count = 5_000usize;
        let mut steps = Vec::with_capacity(count);
        for index in 0..count {
            let id = format!("step-{index}");
            let depends_on = if index == 0 {
                Vec::new()
            } else {
                vec![format!("step-{}", index - 1)]
            };
            steps.push(WorkflowStep {
                id,
                action_id: "image-optimize".into(),
                depends_on,
                target_format: None,
                quality: None,
                parameters: HashMap::new(),
                output_policy: OutputPolicy::default(),
            });
        }
        let workflow = WorkflowDefinition {
            version: 1,
            name: "Stress DAG".into(),
            description: String::new(),
            steps,
        };
        let plan = workflow.validate().unwrap();
        assert_eq!(plan.order.len(), count);
        assert_eq!(plan.roots, vec!["step-0"]);
        assert_eq!(plan.leaves, vec![format!("step-{}", count - 1)]);
    }

    #[test]
    fn rejects_cycles() {
        let workflow = WorkflowDefinition {
            version: 1,
            name: String::new(),
            description: String::new(),
            steps: vec![step("a", &["b"]), step("b", &["a"])],
        };
        assert_eq!(workflow.validate(), Err(WorkflowError::Cycle));
    }
}
