/// Append plan-phase instructions to the prompt.
pub fn append_instructions(prompt: &mut String) {
    prompt.push_str("## Instructions\n\n");
    prompt.push_str(
        "Based on the specification above, produce a detailed implementation plan. \
         The plan MUST contain all four of the following sections:\n\n\
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
         IMPORTANT: Write the FULL plan to a file named exactly \
         `PLAN.md` in the current working directory. \
         This file will be picked up by the system. \
         You may use tools (web search, file reading, etc.) for research.\n",
    );
}
