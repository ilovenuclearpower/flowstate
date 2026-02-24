use crate::runner::RunnerCapability;
use serde::{Deserialize, Serialize};

/// A subtask definition parsed from a plan's structured output.
/// Used by the runner to create child tasks after plan completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskDefinition {
    /// Short title for the subtask
    pub title: String,
    /// Detailed description of what this subtask accomplishes
    pub description: String,
    /// Recommended capability tier for building this subtask
    pub build_capability: Option<RunnerCapability>,
    /// Sort order hint (used to preserve plan ordering)
    pub sort_order: f64,
    /// Files this subtask should touch (allowlist from the plan)
    pub files: Vec<String>,
}
