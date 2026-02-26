/// Append plan-phase instructions to the prompt.
pub fn append_instructions(prompt: &mut String) {
    prompt.push_str("## Instructions\n\n");
    prompt.push_str(
        "Based on the specification above, produce a detailed implementation plan. \
         The plan MUST contain all five of the following sections:\n\n\
         ### 1. Directories and Files\n\
         List every directory and file that will be created or modified. \
         Mark each entry as NEW or MODIFIED. Use a table or bullet list with full paths.\n\n\
         ### 2. Work Phases\n\
         Break the implementation into ordered phases. For each phase provide:\n\
         - Phase name and objective\n\
         - Ordered steps within the phase\n\
         - Dependencies on prior phases (if any)\n\
         - Deliverables (concrete outputs: files written, tests passing, etc.)\n\n\
         ### 3. Agent/Capability Tier Assignments\n\
         For each phase, recommend:\n\
         - Which capability tier to use: Heavy (for architecture, complex implementation), \
         Standard (for design, planning, verification), or Light (for research, boilerplate, distillation)\n\
         - A brief agent personality description (e.g. \"Senior backend engineer focused on API correctness\")\n\
         - Whether multiple agents can work in parallel on sub-tasks within this phase\n\n\
         ### 4. Validation Steps\n\
         For each phase, specify:\n\
         - Automated checks: exact commands to run (test suites, linters, build commands, type checks)\n\
         - Human review checkpoints: what a reviewer should verify before moving to the next phase\n\n\
         ### 5. Subtask Definitions\n\n\
         Break the implementation into discrete subtasks. Each subtask will be created as a \
         child of the current task and will go through its own Build → Verify lifecycle.\n\n\
         For each subtask, write a block in EXACTLY this format:\n\n\
         #### SUBTASK: <title>\n\
         **Capability:** <light|standard|heavy>\n\
         **Description:**\n\n\
         <detailed description of what this subtask accomplishes, including which files \
         it touches and what the acceptance criteria are>\n\
         ** Files **\n\
         1 `list`\n\
         2 `of`\n\
         3 `paths`\n\
         ---\n\n\
         Guidelines for subtask decomposition:\n\
         - Each subtask should be independently buildable and verifiable\n\
         - Order subtasks by dependency (earlier subtasks should not depend on later ones)\n\
         - Assign capability tiers based on complexity:\n\
           - Light: boilerplate, simple additions, config changes\n\
           - Standard: moderate logic, API endpoints, database queries\n\
           - Heavy: complex architecture, multi-file refactors, algorithmic work\n\
         - Subtasks skip Research/Design/Plan phases — they go directly Todo → Build → Verify → Done\n\
         - Keep subtasks focused: each should take a single build run to complete\n\
         - Always include exactly what files should be touched\n\n\
         IMPORTANT: Write the FULL plan to a file named exactly \
         `PLAN.md` in the current working directory. \
         This file will be picked up by the system. \
         You may use tools (web search, file reading, etc.) for research.\n",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_instructions_content() {
        let mut out = String::new();
        append_instructions(&mut out);
        assert!(out.contains("## Instructions"));
        assert!(out.contains("implementation plan"));
        assert!(out.contains("Work Phases"));
        assert!(out.contains("Validation Steps"));
        assert!(out.contains("Subtask Definitions"));
        assert!(out.contains("#### SUBTASK:"));
        assert!(out.contains("**Capability:**"));
        assert!(out.contains("** Files **"));
    }
}
