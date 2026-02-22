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
    fn assemble_prompt_research() {
        let ctx = minimal_ctx();
        let out = assemble_prompt(&ctx, ClaudeAction::Research);
        assert!(out.contains("# Project:"));
        assert!(out.contains("Perform a thorough research phase"));
    }

    #[test]
    fn assemble_prompt_design() {
        let ctx = minimal_ctx();
        let out = assemble_prompt(&ctx, ClaudeAction::Design);
        assert!(out.contains("technical specification"));
    }

    #[test]
    fn assemble_prompt_plan() {
        let ctx = minimal_ctx();
        let out = assemble_prompt(&ctx, ClaudeAction::Plan);
        assert!(out.contains("implementation plan"));
    }

    #[test]
    fn assemble_prompt_build() {
        let ctx = minimal_ctx();
        let out = assemble_prompt(&ctx, ClaudeAction::Build);
        assert!(out.contains("Implement the changes"));
    }

    #[test]
    fn assemble_prompt_verify() {
        let ctx = minimal_ctx();
        let out = assemble_prompt(&ctx, ClaudeAction::Verify);
        assert!(out.contains("verification"));
    }

    #[test]
    fn assemble_prompt_research_distill() {
        let mut ctx = minimal_ctx();
        ctx.research_content = Some("Existing research".into());
        ctx.distill_feedback = Some("fix typos".into());
        let out = assemble_prompt(&ctx, ClaudeAction::ResearchDistill);
        assert!(out.contains("Current Research Document"));
        assert!(out.contains("Review & Distill"));
        assert!(out.contains("Existing research"));
    }

    #[test]
    fn assemble_prompt_design_distill() {
        let mut ctx = minimal_ctx();
        ctx.distill_feedback = Some("revise API".into());
        let out = assemble_prompt(&ctx, ClaudeAction::DesignDistill);
        assert!(out.contains("Review & Distill"));
        assert!(out.contains("design"));
    }

    #[test]
    fn assemble_prompt_plan_distill() {
        let mut ctx = minimal_ctx();
        ctx.distill_feedback = Some("add phases".into());
        let out = assemble_prompt(&ctx, ClaudeAction::PlanDistill);
        assert!(out.contains("Review & Distill"));
        assert!(out.contains("plan"));
    }

    #[test]
    fn assemble_prompt_verify_distill() {
        let mut ctx = minimal_ctx();
        ctx.distill_feedback = Some("check edge cases".into());
        let out = assemble_prompt(&ctx, ClaudeAction::VerifyDistill);
        assert!(out.contains("Review & Distill"));
        assert!(out.contains("verification"));
    }
}
