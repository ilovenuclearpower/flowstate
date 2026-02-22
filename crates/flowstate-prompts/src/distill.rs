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
    fn distill_output_file_mapping() {
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

    #[test]
    fn distill_instructions_research() {
        let mut out = String::new();
        append_instructions(&mut out, "research", "fix typos");
        assert!(out.contains("Review & Distill"));
        assert!(out.contains("research"));
        assert!(out.contains("fix typos"));
        assert!(out.contains("RESEARCH.md"));
    }

    #[test]
    fn distill_instructions_design() {
        let mut out = String::new();
        append_instructions(&mut out, "design", "revise API");
        assert!(out.contains("Review & Distill"));
        assert!(out.contains("design"));
        assert!(out.contains("SPECIFICATION.md"));
    }

    #[test]
    fn distill_instructions_plan() {
        let mut out = String::new();
        append_instructions(&mut out, "plan", "add phases");
        assert!(out.contains("Review & Distill"));
        assert!(out.contains("plan"));
        assert!(out.contains("PLAN.md"));
    }

    #[test]
    fn distill_instructions_verify() {
        let mut out = String::new();
        append_instructions(&mut out, "verification", "check edge cases");
        assert!(out.contains("Review & Distill"));
        assert!(out.contains("verification"));
        assert!(out.contains("VERIFICATION.md"));
    }

    #[test]
    fn distill_instructions_unknown_phase() {
        let mut out = String::new();
        append_instructions(&mut out, "foobar", "some feedback");
        assert!(out.contains("Review & Distill"));
        assert!(out.contains("foobar"));
        assert!(out.contains("OUTPUT.md"));
    }

    #[test]
    fn distill_instructions_empty_feedback() {
        let mut out = String::new();
        append_instructions(&mut out, "research", "");
        assert!(out.contains("Review & Distill"));
        assert!(out.contains("research"));
        assert!(out.contains("RESEARCH.md"));
        assert!(out.contains("### Reviewer Feedback"));
    }
}
