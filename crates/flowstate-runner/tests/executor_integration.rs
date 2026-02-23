//! Integration tests for executor::dispatch() with a mock backend and real test server.
//!
//! Each test spawns an in-process axum server, creates project+task+run via HttpService,
//! and calls dispatch() with a MockBackend to verify the full executor flow.

use std::path::PathBuf;

use flowstate_core::claude_run::ClaudeRunStatus;
use flowstate_core::task::ApprovalStatus;
use flowstate_runner::backend::mock::MockBackend;
use flowstate_runner::config::RunnerConfig;
use flowstate_runner::executor;
use flowstate_service::{HttpService, TaskService};

async fn spawn_server() -> String {
    let server = flowstate_server::test_helpers::spawn_test_server().await;
    server.base_url
}

/// Create a bare git repo with an initial commit, return its file:// URL.
fn create_test_repo(dir: &std::path::Path) -> String {
    let bare = dir.join("test-repo.git");
    std::fs::create_dir_all(&bare).unwrap();
    std::process::Command::new("git")
        .args(["init", "--bare"])
        .current_dir(&bare)
        .output()
        .unwrap();

    // Clone, add initial commit, push
    let work = dir.join("init-work");
    std::process::Command::new("git")
        .args(["clone", bare.to_str().unwrap(), work.to_str().unwrap()])
        .output()
        .unwrap();

    // Configure git user for commits
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&work)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&work)
        .output()
        .unwrap();

    std::fs::write(work.join("README.md"), "# Test Repo\n").unwrap();
    std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(&work)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(&work)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["push", "origin", "HEAD"])
        .current_dir(&work)
        .output()
        .unwrap();

    format!("file://{}", bare.display())
}

fn test_config(workspace_root: PathBuf) -> RunnerConfig {
    RunnerConfig {
        server_url: String::new(),
        api_key: None,
        poll_interval: 5,
        workspace_root: Some(workspace_root),
        health_port: 0,
        light_timeout: 30,
        build_timeout: 60,
        kill_grace_period: 2,
        activity_timeout: 900,
        max_concurrent: 1,
        max_builds: 1,
        shutdown_timeout: 10,
        agent_backend: "mock".into(),
        runner_capability: "heavy".into(),
        anthropic_base_url: None,
        anthropic_auth_token: None,
        anthropic_model: None,
        opencode_provider: None,
        opencode_model: None,
        opencode_api_key: None,
        opencode_base_url: None,
        gemini_api_key: None,
        gemini_model: None,
        gemini_gcp_project: None,
        gemini_gcp_location: None,
    }
}

