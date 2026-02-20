use std::collections::HashMap;
use std::time::Instant;

use flowstate_core::claude_run::ClaudeAction;
use serde::Serialize;

/// Tracks active runs for health reporting and capacity management.
pub struct RunTracker {
    active: HashMap<String, ActiveRun>,
}

/// An in-progress run tracked by the runner.
pub struct ActiveRun {
    pub run_id: String,
    pub task_id: String,
    pub action: ClaudeAction,
    pub started_at: Instant,
}

/// Serializable snapshot of an active run for the health endpoint.
#[derive(Serialize)]
pub struct ActiveRunSnapshot {
    pub run_id: String,
    pub task_id: String,
    pub action: String,
    pub elapsed_seconds: u64,
}

/// Result returned from a spawned run task.
pub struct RunResult {
    pub run_id: String,
    pub task_id: String,
    pub action: ClaudeAction,
    pub outcome: RunOutcome,
}

/// Outcome of a completed run.
pub enum RunOutcome {
    Success,
    Failed(String),
    TimedOut,
    Panicked(String),
}

impl Default for RunTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl RunTracker {
    pub fn new() -> Self {
        Self {
            active: HashMap::new(),
        }
    }

    pub fn insert(&mut self, run: ActiveRun) {
        self.active.insert(run.run_id.clone(), run);
    }

    pub fn remove(&mut self, run_id: &str) {
        self.active.remove(run_id);
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn active_build_count(&self) -> usize {
        self.active
            .values()
            .filter(|r| r.action == ClaudeAction::Build)
            .count()
    }

    /// Return a consistent snapshot of all active runs for health reporting.
    pub fn snapshot(&self) -> Vec<ActiveRunSnapshot> {
        self.active
            .values()
            .map(|r| ActiveRunSnapshot {
                run_id: r.run_id.clone(),
                task_id: r.task_id.clone(),
                action: r.action.as_str().to_string(),
                elapsed_seconds: r.started_at.elapsed().as_secs(),
            })
            .collect()
    }
}
