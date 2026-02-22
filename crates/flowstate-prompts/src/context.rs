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

    fn minimal_ctx() -> PromptContext {
        PromptContext {
            project_name: "TestProject".into(),
            repo_url: String::new(),
            task_title: "Test Task".into(),
            task_description: "Do the thing".into(),
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
    fn preamble_minimal() {
        let ctx = minimal_ctx();
        let mut out = String::new();
        ctx.append_preamble(&mut out);
        assert!(out.contains("# Project:"));
        assert!(out.contains("TestProject"));
        assert!(out.contains("# Task:"));
        assert!(out.contains("Test Task"));
        assert!(out.contains("## Description"));
        assert!(out.contains("Do the thing"));
        assert!(!out.contains("Repository:"));
    }

    #[test]
    fn preamble_with_repo_url() {
        let mut ctx = minimal_ctx();
        ctx.repo_url = "https://github.com/test/repo".into();
        let mut out = String::new();
        ctx.append_preamble(&mut out);
        assert!(out.contains("Repository:"));
        assert!(out.contains("https://github.com/test/repo"));
    }

    #[test]
    fn preamble_with_spec_and_plan() {
        let mut ctx = minimal_ctx();
        ctx.spec_content = Some("The spec body".into());
        ctx.plan_content = Some("The plan body".into());
        let mut out = String::new();
        ctx.append_preamble(&mut out);
        assert!(out.contains("## Specification"));
        assert!(out.contains("The spec body"));
        assert!(out.contains("## Implementation Plan"));
        assert!(out.contains("The plan body"));
    }

    #[test]
    fn preamble_with_research() {
        let mut ctx = minimal_ctx();
        ctx.research_content = Some("Research findings".into());
        let mut out = String::new();
        ctx.append_preamble(&mut out);
        assert!(out.contains("## Research"));
        assert!(out.contains("Research findings"));
    }

    #[test]
    fn preamble_with_verification() {
        let mut ctx = minimal_ctx();
        ctx.verification_content = Some("Verification results".into());
        let mut out = String::new();
        ctx.append_preamble(&mut out);
        assert!(out.contains("## Verification"));
        assert!(out.contains("Verification results"));
    }

    #[test]
    fn preamble_with_parent_context() {
        let mut ctx = minimal_ctx();
        ctx.parent_context = Some(ParentContext {
            title: "Parent Task".into(),
            description: "Parent desc".into(),
            spec_content: Some("Parent spec".into()),
            plan_content: Some("Parent plan".into()),
        });
        let mut out = String::new();
        ctx.append_preamble(&mut out);
        assert!(out.contains("# Parent Task:"));
        assert!(out.contains("Parent desc"));
        assert!(out.contains("## Parent Specification"));
        assert!(out.contains("Parent spec"));
        assert!(out.contains("## Parent Plan"));
        assert!(out.contains("Parent plan"));
    }

    #[test]
    fn preamble_with_child_tasks() {
        let mut ctx = minimal_ctx();
        ctx.child_tasks = vec![
            ChildTaskInfo {
                title: "Child A".into(),
                description: "Do A".into(),
                status: "Done".into(),
            },
            ChildTaskInfo {
                title: "Child B".into(),
                description: "Do B".into(),
                status: "InProgress".into(),
            },
        ];
        let mut out = String::new();
        ctx.append_preamble(&mut out);
        assert!(out.contains("## Sub-tasks"));
        assert!(out.contains("Child A"));
        assert!(out.contains("Child B"));
        assert!(out.contains("[Done]"));
        assert!(out.contains("[InProgress]"));
    }
}
