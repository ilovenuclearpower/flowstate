use flowstate_core::verification::VerificationStep;

/// Parse a PLAN.md to extract validation commands from the
/// "### 4. Validation Steps" section.
///
/// Looks for fenced code blocks and indented code lines that
/// appear to be shell commands under the validation heading.
pub fn extract_validation_commands(plan_content: &str) -> Vec<VerificationStep> {
    let mut steps = Vec::new();
    let mut in_validation_section = false;
    let mut in_code_block = false;
    let mut code_block_content = String::new();
    let mut step_index: i32 = 0;

    for line in plan_content.lines() {
        let trimmed = line.trim();

        // Detect section headings
        if trimmed.starts_with("### 4.") || trimmed.starts_with("## 4.") {
            in_validation_section = true;
            continue;
        }

        // A new section at same or higher level ends the validation section
        if in_validation_section
            && (trimmed.starts_with("### ") || trimmed.starts_with("## "))
            && !trimmed.contains("4.")
        {
            // Flush any open code block
            if in_code_block {
                flush_code_block(&code_block_content, &mut steps, &mut step_index);
                code_block_content.clear();
                in_code_block = false;
            }
            in_validation_section = false;
            continue;
        }

        if !in_validation_section {
            continue;
        }

        // Handle fenced code blocks
        if trimmed.starts_with("```") {
            if in_code_block {
                // End of code block
                flush_code_block(&code_block_content, &mut steps, &mut step_index);
                code_block_content.clear();
                in_code_block = false;
            } else {
                // Start of code block
                in_code_block = true;
                code_block_content.clear();
            }
            continue;
        }

        if in_code_block {
            code_block_content.push_str(line);
            code_block_content.push('\n');
            continue;
        }

        // Inline backtick commands: `cargo test`, `npm run lint`
        if let Some(cmd) = extract_inline_command(trimmed) {
            if looks_like_command(&cmd) {
                steps.push(make_step(&cmd, step_index));
                step_index += 1;
            }
        }
    }

    // Flush any trailing code block
    if in_code_block && !code_block_content.is_empty() {
        flush_code_block(&code_block_content, &mut steps, &mut step_index);
    }

    steps
}

fn flush_code_block(content: &str, steps: &mut Vec<VerificationStep>, index: &mut i32) {
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && looks_like_command(trimmed) {
            steps.push(make_step(trimmed, *index));
            *index += 1;
        }
    }
}

fn extract_inline_command(line: &str) -> Option<String> {
    // Match `command here` patterns
    let start = line.find('`')?;
    let rest = &line[start + 1..];
    let end = rest.find('`')?;
    let cmd = rest[..end].trim().to_string();
    if cmd.is_empty() {
        return None;
    }
    Some(cmd)
}

fn looks_like_command(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    // Strip leading $ or #
    let s = s.strip_prefix("$ ").unwrap_or(s);
    let s = s.strip_prefix("# ").unwrap_or(s);

    let cmd_prefixes = [
        "cargo ", "npm ", "npx ", "yarn ", "pnpm ",
        "make", "pytest", "go ", "python ", "ruby ",
        "mix ", "dotnet ", "mvn ", "gradle ",
        "sh ", "bash ", "./", "docker ",
    ];

    cmd_prefixes.iter().any(|p| s.starts_with(p))
        || s.starts_with("cargo")
        || s.starts_with("make")
        || s.starts_with("pytest")
}

fn make_step(command: &str, index: i32) -> VerificationStep {
    // Strip leading $ prompt
    let command = command
        .strip_prefix("$ ")
        .unwrap_or(command)
        .trim()
        .to_string();

    VerificationStep {
        id: uuid::Uuid::new_v4().to_string(),
        profile_id: String::new(),
        name: format!("validation-{}", index + 1),
        command,
        working_dir: None,
        sort_order: index,
        timeout_s: 300,
        created_at: chrono::Utc::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_from_code_block() {
        let plan = r#"
### 4. Validation Steps

Run the following:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

### 5. Next Steps
"#;
        let steps = extract_validation_commands(plan);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].command, "cargo build --workspace");
        assert_eq!(steps[1].command, "cargo test --workspace");
        assert_eq!(steps[2].command, "cargo clippy --workspace -- -D warnings");
    }

    #[test]
    fn test_extract_inline_backticks() {
        let plan = r#"
### 4. Validation Steps

- Run `cargo test` to verify
- Run `npm run lint` for linting
- Check output manually

### 5. Done
"#;
        let steps = extract_validation_commands(plan);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].command, "cargo test");
        assert_eq!(steps[1].command, "npm run lint");
    }

    #[test]
    fn test_empty_plan() {
        let steps = extract_validation_commands("# Some Plan\n\nNo validation section here.");
        assert!(steps.is_empty());
    }

    #[test]
    fn test_extract_dollar_prefix_command() {
        let plan = r#"
### 4. Validation Steps

```bash
$ cargo test --workspace
$ cargo build
```

### 5. Done
"#;
        let steps = extract_validation_commands(plan);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].command, "cargo test --workspace");
        assert_eq!(steps[1].command, "cargo build");
    }

    #[test]
    fn test_section_heading_level_two() {
        let plan = r#"
## 4. Validation Steps

```bash
cargo test
```

## 5. Done
"#;
        let steps = extract_validation_commands(plan);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].command, "cargo test");
    }

    #[test]
    fn test_non_command_content_ignored() {
        let plan = r#"
### 4. Validation Steps

- The `some_variable` should be set
- Check `config_value` in the output

### 5. Done
"#;
        let steps = extract_validation_commands(plan);
        assert!(steps.is_empty(), "non-command backtick content should be skipped");
    }
}
