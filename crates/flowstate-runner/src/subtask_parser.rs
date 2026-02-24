use flowstate_core::runner::RunnerCapability;
use flowstate_core::subtask::SubtaskDefinition;

/// Extract subtask definitions from a plan's markdown content.
///
/// Looks for blocks in this format:
/// ```markdown
/// #### SUBTASK: <title>
/// **Capability:** <light|standard|heavy>
/// **Description:**
/// <multi-line description>
/// ** Files **
/// 1 `path/to/file`
/// 2 `another/file`
/// ---
/// ```
pub fn extract_subtasks(plan_content: &str) -> Vec<SubtaskDefinition> {
    let mut subtasks = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_capability: Option<RunnerCapability> = None;
    let mut current_description = String::new();
    let mut current_files: Vec<String> = Vec::new();
    let mut in_description = false;
    let mut in_files = false;
    let mut sort_order = 1.0_f64;

    for line in plan_content.lines() {
        let trimmed = line.trim();

        // Detect subtask heading
        if let Some(title) = trimmed
            .strip_prefix("#### SUBTASK:")
            .or_else(|| trimmed.strip_prefix("#### SUBTASK :"))
        {
            // Flush previous subtask if any
            if let Some(title_val) = current_title.take() {
                subtasks.push(build_definition(
                    title_val,
                    current_capability.take(),
                    &current_description,
                    &current_files,
                    sort_order,
                ));
                sort_order += 1.0;
                current_description.clear();
                current_files.clear();
            }

            current_title = Some(title.trim().to_string());
            in_description = false;
            in_files = false;
            continue;
        }

        // Only process lines when inside a subtask block
        if current_title.is_none() {
            continue;
        }

        // Delimiter ends the block
        if trimmed == "---" {
            if let Some(title_val) = current_title.take() {
                subtasks.push(build_definition(
                    title_val,
                    current_capability.take(),
                    &current_description,
                    &current_files,
                    sort_order,
                ));
                sort_order += 1.0;
                current_description.clear();
                current_files.clear();
            }
            in_description = false;
            in_files = false;
            continue;
        }

        // Parse capability line
        if let Some(cap_str) = trimmed
            .strip_prefix("**Capability:**")
            .or_else(|| trimmed.strip_prefix("**Capability: **"))
        {
            let cap_str = cap_str.trim().to_lowercase();
            current_capability = RunnerCapability::parse_str(&cap_str);
            in_description = false;
            in_files = false;
            continue;
        }

        // Detect description section start
        if trimmed.starts_with("**Description:**") || trimmed.starts_with("**Description: **") {
            in_description = true;
            in_files = false;
            // Check if description content is on the same line
            let after = trimmed
                .strip_prefix("**Description:**")
                .or_else(|| trimmed.strip_prefix("**Description: **"))
                .unwrap_or("")
                .trim();
            if !after.is_empty() {
                current_description.push_str(after);
                current_description.push('\n');
            }
            continue;
        }

        // Detect files section start
        if trimmed.starts_with("** Files **") || trimmed.starts_with("**Files**") {
            in_description = false;
            in_files = true;
            continue;
        }

        // Collect description lines
        if in_description {
            current_description.push_str(trimmed);
            current_description.push('\n');
            continue;
        }

        // Collect file paths
        if in_files {
            if let Some(path) = extract_file_path(trimmed) {
                current_files.push(path);
            }
        }
    }

    // Flush trailing subtask (no closing delimiter)
    if let Some(title_val) = current_title.take() {
        subtasks.push(build_definition(
            title_val,
            current_capability.take(),
            &current_description,
            &current_files,
            sort_order,
        ));
    }

    subtasks
}

fn build_definition(
    title: String,
    capability: Option<RunnerCapability>,
    description: &str,
    files: &[String],
    sort_order: f64,
) -> SubtaskDefinition {
    let description = description.trim().to_string();
    let description = if description.is_empty() {
        title.clone()
    } else {
        description
    };

    SubtaskDefinition {
        title,
        description,
        build_capability: Some(capability.unwrap_or(RunnerCapability::Standard)),
        sort_order,
        files: files.to_vec(),
    }
}