/// Helper: create a project, task, and claimed run. Returns (project, task, run).
async fn setup_run(
    svc: &mut HttpService,
    repo_url: &str,
    action: &str,
) -> (
    flowstate_core::project::Project,
    flowstate_core::task::Task,
    flowstate_core::claude_run::ClaudeRun,
) {
    let project = svc
        .create_project(&flowstate_core::project::CreateProject {
            name: "Test".into(),
            slug: format!("test-{}", uuid::Uuid::new_v4()),
            description: String::new(),
            repo_url: repo_url.to_string(),
        })
        .await
        .unwrap();

    let task = svc
        .create_task(&flowstate_core::task::CreateTask {
            project_id: project.id.clone(),
            title: "Test Task".into(),
            description: "A test task".into(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .await
        .unwrap();

    let run = svc.trigger_claude_run(&task.id, action).await.unwrap();

    // Claim the run
    svc.set_runner_id("test-runner".into());
    svc.register_runner("test-runner", "mock", "standard")
        .await
        .unwrap();
    let claimed = svc.claim_claude_run().await.unwrap().unwrap();
    assert_eq!(claimed.id, run.id);

    (project, task, claimed)
}

// ---- resolve_workspace_dir tests ----

#[test]
fn resolve_workspace_dir_with_root() {
    let root = PathBuf::from("/tmp/workspaces");
    let dir = executor::resolve_workspace_dir(&Some(root.clone()), "run-123");
    assert_eq!(dir, root.join("run-123"));
}

#[test]
fn resolve_workspace_dir_with_xdg() {
    let dir = executor::resolve_workspace_dir_from(
        &None,
        "run-456",
        Some("/tmp/xdg-data-test".into()),
        None,
    );
    assert_eq!(
        dir,
        PathBuf::from("/tmp/xdg-data-test/flowstate/workspaces/run-456")
    );
}

#[test]
fn resolve_workspace_dir_with_home() {
    let dir = executor::resolve_workspace_dir_from(
        &None,
        "run-789",
        None,
        Some(PathBuf::from("/home/testuser")),
    );
    assert_eq!(
        dir,
        PathBuf::from("/home/testuser/.local/share/flowstate/workspaces/run-789")
    );
}

#[test]
fn resolve_workspace_dir_no_env() {
    let dir = executor::resolve_workspace_dir_from(&None, "run-000", None, None);
    assert_eq!(
        dir,
        PathBuf::from("./flowstate/workspaces/run-000")
    );
}

#[test]
fn cleanup_workspace_nonexistent() {
    // Should not panic when dir doesn't exist
    executor::cleanup_workspace(std::path::Path::new("/tmp/nonexistent-flowstate-ws"));
}

#[test]
fn cleanup_workspace_removes_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("test-ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("file.txt"), "content").unwrap();
    assert!(ws.exists());

    executor::cleanup_workspace(&ws);
    assert!(!ws.exists());
}

// ---- Executor dispatch integration tests ----

#[tokio::test]
async fn dispatch_research_success() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_url = create_test_repo(tmp.path());
    let url = spawn_server().await;
    let mut svc = HttpService::new(&url);
    let (project, task, run) = setup_run(&mut svc, &repo_url, "research").await;

    let ws_root = tmp.path().join("workspaces");
    let config = test_config(ws_root);
    let backend = MockBackend::success("research output")
        .with_files(vec![("RESEARCH.md", "# Research Findings\n\nGood stuff.")]);

    executor::dispatch(&svc, &run, &task, &project, &config, &backend)
        .await
        .unwrap();

    // Verify run completed
    let updated_run = svc.get_claude_run(&run.id).await.unwrap();
    assert_eq!(updated_run.status, ClaudeRunStatus::Completed);

    // Verify research content was stored
    let research = svc.read_task_research(&task.id).await.unwrap();
    assert!(research.contains("Research Findings"));

    // Verify task research_status set to Pending
    let updated_task = svc.get_task(&task.id).await.unwrap();
    assert_eq!(updated_task.research_status, ApprovalStatus::Pending);
}

#[tokio::test]
async fn dispatch_research_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_url = create_test_repo(tmp.path());
    let url = spawn_server().await;
    let mut svc = HttpService::new(&url);
    let (project, task, run) = setup_run(&mut svc, &repo_url, "research").await;

    let ws_root = tmp.path().join("workspaces");
    let config = test_config(ws_root);
    let backend = MockBackend::failure("agent crashed", 1);

    executor::dispatch(&svc, &run, &task, &project, &config, &backend)
        .await
        .unwrap();

    // Verify run marked as failed
    let updated_run = svc.get_claude_run(&run.id).await.unwrap();
    assert_eq!(updated_run.status, ClaudeRunStatus::Failed);
}

#[tokio::test]
async fn dispatch_design_success() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_url = create_test_repo(tmp.path());
    let url = spawn_server().await;
    let mut svc = HttpService::new(&url);

    // Design needs approved research first — but trigger_claude_run doesn't enforce
    // for research action, so we trigger research directly
    let (project, task, run) = setup_run(&mut svc, &repo_url, "research").await;

    // Complete research first
    let ws_root = tmp.path().join("workspaces");
    let config = test_config(ws_root.clone());
    let research_backend = MockBackend::success("research output")
        .with_files(vec![("RESEARCH.md", "# Research\n\nDone.")]);
    executor::dispatch(&svc, &run, &task, &project, &config, &research_backend)
        .await
        .unwrap();

    // Approve research
    svc.update_task(
        &task.id,
        &flowstate_core::task::UpdateTask {
            research_status: Some(ApprovalStatus::Approved),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Now trigger design
    let design_run = svc.trigger_claude_run(&task.id, "design").await.unwrap();
    let claimed = svc.claim_claude_run().await.unwrap().unwrap();
    assert_eq!(claimed.id, design_run.id);

    let design_backend = MockBackend::success("spec output")
        .with_files(vec![("SPECIFICATION.md", "# Spec\n\nThe spec.")]);
    executor::dispatch(&svc, &claimed, &task, &project, &config, &design_backend)
        .await
        .unwrap();

    // Verify design run completed
    let updated = svc.get_claude_run(&claimed.id).await.unwrap();
    assert_eq!(updated.status, ClaudeRunStatus::Completed);

    // Verify spec stored
    let spec = svc.read_task_spec(&task.id).await.unwrap();
    assert!(spec.contains("The spec"));

    // Verify spec_status = Pending
    let updated_task = svc.get_task(&task.id).await.unwrap();
    assert_eq!(updated_task.spec_status, ApprovalStatus::Pending);
}

#[tokio::test]
async fn dispatch_plan_success() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_url = create_test_repo(tmp.path());
    let url = spawn_server().await;
    let mut svc = HttpService::new(&url);
    let (project, task, run) = setup_run(&mut svc, &repo_url, "research").await;

    let ws_root = tmp.path().join("workspaces");
    let config = test_config(ws_root);

    // Complete research
    let backend = MockBackend::success("ok")
        .with_files(vec![("RESEARCH.md", "# Research")]);
    executor::dispatch(&svc, &run, &task, &project, &config, &backend)
        .await
        .unwrap();

    // Approve research + spec (for plan)
    svc.update_task(
        &task.id,
        &flowstate_core::task::UpdateTask {
            research_status: Some(ApprovalStatus::Approved),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Design
    let _design_run = svc.trigger_claude_run(&task.id, "design").await.unwrap();
    let claimed = svc.claim_claude_run().await.unwrap().unwrap();
    let backend = MockBackend::success("ok")
        .with_files(vec![("SPECIFICATION.md", "# Spec")]);
    executor::dispatch(&svc, &claimed, &task, &project, &config, &backend)
        .await
        .unwrap();

    svc.update_task(
        &task.id,
        &flowstate_core::task::UpdateTask {
            spec_status: Some(ApprovalStatus::Approved),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Plan
    let plan_run = svc.trigger_claude_run(&task.id, "plan").await.unwrap();
    let claimed = svc.claim_claude_run().await.unwrap().unwrap();
    assert_eq!(claimed.id, plan_run.id);

    let backend = MockBackend::success("ok")
        .with_files(vec![("PLAN.md", "# Plan\n\n## Validation\n\n```bash\necho ok\n```")]);
    executor::dispatch(&svc, &claimed, &task, &project, &config, &backend)
        .await
        .unwrap();

    let updated = svc.get_claude_run(&claimed.id).await.unwrap();
    assert_eq!(updated.status, ClaudeRunStatus::Completed);

    let plan = svc.read_task_plan(&task.id).await.unwrap();
    assert!(plan.contains("Validation"));
}

#[tokio::test]
async fn dispatch_research_no_file_uses_stdout() {
    let tmp = tempfile::tempdir().unwrap();
    let repo_url = create_test_repo(tmp.path());
    let url = spawn_server().await;
    let mut svc = HttpService::new(&url);
    let (project, task, run) = setup_run(&mut svc, &repo_url, "research").await;

    let ws_root = tmp.path().join("workspaces");
    let config = test_config(ws_root);
    // Backend succeeds but doesn't write RESEARCH.md — executor falls back to stdout
    let backend = MockBackend::success("stdout research content");

    executor::dispatch(&svc, &run, &task, &project, &config, &backend)
        .await
        .unwrap();

    let research = svc.read_task_research(&task.id).await.unwrap();
    assert_eq!(research, "stdout research content");
}
