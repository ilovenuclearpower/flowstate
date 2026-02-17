/// Append build-phase instructions to the prompt.
pub fn append_instructions(prompt: &mut String) {
    prompt.push_str("## Instructions\n\n");
    prompt.push_str(
        "Implement the changes described in the specification and plan above.\n\n\
         Follow these guidelines:\n\
         - Follow the implementation plan phase by phase, in order.\n\
         - Follow existing code style and patterns in the repository.\n\
         - Make atomic, well-scoped changes — do not modify anything outside the plan's scope.\n\
         - Run the project's test suite and fix any failures before finishing.\n\
         - Do not introduce new dependencies unless they are explicitly specified in the plan.\n\
         - Write clean, well-tested code.\n\
         - Ensure all existing tests continue to pass.\n\
         - If the plan specifies validation commands, run them and confirm they pass.\n",
    );
}
