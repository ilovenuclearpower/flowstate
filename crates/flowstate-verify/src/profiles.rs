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
    fn builtin_profiles_count() {
        assert_eq!(builtin_profiles().len(), 4);
    }

    #[test]
    fn profile_names() {
        let profiles = builtin_profiles();
        let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Rust TUI", "Rust Backend", "Vite Frontend", "NixOS"]
        );
    }

    #[test]
    fn rust_tui_steps() {
        let profiles = builtin_profiles();
        let steps = &profiles[0].steps;
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].name, "Check");
        assert_eq!(steps[1].name, "Test");
        assert_eq!(steps[2].name, "Clippy");
    }

    #[test]
    fn vite_frontend_steps() {
        let profiles = builtin_profiles();
        let steps = &profiles[2].steps;
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0].name, "Typecheck");
        assert_eq!(steps[1].name, "Test");
        assert_eq!(steps[2].name, "Lint");
        assert_eq!(steps[3].name, "Build");
    }

    #[test]
    fn nixos_steps() {
        let profiles = builtin_profiles();
        let steps = &profiles[3].steps;
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].name, "Build");
        assert_eq!(steps[1].name, "Flake Check");
    }

    #[test]
    fn all_profiles_have_steps() {
        for profile in builtin_profiles() {
            assert!(
                !profile.steps.is_empty(),
                "profile '{}' should have at least one step",
                profile.name
            );
        }
    }

    #[test]
    fn all_steps_have_timeouts() {
        for profile in builtin_profiles() {
            for step in &profile.steps {
                assert!(
                    step.timeout_s > 0,
                    "Step '{}' in '{}' has timeout_s <= 0",
                    step.name,
                    profile.name
                );
            }
        }
    }
}
