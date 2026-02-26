/// Append build-phase instructions to the prompt.
pub fn append_instructions(prompt: &mut String, file_allowlist: &[String]) {
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

    if !file_allowlist.is_empty() {
        prompt.push_str(
            "\n### File Change Restrictions\n\n\
             You MUST only modify files listed in the allowlist below. \
             If you find yourself needing to change files not on the list, \
             stop and consider whether you are overcomplicating the solution. \
             Only change unlisted files if absolutely necessary and document why.\n\n\
             **Allowed files:**\n",
        );
        for path in file_allowlist {
            prompt.push_str(&format!("- `{path}`\n"));
        }
        prompt.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_instructions_content() {
        let mut out = String::new();
        append_instructions(&mut out, &[]);
        assert!(out.contains("## Instructions"));
        assert!(out.contains("Implement the changes"));
        assert!(out.contains("test suite"));
        assert!(!out.contains("File Change Restrictions"));
    }

    #[test]
    fn build_instructions_with_file_allowlist() {
        let mut out = String::new();
        append_instructions(
            &mut out,
            &["src/main.rs".to_string(), "Cargo.toml".to_string()],
        );
        assert!(out.contains("File Change Restrictions"));
        assert!(out.contains("Allowed files:"));
        assert!(out.contains("`src/main.rs`"));
        assert!(out.contains("`Cargo.toml`"));
    }
}
