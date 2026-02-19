pub mod build;
pub mod context;
pub mod design;
pub mod distill;
pub mod plan;
pub mod research;
pub mod verify;

pub use context::{ChildTaskInfo, PromptContext};
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
