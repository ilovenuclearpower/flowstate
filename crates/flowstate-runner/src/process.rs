use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::{info, warn};

use crate::executor::ClaudeOutput;

/// A child process managed within its own process group.
/// Enables killing the entire process tree (including any orphaned children
/// that inherit pipe file descriptors).
pub struct ManagedChild {
    child: tokio::process::Child,
    pgid: i32,
}

impl ManagedChild {
    /// Kill the entire process group.
    /// Sends SIGTERM first, waits grace_period, then SIGKILL.
    pub async fn kill_group(&mut self, grace_period: Duration) {
        // SIGTERM to process group (negative PID = process group)
        if let Err(e) = kill(Pid::from_raw(-self.pgid), Signal::SIGTERM) {
            // ESRCH means the process already exited — not an error
            if e != nix::errno::Errno::ESRCH {
                warn!("SIGTERM to process group {} failed: {e}", self.pgid);
            }
            return;
        }

        // Wait for graceful shutdown
        match tokio::time::timeout(grace_period, self.child.wait()).await {
            Ok(_) => (), // exited cleanly after SIGTERM
            Err(_) => {
                // SIGKILL the process group
                if let Err(e) = kill(Pid::from_raw(-self.pgid), Signal::SIGKILL) {
                    if e != nix::errno::Errno::ESRCH {
                        warn!("SIGKILL to process group {} failed: {e}", self.pgid);
                    }
                }
                let _ = self.child.wait().await;
            }
        }
    }
}

/// Spawn Claude CLI in a new process group via setsid.
/// Returns the managed child and its stdout/stderr handles.
fn spawn_claude_managed(
    prompt: &str,
    work_dir: &Path,
) -> Result<(ManagedChild, tokio::process::ChildStdout, tokio::process::ChildStderr)> {
    let mut cmd = Command::new("claude");
    cmd.arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("text")
        .arg("--dangerously-skip-permissions")
        .current_dir(work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Create new process group via setsid so we can kill the whole tree
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let mut child = cmd.spawn().context("spawn claude")?;
    let pid = child
        .id()
        .ok_or_else(|| anyhow::anyhow!("no child PID"))? as i32;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    Ok((ManagedChild { child, pgid: pid }, stdout, stderr))
}

/// Run Claude CLI with a timeout. If the timeout fires, kills the entire
/// process group (SIGTERM → grace period → SIGKILL) and returns an error.
pub async fn run_claude_with_timeout(
    prompt: &str,
    work_dir: &Path,
    timeout_duration: Duration,
    kill_grace: Duration,
) -> Result<ClaudeOutput> {
    let (mut managed, mut stdout, mut stderr) = spawn_claude_managed(prompt, work_dir)?;

    let result = tokio::time::timeout(timeout_duration, async {
        let mut stdout_bytes = Vec::new();
        let mut stderr_bytes = Vec::new();

        let (stdout_res, stderr_res, status) = tokio::try_join!(
            async { stdout.read_to_end(&mut stdout_bytes).await },
            async { stderr.read_to_end(&mut stderr_bytes).await },
            managed.child.wait()
        )?;

        let _ = stdout_res;
        let _ = stderr_res;
        Ok::<_, anyhow::Error>((stdout_bytes, stderr_bytes, status))
    })
    .await;

    match result {
        Ok(Ok((stdout_bytes, stderr_bytes, status))) => {
            let stdout_str = String::from_utf8_lossy(&stdout_bytes).to_string();
            let stderr_str = String::from_utf8_lossy(&stderr_bytes).to_string();
            let exit_code = status.code().unwrap_or(-1);

            // Save output
            let run_dir = work_dir.join(".flowstate-output");
            let _ = std::fs::create_dir_all(&run_dir);
            let _ = std::fs::write(run_dir.join("output.txt"), &stdout_str);

            Ok(ClaudeOutput {
                success: status.success(),
                stdout: stdout_str,
                stderr: stderr_str,
                exit_code,
            })
        }
        Ok(Err(e)) => Err(e),
        Err(_elapsed) => {
            // TIMEOUT — kill the process group
            info!(
                "claude process timed out after {:?}, killing process group",
                timeout_duration
            );
            managed.kill_group(kill_grace).await;
            Err(anyhow::anyhow!(
                "claude process timed out after {:?}",
                timeout_duration
            ))
        }
    }
}
