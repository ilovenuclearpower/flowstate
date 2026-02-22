use std::path::Path;

use chrono::Utc;
use flowstate_core::verification::VerificationStep;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[derive(Debug, Clone)]
pub struct Runner;

#[derive(Debug)]
pub struct StepResult {
    pub step_name: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub started_at: chrono::DateTime<Utc>,
    pub finished_at: chrono::DateTime<Utc>,
}

#[derive(Debug)]
pub enum RunStatus {
    Passed,
    Failed,
    Error,
}

#[derive(Debug)]
pub struct RunResult {
    pub status: RunStatus,
    pub steps: Vec<StepResult>,
}

impl Runner {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute(
        &self,
        steps: &[VerificationStep],
        working_dir: &Path,
    ) -> RunResult {
        let mut results = Vec::new();

        for step in steps {
            let dir = step
                .working_dir
                .as_ref()
                .map(|d| working_dir.join(d))
                .unwrap_or_else(|| working_dir.to_path_buf());

            let started_at = Utc::now();

            let result = timeout(
                Duration::from_secs(step.timeout_s as u64),
                run_command(&step.command, &dir),
            )
            .await;

            let finished_at = Utc::now();

            let step_result = match result {
                Ok(Ok(output)) => StepResult {
                    step_name: step.name.clone(),
                    command: step.command.clone(),
                    exit_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    started_at,
                    finished_at,
                },
                Ok(Err(e)) => StepResult {
                    step_name: step.name.clone(),
                    command: step.command.clone(),
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("Process error: {e}"),
                    started_at,
                    finished_at,
                },
                Err(_) => StepResult {
                    step_name: step.name.clone(),
                    command: step.command.clone(),
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("Timeout after {}s", step.timeout_s),
                    started_at,
                    finished_at,
                },
            };

            let failed = step_result.exit_code != Some(0);
            results.push(step_result);

            if failed {
                break;
            }
        }

        let status = if results.iter().all(|r| r.exit_code == Some(0)) {
            RunStatus::Passed
        } else {
            RunStatus::Failed
        };

        RunResult { status, steps: results }
    }
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

async fn run_command(command: &str, dir: &Path) -> std::io::Result<std::process::Output> {
    Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .output()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowstate_core::verification::VerificationStep;

    fn make_step(name: &str, command: &str) -> VerificationStep {
        VerificationStep {
            id: uuid::Uuid::new_v4().to_string(),
            profile_id: String::new(),
            name: name.into(),
            command: command.into(),
            working_dir: None,
            sort_order: 0,
            timeout_s: 30,
            created_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_runner_execute_passing_step() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = Runner::new();
        let steps = vec![make_step("pass", "true")];
        let result = runner.execute(&steps, tmp.path()).await;
        assert!(matches!(result.status, RunStatus::Passed));
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_runner_execute_failing_step() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = Runner::new();
        let steps = vec![make_step("fail", "false")];
        let result = runner.execute(&steps, tmp.path()).await;
        assert!(matches!(result.status, RunStatus::Failed));
        assert_eq!(result.steps.len(), 1);
        assert_eq!(result.steps[0].exit_code, Some(1));
    }

    #[tokio::test]
    async fn test_runner_execute_multiple_passing_steps() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = Runner::new();
        let steps = vec![
            make_step("step1", "true"),
            make_step("step2", "true"),
        ];
        let result = runner.execute(&steps, tmp.path()).await;
        assert!(matches!(result.status, RunStatus::Passed));
        assert_eq!(result.steps.len(), 2);
    }
}
