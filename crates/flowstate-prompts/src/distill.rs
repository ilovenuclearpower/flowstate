/// Append review-distill instructions to the prompt.
pub fn append_instructions(prompt: &mut String, phase: &str, feedback: &str) {
    prompt.push_str("## Instructions — Review & Distill\n\n");
    prompt.push_str(&format!(
        "The previous {phase} output has been reviewed and feedback was provided. \
         Your task is to revise the {phase} artifact based on this feedback.\n\n"
    ));
    prompt.push_str("### Reviewer Feedback\n\n");
    prompt.push_str(feedback);
    prompt.push_str("\n\n");
    prompt.push_str(&format!(
        "Revise the {phase} document to address ALL feedback points. \
         Maintain everything that was correct in the original while fixing \
         the issues raised. Do not remove content unless the feedback \
         specifically asks for removal.\n\n"
    ));

    let output_file = match phase {
        "research" => "RESEARCH.md",
        "design" | "specification" => "SPECIFICATION.md",
        "plan" => "PLAN.md",
        "verification" | "verify" => "VERIFICATION.md",
        _ => "OUTPUT.md",
    };

    prompt.push_str(&format!(
        "IMPORTANT: Write the FULL revised document to a file named exactly \
         `{output_file}` in the current working directory. \
         This file will be picked up by the system.\n"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distill_output_file_mapping() {
        let cases = vec![
            ("research", "RESEARCH.md"),
            ("design", "SPECIFICATION.md"),
            ("specification", "SPECIFICATION.md"),
            ("plan", "PLAN.md"),
            ("verification", "VERIFICATION.md"),
            ("verify", "VERIFICATION.md"),
            ("unknown", "OUTPUT.md"),
        ];
        for (phase, expected_file) in cases {
            let mut prompt = String::new();
            append_instructions(&mut prompt, phase, "feedback");
            assert!(
                prompt.contains(expected_file),
                "phase '{phase}' should produce file '{expected_file}', got: {prompt}"
            );
        }
    }
}
