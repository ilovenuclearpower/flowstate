/// Append design-phase instructions to the prompt.
pub fn append_instructions(prompt: &mut String) {
    prompt.push_str("## Instructions\n\n");
    prompt.push_str(
        "Produce a detailed technical specification for this task. \
         The specification should include:\n\
         - Problem statement and goals\n\
         - Proposed solution architecture\n\
         - API changes or new interfaces\n\
         - Data model changes\n\
         - Edge cases and error handling\n\
         - Testing strategy\n\n\
         IMPORTANT: Write the FULL specification to a file named exactly \
         `SPECIFICATION.md` in the current working directory. \
         This file will be picked up by the system. \
         You may use tools (web search, file reading, etc.) for research.\n",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_instructions_content() {
        let mut out = String::new();
        append_instructions(&mut out);
        assert!(out.contains("## Instructions"));
        assert!(out.contains("technical specification"));
        assert!(out.contains("SPECIFICATION.md"));
    }
}
