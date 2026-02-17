pub mod build;
pub mod context;
pub mod design;
pub mod plan;

pub use context::{ChildTaskInfo, PromptContext};
use flowstate_core::claude_run::ClaudeAction;

/// Assemble the full prompt for a given action and context.
pub fn assemble_prompt(ctx: &PromptContext, action: ClaudeAction) -> String {
    let mut prompt = String::new();
    ctx.append_preamble(&mut prompt);

    match action {
        ClaudeAction::Design => design::append_instructions(&mut prompt),
        ClaudeAction::Plan => plan::append_instructions(&mut prompt),
        ClaudeAction::Build => build::append_instructions(&mut prompt),
    }

    prompt
}
