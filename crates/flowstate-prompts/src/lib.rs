pub mod build;
pub mod context;
pub mod design;
pub mod distill;
pub mod plan;
pub mod research;
pub mod verify;

pub use context::{ChildTaskInfo, ParentContext, PromptContext};
use flowstate_core::claude_run::ClaudeAction;

/// Assemble the full prompt for a given action and context.
pub fn assemble_prompt(ctx: &PromptContext, action: ClaudeAction) -> String {
    let mut prompt = String::new();
    ctx.append_preamble(&mut prompt);

    let feedback = ctx.distill_feedback.as_deref().unwrap_or("(no feedback)");

    match action {
        ClaudeAction::Research => research::append_instructions(&mut prompt),
        ClaudeAction::Design => design::append_instructions(&mut prompt),
        ClaudeAction::Plan => plan::append_instructions(&mut prompt),
        ClaudeAction::Build => build::append_instructions(&mut prompt),
        ClaudeAction::Verify => verify::append_instructions(&mut prompt),
        ClaudeAction::ResearchDistill => {
            if let Some(ref content) = ctx.research_content {
                prompt.push_str("## Current Research Document\n\n");
                prompt.push_str(content);
                prompt.push_str("\n\n");
            }
            distill::append_instructions(&mut prompt, "research", feedback);
        }
        ClaudeAction::DesignDistill => {
            distill::append_instructions(&mut prompt, "design", feedback);
        }
        ClaudeAction::PlanDistill => {
            distill::append_instructions(&mut prompt, "plan", feedback);
        }
        ClaudeAction::VerifyDistill => {
            distill::append_instructions(&mut prompt, "verification", feedback);
        }
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use context::PromptContext;

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
    fn test_assemble_prompt_research() {
        let ctx = test_context();
        let output = assemble_prompt(&ctx, ClaudeAction::Research);
        assert!(output.contains("# Project: TestProject"));
        assert!(output.contains("## Instructions"));
    }

    #[test]
    fn test_assemble_prompt_build() {
        let ctx = test_context();
        let output = assemble_prompt(&ctx, ClaudeAction::Build);
        assert!(output.contains("# Project: TestProject"));
        assert!(output.contains("## Instructions"));
    }

    #[test]
    fn test_assemble_prompt_distill_includes_feedback() {
        let mut ctx = test_context();
        ctx.distill_feedback = Some("Please fix the intro section".into());
        let output = assemble_prompt(&ctx, ClaudeAction::ResearchDistill);
        assert!(output.contains("Please fix the intro section"));
        assert!(output.contains("research"));
    }
}