/// Extract a file path from a numbered or bulleted list line with backticks.
///
/// Matches patterns like:
/// - `1 \`path/to/file\``
/// - `1. \`path/to/file\``
/// - `- \`path/to/file\``
/// - `* \`path/to/file\``
/// - `\`path/to/file\``
fn extract_file_path(line: &str) -> Option<String> {
    let start = line.find('`')?;
    let rest = &line[start + 1..];
    let end = rest.find('`')?;
    let path = rest[..end].trim().to_string();
    if path.is_empty() {
        return None;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_multiple_subtasks() {
        let plan = r#"
### 5. Subtask Definitions

#### SUBTASK: Add SubtaskDefinition type
**Capability:** light
**Description:**
Create the subtask.rs file with the SubtaskDefinition struct.
Add pub mod subtask to lib.rs.
** Files **
1 `crates/flowstate-core/src/subtask.rs`
2 `crates/flowstate-core/src/lib.rs`
---

#### SUBTASK: Implement subtask parser
**Capability:** standard
**Description:**
Create subtask_parser.rs that parses structured subtask format.
Include comprehensive unit tests.
** Files **
1 `crates/flowstate-runner/src/subtask_parser.rs`
2 `crates/flowstate-runner/src/lib.rs`
---
"#;
        let subtasks = extract_subtasks(plan);
        assert_eq!(subtasks.len(), 2);

        assert_eq!(subtasks[0].title, "Add SubtaskDefinition type");
        assert_eq!(
            subtasks[0].build_capability,
            Some(RunnerCapability::Light)
        );
        assert!(subtasks[0].description.contains("subtask.rs"));
        assert_eq!(subtasks[0].files.len(), 2);
        assert_eq!(subtasks[0].files[0], "crates/flowstate-core/src/subtask.rs");
        assert_eq!(subtasks[0].sort_order, 1.0);

        assert_eq!(subtasks[1].title, "Implement subtask parser");
        assert_eq!(
            subtasks[1].build_capability,
            Some(RunnerCapability::Standard)
        );
        assert!(subtasks[1].description.contains("subtask_parser.rs"));
        assert_eq!(subtasks[1].files.len(), 2);
        assert_eq!(subtasks[1].sort_order, 2.0);
    }

    #[test]
    fn missing_capability_defaults_to_standard() {
        let plan = r#"
#### SUBTASK: No capability specified
**Description:**
Do the thing.
---
"#;
        let subtasks = extract_subtasks(plan);
        assert_eq!(subtasks.len(), 1);
        assert_eq!(
            subtasks[0].build_capability,
            Some(RunnerCapability::Standard)
        );
    }

    #[test]
    fn missing_description_uses_title() {
        let plan = r#"
#### SUBTASK: Just a title
**Capability:** heavy
---
"#;
        let subtasks = extract_subtasks(plan);
        assert_eq!(subtasks.len(), 1);
        assert_eq!(subtasks[0].description, "Just a title");
        assert_eq!(
            subtasks[0].build_capability,
            Some(RunnerCapability::Heavy)
        );
    }

    #[test]
    fn missing_files_returns_empty_vec() {
        let plan = r#"
#### SUBTASK: No files section
**Capability:** light
**Description:**
This subtask has no files listed.
---
"#;
        let subtasks = extract_subtasks(plan);
        assert_eq!(subtasks.len(), 1);
        assert!(subtasks[0].files.is_empty());
    }

    #[test]
    fn empty_input_returns_empty_vec() {
        let subtasks = extract_subtasks("");
        assert!(subtasks.is_empty());
    }

    #[test]
    fn no_subtask_blocks_returns_empty() {
        let plan = r#"
# Implementation Plan

## Phase 1
Do some things.

## Phase 2
Do more things.
"#;
        let subtasks = extract_subtasks(plan);
        assert!(subtasks.is_empty());
    }

    #[test]
    fn multi_line_descriptions() {
        let plan = r#"
#### SUBTASK: Complex task
**Capability:** heavy
**Description:**
This is a multi-line description that spans
several lines and includes details about
what the subtask should accomplish.

It even has a blank line in between.
** Files **
1 `src/main.rs`
---
"#;
        let subtasks = extract_subtasks(plan);
        assert_eq!(subtasks.len(), 1);
        assert!(subtasks[0].description.contains("multi-line description"));
        assert!(subtasks[0].description.contains("several lines"));
    }

    #[test]
    fn mixed_content_with_subtask_blocks() {
        let plan = r#"
# Plan

## Phase 1: Setup
Do some setup work.

### 5. Subtask Definitions

Some intro text before the subtasks.

#### SUBTASK: First subtask
**Capability:** light
**Description:**
First task description.
---

Some text between subtasks.

#### SUBTASK: Second subtask
**Capability:** heavy
**Description:**
Second task description.
** Files **
1 `src/lib.rs`
---

## Phase 2: Follow-up
More plan content after subtasks.
"#;
        let subtasks = extract_subtasks(plan);
        assert_eq!(subtasks.len(), 2);
        assert_eq!(subtasks[0].title, "First subtask");
        assert_eq!(subtasks[1].title, "Second subtask");
        assert_eq!(subtasks[1].files.len(), 1);
    }

    #[test]
    fn trailing_block_without_delimiter() {
        let plan = r#"
#### SUBTASK: Trailing subtask
**Capability:** standard
**Description:**
This block has no trailing --- delimiter.
** Files **
1 `src/mod.rs`
"#;
        let subtasks = extract_subtasks(plan);
        assert_eq!(subtasks.len(), 1);
        assert_eq!(subtasks[0].title, "Trailing subtask");
        assert!(subtasks[0].description.contains("no trailing"));
        assert_eq!(subtasks[0].files.len(), 1);
        assert_eq!(subtasks[0].files[0], "src/mod.rs");
    }

    #[test]
    fn capability_case_insensitive() {
        let plan = r#"
#### SUBTASK: Light task
**Capability:** Light
**Description:**
Task with uppercase capability.
---

#### SUBTASK: HEAVY task
**Capability:** HEAVY
**Description:**
Task with all caps capability.
---
"#;
        let subtasks = extract_subtasks(plan);
        assert_eq!(subtasks.len(), 2);
        assert_eq!(
            subtasks[0].build_capability,
            Some(RunnerCapability::Light)
        );
        // "HEAVY" lowercased = "heavy" which should parse
        assert_eq!(
            subtasks[1].build_capability,
            Some(RunnerCapability::Heavy)
        );
    }

    #[test]
    fn sequential_sort_order() {
        let plan = r#"
#### SUBTASK: First
**Description:**
One.
---
#### SUBTASK: Second
**Description:**
Two.
---
#### SUBTASK: Third
**Description:**
Three.
---
"#;
        let subtasks = extract_subtasks(plan);
        assert_eq!(subtasks.len(), 3);
        assert_eq!(subtasks[0].sort_order, 1.0);
        assert_eq!(subtasks[1].sort_order, 2.0);
        assert_eq!(subtasks[2].sort_order, 3.0);
    }

    #[test]
    fn file_paths_with_various_formats() {
        let plan = r#"
#### SUBTASK: File format test
**Description:**
Test various file list formats.
** Files **
1 `src/main.rs`
2. `src/lib.rs`
- `Cargo.toml`
* `README.md`
`bare.txt`
---
"#;
        let subtasks = extract_subtasks(plan);
        assert_eq!(subtasks.len(), 1);
        assert_eq!(subtasks[0].files.len(), 5);
        assert_eq!(subtasks[0].files[0], "src/main.rs");
        assert_eq!(subtasks[0].files[1], "src/lib.rs");
        assert_eq!(subtasks[0].files[2], "Cargo.toml");
        assert_eq!(subtasks[0].files[3], "README.md");
        assert_eq!(subtasks[0].files[4], "bare.txt");
    }

    #[test]
    fn extract_file_path_helper() {
        assert_eq!(
            extract_file_path("1 `src/main.rs`"),
            Some("src/main.rs".to_string())
        );
        assert_eq!(
            extract_file_path("- `Cargo.toml`"),
            Some("Cargo.toml".to_string())
        );
        assert_eq!(extract_file_path("no backticks here"), None);
        assert_eq!(extract_file_path("``"), None); // empty backticks
    }

    #[test]
    fn description_on_same_line_as_header() {
        let plan = r#"
#### SUBTASK: Inline desc
**Capability:** light
**Description:** This is inline.
---
"#;
        let subtasks = extract_subtasks(plan);
        assert_eq!(subtasks.len(), 1);
        assert_eq!(subtasks[0].description, "This is inline.");
    }
}
