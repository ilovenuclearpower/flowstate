use std::fmt;

use serde::{Deserialize, Serialize};

use crate::claude_run::ClaudeAction;

/// Capability tier for runner classification.
///
/// Tiers are ordered by capability — a runner at a higher tier
/// can handle work at any lower tier. A Heavy runner advertises
/// capabilities [Light, Standard, Heavy]. A Light runner advertises
/// only [Light].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerCapability {
    /// Fast, cheap models suited for research and distill phases.
    Light,
    /// Capable models suited for design, planning, verification.
    Standard,
    /// Frontier models for complex builds and architecture.
    Heavy,
}

impl RunnerCapability {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunnerCapability::Light => "light",
            RunnerCapability::Standard => "standard",
            RunnerCapability::Heavy => "heavy",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "light" => Some(RunnerCapability::Light),
            "standard" => Some(RunnerCapability::Standard),
            "heavy" => Some(RunnerCapability::Heavy),
            _ => None,
        }
    }

    /// Return all capability tiers this tier can handle.
    /// A higher tier can handle all lower-tier work.
    pub fn handled_tiers(&self) -> Vec<RunnerCapability> {
        match self {
            RunnerCapability::Light => vec![RunnerCapability::Light],
            RunnerCapability::Standard => {
                vec![RunnerCapability::Light, RunnerCapability::Standard]
            }
            RunnerCapability::Heavy => vec![
                RunnerCapability::Light,
                RunnerCapability::Standard,
                RunnerCapability::Heavy,
            ],
        }
    }

    /// Default required capability tier for a given action type.
    pub fn default_for_action(action: ClaudeAction) -> Self {
        match action {
            ClaudeAction::Research => RunnerCapability::Light,
            ClaudeAction::ResearchDistill => RunnerCapability::Light,
            ClaudeAction::Design => RunnerCapability::Standard,
            ClaudeAction::DesignDistill => RunnerCapability::Light,
            ClaudeAction::Plan => RunnerCapability::Standard,
            ClaudeAction::PlanDistill => RunnerCapability::Light,
            ClaudeAction::Build => RunnerCapability::Heavy,
            ClaudeAction::Verify => RunnerCapability::Standard,
            ClaudeAction::VerifyDistill => RunnerCapability::Light,
        }
    }
}

impl fmt::Display for RunnerCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
