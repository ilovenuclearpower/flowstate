use serde::{Deserialize, Serialize};

/// Information about a child task, for inclusion in prompts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildTaskInfo {
    pub title: String,
    pub description: String,
    pub status: String,
}

/// Context from a parent task, injected into subtask prompts.
#[derive(Debug, Clone)]
pub struct ParentContext {
    pub title: String,
    pub description: String,
    pub spec_content: Option<String>,
    pub plan_content: Option<String>,
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
    pub parent_context: Option<ParentContext>,
}

impl PromptContext {
    /// Render the shared preamble: project header, task description, spec, plan, children.
    pub fn append_preamble(&self, prompt: &mut String) {
        prompt.push_str(&format!("# Project: {}\n\n", self.project_name));
        if !self.repo_url.is_empty() {
            prompt.push_str(&format!("Repository: {}\n\n", self.repo_url));
        }

        if let Some(ref parent) = self.parent_context {
            prompt.push_str(&format!("# Parent Task: {}\n\n", parent.title));
            prompt.push_str(&format!("{}\n\n", parent.description));
            if let Some(ref spec) = parent.spec_content {
                prompt.push_str("## Parent Specification\n\n");
                prompt.push_str(spec);
                prompt.push_str("\n\n");
            }
            if let Some(ref plan) = parent.plan_content {
                prompt.push_str("## Parent Plan\n\n");
                prompt.push_str(plan);
                prompt.push_str("\n\n");
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context() -> PromptContext {
        PromptContext {
            project_name: "TestProject".into(),
            repo_url: "https://github.com/test/repo".into(),
            task_title: "Test Task".into(),
            task_description: "A test task description".into(),
            spec_content: None,
            plan_content: None,
            research_content: None,
            verification_content: None,
            distill_feedback: None,
            child_tasks: vec![],
            parent_context: None,
        }
    }

    #[test]
    fn test_preamble_includes_project_and_task() {
        let ctx = test_context();
        let mut output = String::new();
        ctx.append_preamble(&mut output);
        assert!(output.contains("# Project: TestProject"), "should contain project name");
        assert!(output.contains("# Task: Test Task"), "should contain task title");
    }

    #[test]
    fn test_preamble_includes_repo_url() {
        let ctx = test_context();
        let mut output = String::new();
        ctx.append_preamble(&mut output);
        assert!(output.contains("Repository: https://github.com/test/repo"));
    }

    #[test]
    fn test_preamble_omits_repo_url_when_empty() {
        let mut ctx = test_context();
        ctx.repo_url = String::new();
        let mut output = String::new();
        ctx.append_preamble(&mut output);
        assert!(!output.contains("Repository:"));
    }

    #[test]
    fn test_preamble_includes_parent_context() {
        let mut ctx = test_context();
        ctx.parent_context = Some(ParentContext {
            title: "Parent Feature".into(),
            description: "Parent desc".into(),
            spec_content: None,
            plan_content: None,
        });
        let mut output = String::new();
        ctx.append_preamble(&mut output);
        assert!(output.contains("# Parent Task: Parent Feature"));
    }

    #[test]
    fn test_preamble_includes_child_tasks() {
        let mut ctx = test_context();
        ctx.child_tasks = vec![ChildTaskInfo {
            title: "Subtask 1".into(),
            description: "Do sub-thing".into(),
            status: "todo".into(),
        }];
        let mut output = String::new();
        ctx.append_preamble(&mut output);
        assert!(output.contains("## Sub-tasks"));
        assert!(output.contains("Subtask 1"));
    }

    #[test]
    fn test_preamble_includes_content_sections() {
        let mut ctx = test_context();
        ctx.spec_content = Some("spec text".into());
        ctx.plan_content = Some("plan text".into());
        ctx.research_content = Some("research text".into());
        ctx.verification_content = Some("verify text".into());
        let mut output = String::new();
        ctx.append_preamble(&mut output);
        assert!(output.contains("## Specification"));
        assert!(output.contains("## Implementation Plan"));
        assert!(output.contains("## Research"));
        assert!(output.contains("## Verification"));
    }
}
