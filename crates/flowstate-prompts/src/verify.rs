/// Append verify-phase instructions to the prompt.
pub fn append_instructions(prompt: &mut String) {
    prompt.push_str("## Instructions\n\n");
    prompt.push_str(
        "Perform a thorough verification of the implementation against the \
         specification and plan. Your goal is to ensure the implementation \
         meets all requirements.\n\n\
         Your verification report MUST include:\n\
         - **Spec Compliance**: For each requirement in the specification, \
           confirm whether it has been implemented. Flag any gaps.\n\
         - **Plan Adherence**: Verify that the implementation followed the \
           plan's phases, file changes, and architecture decisions.\n\
         - **Test Coverage**: Check that tests exist for new functionality \
           and that all tests pass.\n\
         - **Code Quality**: Review for code style consistency, proper error \
           handling, security considerations, and performance.\n\
         - **Edge Cases**: Verify that edge cases identified in the spec \
           are properly handled.\n\
         - **Verdict**: State either PASS or FAIL with a summary rationale.\n\n\
         IMPORTANT: Write the FULL verification report to a file named exactly \
         `VERIFICATION.md` in the current working directory. \
         This file will be picked up by the system. \
         You may use tools (web search, file reading, code execution, etc.) \
         for verification.\n",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_instructions_content() {
        let mut out = String::new();
        append_instructions(&mut out);
        assert!(out.contains("## Instructions"));
        assert!(out.contains("verification"));
        assert!(out.contains("VERIFICATION.md"));
    }
}
