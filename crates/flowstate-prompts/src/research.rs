/// Append research-phase instructions to the prompt.
pub fn append_instructions(prompt: &mut String) {
    prompt.push_str("## Instructions\n\n");
    prompt.push_str(
        "Perform a thorough research phase for this task. Your goal is to \
         analyze the codebase, understand the problem domain, and document \
         everything needed before design begins.\n\n\
         Your research document MUST include:\n\
         - **Codebase Analysis**: Identify all relevant files, modules, and \
           patterns that relate to this task. Map out the dependency graph.\n\
         - **Key Details**: Extract important constants, configurations, \
           type definitions, and interfaces that will be affected.\n\
         - **Major Complications**: Identify technical debt, edge cases, \
           performance concerns, or architectural constraints that could \
           complicate implementation.\n\
         - **Non-Functional Requirements**: Document scalability, performance, \
           security, accessibility, and maintainability considerations.\n\
         - **Aspirational Attributes**: Note quality-of-life improvements, \
           developer experience enhancements, or stretch goals worth considering.\n\
         - **Open Questions**: List anything that needs clarification from \
           stakeholders before proceeding to design.\n\n\
         IMPORTANT: Write the FULL research document to a file named exactly \
         `RESEARCH.md` in the current working directory. \
         This file will be picked up by the system. \
         You may use tools (web search, file reading, etc.) for research.\n",
    );
}
