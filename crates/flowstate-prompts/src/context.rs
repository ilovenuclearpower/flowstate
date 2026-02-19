use serde::{Deserialize, Serialize};

/// Information about a child task, for inclusion in prompts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildTaskInfo {
    pub title: String,
    pub description: String,
    pub status: String,
}

/// All the context needed to assemble a prompt for any action.
#[derive(Debug, Clone)]
pub struct PromptContext {
    pub project_name: String,
    pub repo_url: String,
    pub task_title: String,
    pub task_description: String,
    pub spec_content: Option<String>,
    pub plan_content: Option<String>,
    pub research_content: Option<String>,
    pub verification_content: Option<String>,
    pub distill_feedback: Option<String>,
    pub child_tasks: Vec<ChildTaskInfo>,
}

impl PromptContext {
    /// Render the shared preamble: project header, task description, spec, plan, children.
    pub fn append_preamble(&self, prompt: &mut String) {
        prompt.push_str(&format!("# Project: {}\n\n", self.project_name));
        if !self.repo_url.is_empty() {
            prompt.push_str(&format!("Repository: {}\n\n", self.repo_url));
        }

        prompt.push_str(&format!("# Task: {}\n\n", self.task_title));
        prompt.push_str(&format!("## Description\n\n{}\n\n", self.task_description));

        if let Some(ref research) = self.research_content {
            prompt.push_str("## Research\n\n");
            prompt.push_str(research);
            prompt.push_str("\n\n");
        }

        if let Some(ref spec) = self.spec_content {
            prompt.push_str("## Specification\n\n");
            prompt.push_str(spec);
            prompt.push_str("\n\n");
        }

        if let Some(ref plan) = self.plan_content {
            prompt.push_str("## Implementation Plan\n\n");
            prompt.push_str(plan);
            prompt.push_str("\n\n");
        }

        if let Some(ref verification) = self.verification_content {
            prompt.push_str("## Verification\n\n");
            prompt.push_str(verification);
            prompt.push_str("\n\n");
        }

        if !self.child_tasks.is_empty() {
            prompt.push_str("## Sub-tasks\n\n");
            for child in &self.child_tasks {
                prompt.push_str(&format!(
                    "- [{}] {}: {}\n",
                    child.status, child.title, child.description
                ));
            }
            prompt.push('\n');
        }
    }
}
