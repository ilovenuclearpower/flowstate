use flowstate_core::verification::{ProfileTemplate, StepTemplate};

pub fn builtin_profiles() -> Vec<ProfileTemplate> {
    vec![
        ProfileTemplate {
            name: "Rust TUI".into(),
            description: "Standard Rust verification: check, test, clippy".into(),
            steps: vec![
                StepTemplate {
                    name: "Check".into(),
                    command: "cargo check --workspace".into(),
                    working_dir: None,
                    timeout_s: 300,
                },
                StepTemplate {
                    name: "Test".into(),
                    command: "cargo test --workspace".into(),
                    working_dir: None,
                    timeout_s: 300,
                },
                StepTemplate {
                    name: "Clippy".into(),
                    command: "cargo clippy --workspace -- -D warnings".into(),
                    working_dir: None,
                    timeout_s: 300,
                },
            ],
        },
        ProfileTemplate {
            name: "Rust Backend".into(),
            description: "Rust backend verification: check, test, clippy".into(),
            steps: vec![
                StepTemplate {
                    name: "Check".into(),
                    command: "cargo check --workspace".into(),
                    working_dir: None,
                    timeout_s: 300,
                },
                StepTemplate {
                    name: "Test".into(),
                    command: "cargo test --workspace".into(),
                    working_dir: None,
                    timeout_s: 300,
                },
                StepTemplate {
                    name: "Clippy".into(),
                    command: "cargo clippy --workspace -- -D warnings".into(),
                    working_dir: None,
                    timeout_s: 300,
                },
            ],
        },
        ProfileTemplate {
            name: "Vite Frontend".into(),
            description: "Vite/Node verification: typecheck, test, lint, build".into(),
            steps: vec![
                StepTemplate {
                    name: "Typecheck".into(),
                    command: "npx tsc --noEmit".into(),
                    working_dir: None,
                    timeout_s: 120,
                },
                StepTemplate {
                    name: "Test".into(),
                    command: "npm test".into(),
                    working_dir: None,
                    timeout_s: 300,
                },
                StepTemplate {
                    name: "Lint".into(),
                    command: "npm run lint".into(),
                    working_dir: None,
                    timeout_s: 120,
                },
                StepTemplate {
                    name: "Build".into(),
                    command: "npm run build".into(),
                    working_dir: None,
                    timeout_s: 300,
                },
            ],
        },
        ProfileTemplate {
            name: "NixOS".into(),
            description: "Nix verification: build, flake check".into(),
            steps: vec![
                StepTemplate {
                    name: "Build".into(),
                    command: "nix build".into(),
                    working_dir: None,
                    timeout_s: 600,
                },
                StepTemplate {
                    name: "Flake Check".into(),
                    command: "nix flake check".into(),
                    working_dir: None,
                    timeout_s: 600,
                },
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_profiles_count() {
        assert_eq!(builtin_profiles().len(), 4);
    }

    #[test]
    fn test_builtin_profile_names() {
        let profiles = builtin_profiles();
        let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Rust TUI", "Rust Backend", "Vite Frontend", "NixOS"]);
    }

    #[test]
    fn test_builtin_profiles_have_steps() {
        for profile in builtin_profiles() {
            assert!(
                !profile.steps.is_empty(),
                "profile '{}' should have at least one step",
                profile.name
            );
        }
    }

    #[test]
    fn test_builtin_profiles_positive_timeouts() {
        for profile in builtin_profiles() {
            for step in &profile.steps {
                assert!(
                    step.timeout_s > 0,
                    "step '{}' in profile '{}' should have positive timeout",
                    step.name,
                    profile.name
                );
            }
        }
    }
}
