//! Integration tests for HttpService + BlockingHttpService against a real server.
//!
//! Each test spawns an in-process axum server on 127.0.0.1:0 with in-memory SQLite,
//! then exercises the HTTP client layer through the full request/response cycle.

use flowstate_core::project::CreateProject;
use flowstate_core::sprint::CreateSprint;
use flowstate_core::task::{CreateTask, TaskFilter, UpdateTask};
use flowstate_core::task_link::CreateTaskLink;
use flowstate_core::task_pr::CreateTaskPr;
use flowstate_service::{HttpService, TaskService};

async fn spawn_server() -> String {
    let server = flowstate_server::test_helpers::spawn_test_server().await;
    server.base_url
}

fn create_test_project() -> CreateProject {
    CreateProject {
        name: "Test Project".into(),
        slug: "test-project".into(),
        description: "A test project".into(),
        repo_url: String::new(),
    }
}

// ---- Async HttpService tests ----

#[tokio::test]
async fn health_check_via_http() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);
    svc.health_check().await.unwrap();
}

#[tokio::test]
async fn project_crud_via_http() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);

    // Create
    let project = svc.create_project(&create_test_project()).await.unwrap();
    assert_eq!(project.name, "Test Project");
    assert_eq!(project.slug, "test-project");

    // Get
    let fetched = svc.get_project(&project.id).await.unwrap();
    assert_eq!(fetched.id, project.id);

    // Get by slug
    let by_slug = svc.get_project_by_slug("test-project").await.unwrap();
    assert_eq!(by_slug.id, project.id);

    // List
    let all = svc.list_projects().await.unwrap();
    assert_eq!(all.len(), 1);

    // Update
    let updated = svc
        .update_project(
            &project.id,
            &flowstate_core::project::UpdateProject {
                name: Some("Renamed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "Renamed");

    // Delete
    svc.delete_project(&project.id).await.unwrap();
    let all = svc.list_projects().await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn task_crud_via_http() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);
    let project = svc.create_project(&create_test_project()).await.unwrap();

    // Create
    let task = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "My Task".into(),
            description: "desc".into(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .await
        .unwrap();
    assert_eq!(task.title, "My Task");

    // Get
    let fetched = svc.get_task(&task.id).await.unwrap();
    assert_eq!(fetched.id, task.id);

    // List
    let all = svc
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(all.len(), 1);

    // Update
    let updated = svc
        .update_task(
            &task.id,
            &UpdateTask {
                title: Some("Updated".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.title, "Updated");

    // Count by status
    let counts = svc.count_tasks_by_status(&project.id).await.unwrap();
    assert!(!counts.is_empty());

    // List children (none yet)
    let children = svc.list_child_tasks(&task.id).await.unwrap();
    assert!(children.is_empty());

    // Delete
    svc.delete_task(&task.id).await.unwrap();
    let all = svc
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn task_list_with_filters_via_http() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);
    let project = svc.create_project(&create_test_project()).await.unwrap();

    svc.create_task(&CreateTask {
        project_id: project.id.clone(),
        title: "Todo".into(),
        description: String::new(),
        status: flowstate_core::task::Status::Todo,
        priority: flowstate_core::task::Priority::High,
        parent_id: None,
        reviewer: String::new(),
    })
    .await
    .unwrap();

    svc.create_task(&CreateTask {
        project_id: project.id.clone(),
        title: "Done".into(),
        description: String::new(),
        status: flowstate_core::task::Status::Done,
        priority: flowstate_core::task::Priority::Low,
        parent_id: None,
        reviewer: String::new(),
    })
    .await
    .unwrap();

    // Filter by status
    let todo_only = svc
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            status: Some(flowstate_core::task::Status::Todo),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(todo_only.len(), 1);
    assert_eq!(todo_only[0].title, "Todo");

    // Filter by priority
    let high_only = svc
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            priority: Some(flowstate_core::task::Priority::High),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(high_only.len(), 1);

    // Filter with limit
    let limited = svc
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            limit: Some(1),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(limited.len(), 1);
}

#[tokio::test]
async fn sprint_crud_via_http() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);
    let project = svc.create_project(&create_test_project()).await.unwrap();

    // Create
    let sprint = svc
        .create_sprint(&CreateSprint {
            project_id: project.id.clone(),
            name: "Sprint 1".into(),
            goal: "Do things".into(),
            starts_at: None,
            ends_at: None,
        })
        .await
        .unwrap();
    assert_eq!(sprint.name, "Sprint 1");

    // Get
    let fetched = svc.get_sprint(&sprint.id).await.unwrap();
    assert_eq!(fetched.id, sprint.id);

    // List
    let all = svc.list_sprints(&project.id).await.unwrap();
    assert_eq!(all.len(), 1);

    // Update
    let updated = svc
        .update_sprint(
            &sprint.id,
            &flowstate_core::sprint::UpdateSprint {
                name: Some("Sprint 1 Updated".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "Sprint 1 Updated");

    // Delete
    svc.delete_sprint(&sprint.id).await.unwrap();
    let all = svc.list_sprints(&project.id).await.unwrap();
    assert!(all.is_empty());
}

#[tokio::test]
async fn task_links_via_http() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);
    let project = svc.create_project(&create_test_project()).await.unwrap();

    let task1 = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Task 1".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .await
        .unwrap();

    let task2 = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Task 2".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .await
        .unwrap();

    // Create link
    let link = svc
        .create_task_link(&CreateTaskLink {
            source_task_id: task1.id.clone(),
            target_task_id: task2.id.clone(),
            link_type: flowstate_core::task_link::LinkType::Blocks,
        })
        .await
        .unwrap();
    assert_eq!(link.source_task_id, task1.id);

    // List links
    let links = svc.list_task_links(&task1.id).await.unwrap();
    assert_eq!(links.len(), 1);

    // Delete link
    svc.delete_task_link(&link.id).await.unwrap();
    let links = svc.list_task_links(&task1.id).await.unwrap();
    assert!(links.is_empty());
}

#[tokio::test]
async fn task_prs_via_http() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);
    let project = svc.create_project(&create_test_project()).await.unwrap();

    let task = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "PR Task".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .await
        .unwrap();

    // Create PR
    let pr = svc
        .create_task_pr(&CreateTaskPr {
            task_id: task.id.clone(),
            claude_run_id: None,
            pr_url: "https://github.com/org/repo/pull/42".into(),
            pr_number: 42,
            branch_name: "flowstate/test".into(),
        })
        .await
        .unwrap();
    assert_eq!(pr.pr_number, 42);

    // List PRs
    let prs = svc.list_task_prs(&task.id).await.unwrap();
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].pr_number, 42);
}

#[tokio::test]
async fn claude_run_lifecycle_via_http() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);
    let project = svc.create_project(&create_test_project()).await.unwrap();

    let task = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Run Task".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .await
        .unwrap();

    // Trigger a research run (no prerequisites)
    let run = svc
        .trigger_claude_run(&task.id, "research")
        .await
        .unwrap();
    assert_eq!(run.task_id, task.id);
    assert_eq!(run.action, flowstate_core::claude_run::ClaudeAction::Research);

    // Get run
    let fetched = svc.get_claude_run(&run.id).await.unwrap();
    assert_eq!(fetched.id, run.id);

    // List runs
    let runs = svc.list_claude_runs(&task.id).await.unwrap();
    assert_eq!(runs.len(), 1);
}

#[tokio::test]
async fn spec_plan_research_verification_roundtrip() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);
    let project = svc.create_project(&create_test_project()).await.unwrap();

    let task = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Content Task".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .await
        .unwrap();

    // Spec
    svc.write_task_spec(&task.id, "# Spec Content").await.unwrap();
    let spec = svc.read_task_spec(&task.id).await.unwrap();
    assert_eq!(spec, "# Spec Content");

    // Plan
    svc.write_task_plan(&task.id, "# Plan Content").await.unwrap();
    let plan = svc.read_task_plan(&task.id).await.unwrap();
    assert_eq!(plan, "# Plan Content");

    // Research
    svc.write_task_research(&task.id, "# Research Content").await.unwrap();
    let research = svc.read_task_research(&task.id).await.unwrap();
    assert_eq!(research, "# Research Content");

    // Verification
    svc.write_task_verification(&task.id, "# Verification Content").await.unwrap();
    let verification = svc.read_task_verification(&task.id).await.unwrap();
    assert_eq!(verification, "# Verification Content");
}

#[tokio::test]
async fn runner_registration_and_claim() {
    let url = spawn_server().await;
    let mut svc = HttpService::new(&url);
    svc.set_runner_id("test-runner-1".into());

    let project = svc.create_project(&create_test_project()).await.unwrap();
    let task = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Claim Task".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .await
        .unwrap();

    // Register runner
    svc.register_runner("test-runner-1", "claude-cli", "standard")
        .await
        .unwrap();

    // No queued runs → None
    let claimed = svc.claim_claude_run().await.unwrap();
    assert!(claimed.is_none());

    // Trigger a run, then claim it
    let run = svc
        .trigger_claude_run(&task.id, "research")
        .await
        .unwrap();
    let claimed = svc.claim_claude_run().await.unwrap();
    assert!(claimed.is_some());
    assert_eq!(claimed.unwrap().id, run.id);

    // Update status
    let updated = svc
        .update_claude_run_status(&run.id, "completed", None, Some(0))
        .await
        .unwrap();
    assert_eq!(
        updated.status,
        flowstate_core::claude_run::ClaudeRunStatus::Completed
    );

    // Update progress on a new run
    let run2 = svc
        .trigger_claude_run(&task.id, "research")
        .await
        .unwrap();
    let claimed2 = svc.claim_claude_run().await.unwrap().unwrap();
    svc.update_claude_run_progress(&claimed2.id, "Working on it...")
        .await
        .unwrap();
    // Verify the run still exists
    let fetched = svc.get_claude_run(&run2.id).await.unwrap();
    assert_eq!(fetched.id, run2.id);
}

#[tokio::test]
async fn repo_token_roundtrip() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);
    let project = svc.create_project(&create_test_project()).await.unwrap();

    // Set repo token
    svc.set_repo_token(&project.id, "ghp_fake_token_12345")
        .await
        .unwrap();

    // Get repo token (decrypted)
    let token = svc.get_repo_token(&project.id).await.unwrap();
    assert_eq!(token, "ghp_fake_token_12345");
}

#[tokio::test]
async fn system_status_via_http() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);
    let status = svc.system_status().await.unwrap();
    assert_eq!(status.server, "ok");
}

#[tokio::test]
async fn update_claude_run_pr_via_http() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);
    let project = svc.create_project(&create_test_project()).await.unwrap();
    let task = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "PR Run Task".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .await
        .unwrap();

    let run = svc
        .trigger_claude_run(&task.id, "research")
        .await
        .unwrap();

    // Claim the run first (sets it to running)
    let mut claiming_svc = HttpService::new(&url);
    claiming_svc.set_runner_id("pr-runner".into());
    claiming_svc
        .register_runner("pr-runner", "claude-cli", "standard")
        .await
        .unwrap();
    let _ = claiming_svc.claim_claude_run().await.unwrap();

    // Update with PR info
    let updated = svc
        .update_claude_run_pr(
            &run.id,
            Some("https://github.com/org/repo/pull/99"),
            Some(99),
            Some("flowstate/my-branch"),
        )
        .await
        .unwrap();
    assert_eq!(
        updated.status,
        flowstate_core::claude_run::ClaudeRunStatus::Completed
    );
}

#[tokio::test]
async fn error_responses_via_http() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);

    // 404 NotFound
    let err = svc
        .get_project("00000000-0000-0000-0000-000000000000")
        .await
        .unwrap_err();
    assert!(
        matches!(err, flowstate_service::ServiceError::NotFound(_)),
        "expected NotFound, got: {err:?}"
    );

    // 404 on nonexistent task
    let err = svc
        .get_task("00000000-0000-0000-0000-000000000000")
        .await
        .unwrap_err();
    assert!(matches!(err, flowstate_service::ServiceError::NotFound(_)));

    // Create a project+task, then try an invalid action trigger (400)
    let project = svc.create_project(&create_test_project()).await.unwrap();
    let task = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Error Task".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .await
        .unwrap();

    // Design without approved research → InvalidInput
    let err = svc
        .trigger_claude_run(&task.id, "design")
        .await
        .unwrap_err();
    assert!(
        matches!(err, flowstate_service::ServiceError::InvalidInput(_)),
        "expected InvalidInput, got: {err:?}"
    );
}

#[tokio::test]
async fn auth_headers_propagation() {
    let url = spawn_server().await;

    // with_api_key constructor
    let svc = HttpService::with_api_key(&url, "fake-key-123".into());
    // The server has no auth, so this should still work
    svc.health_check().await.unwrap();
    let projects = svc.list_projects().await.unwrap();
    assert!(projects.is_empty());

    // set_runner_id
    let mut svc = HttpService::new(&url);
    svc.set_runner_id("runner-abc".into());
    svc.health_check().await.unwrap();
}

#[tokio::test]
async fn list_attachments_via_http() {
    let url = spawn_server().await;
    let svc = HttpService::new(&url);
    let project = svc.create_project(&create_test_project()).await.unwrap();
    let task = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Attachment Task".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .await
        .unwrap();

    // No attachments yet
    let attachments = svc.list_attachments(&task.id).await.unwrap();
    assert!(attachments.is_empty());
}

// ---- Blocking HttpService tests ----

// BlockingHttpService creates its own tokio runtime, so we must
// spawn the server on a separate thread to avoid nested runtime panics.

fn spawn_blocking_server() -> String {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let server = flowstate_server::test_helpers::spawn_test_server().await;
            tx.send(server.base_url.clone()).unwrap();
            // Keep the server alive
            std::future::pending::<()>().await;
        });
    });
    rx.recv().unwrap()
}

#[test]
fn blocking_project_crud() {
    let url = spawn_blocking_server();
    let svc = flowstate_service::BlockingHttpService::new(&url);

    // Health check
    svc.health_check().unwrap();

    // Create
    let project = svc.create_project(&create_test_project()).unwrap();
    assert_eq!(project.name, "Test Project");

    // Get
    let fetched = svc.get_project(&project.id).unwrap();
    assert_eq!(fetched.id, project.id);

    // Get by slug
    let by_slug = svc.get_project_by_slug("test-project").unwrap();
    assert_eq!(by_slug.id, project.id);

    // List
    let all = svc.list_projects().unwrap();
    assert_eq!(all.len(), 1);

    // Update
    let updated = svc
        .update_project(
            &project.id,
            &flowstate_core::project::UpdateProject {
                name: Some("Blocking Renamed".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.name, "Blocking Renamed");

    // Delete
    svc.delete_project(&project.id).unwrap();
    assert!(svc.list_projects().unwrap().is_empty());
}

#[test]
fn blocking_task_crud() {
    let url = spawn_blocking_server();
    let svc = flowstate_service::BlockingHttpService::new(&url);

    let project = svc.create_project(&create_test_project()).unwrap();
    let task = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Blocking Task".into(),
            description: "desc".into(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .unwrap();

    let fetched = svc.get_task(&task.id).unwrap();
    assert_eq!(fetched.title, "Blocking Task");

    let updated = svc
        .update_task(
            &task.id,
            &UpdateTask {
                title: Some("Updated Blocking".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.title, "Updated Blocking");

    let all = svc
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(all.len(), 1);

    let counts = svc.count_tasks_by_status(&project.id).unwrap();
    assert!(!counts.is_empty());

    let children = svc.list_child_tasks(&task.id).unwrap();
    assert!(children.is_empty());

    svc.delete_task(&task.id).unwrap();
}

#[test]
fn blocking_sprint_crud() {
    let url = spawn_blocking_server();
    let svc = flowstate_service::BlockingHttpService::new(&url);

    let project = svc.create_project(&create_test_project()).unwrap();

    let sprint = svc
        .create_sprint(&CreateSprint {
            project_id: project.id.clone(),
            name: "Blocking Sprint".into(),
            goal: "goals".into(),
            starts_at: None,
            ends_at: None,
        })
        .unwrap();

    let fetched = svc.get_sprint(&sprint.id).unwrap();
    assert_eq!(fetched.name, "Blocking Sprint");

    let all = svc.list_sprints(&project.id).unwrap();
    assert_eq!(all.len(), 1);

    let updated = svc
        .update_sprint(
            &sprint.id,
            &flowstate_core::sprint::UpdateSprint {
                name: Some("Updated".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.name, "Updated");

    svc.delete_sprint(&sprint.id).unwrap();
}

#[test]
fn blocking_task_links_and_prs() {
    let url = spawn_blocking_server();
    let svc = flowstate_service::BlockingHttpService::new(&url);

    let project = svc.create_project(&create_test_project()).unwrap();

    let task1 = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "T1".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .unwrap();

    let task2 = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "T2".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .unwrap();

    // Links
    let link = svc
        .create_task_link(&CreateTaskLink {
            source_task_id: task1.id.clone(),
            target_task_id: task2.id.clone(),
            link_type: flowstate_core::task_link::LinkType::Blocks,
        })
        .unwrap();

    let links = svc.list_task_links(&task1.id).unwrap();
    assert_eq!(links.len(), 1);

    svc.delete_task_link(&link.id).unwrap();

    // PRs
    let pr = svc
        .create_task_pr(&CreateTaskPr {
            task_id: task1.id.clone(),
            claude_run_id: None,
            pr_url: "https://github.com/org/repo/pull/1".into(),
            pr_number: 1,
            branch_name: "flowstate/test".into(),
        })
        .unwrap();
    assert_eq!(pr.pr_number, 1);

    let prs = svc.list_task_prs(&task1.id).unwrap();
    assert_eq!(prs.len(), 1);
}

#[test]
fn blocking_claude_run_and_content() {
    let url = spawn_blocking_server();
    let svc = flowstate_service::BlockingHttpService::new(&url);

    let project = svc.create_project(&create_test_project()).unwrap();
    let task = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Blocking Run".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .unwrap();

    // Claude run
    let run = svc.trigger_claude_run(&task.id, "research").unwrap();
    let fetched = svc.get_claude_run(&run.id).unwrap();
    assert_eq!(fetched.id, run.id);

    let runs = svc.list_claude_runs(&task.id).unwrap();
    assert_eq!(runs.len(), 1);

    // Content roundtrip
    svc.write_task_spec(&task.id, "blocking spec").unwrap();
    assert_eq!(svc.read_task_spec(&task.id).unwrap(), "blocking spec");

    svc.write_task_plan(&task.id, "blocking plan").unwrap();
    assert_eq!(svc.read_task_plan(&task.id).unwrap(), "blocking plan");

    svc.write_task_research(&task.id, "blocking research").unwrap();
    assert_eq!(svc.read_task_research(&task.id).unwrap(), "blocking research");

    svc.write_task_verification(&task.id, "blocking verify").unwrap();
    assert_eq!(svc.read_task_verification(&task.id).unwrap(), "blocking verify");

    // Repo token
    svc.set_repo_token(&project.id, "ghp_blocking_token").unwrap();
    assert_eq!(svc.get_repo_token(&project.id).unwrap(), "ghp_blocking_token");

    // System status
    let status = svc.system_status().unwrap();
    assert_eq!(status.server, "ok");

    // Attachments
    let attachments = svc.list_attachments(&task.id).unwrap();
    assert!(attachments.is_empty());
}

#[test]
fn blocking_with_api_key() {
    let url = spawn_blocking_server();
    let svc = flowstate_service::BlockingHttpService::with_api_key(&url, "fake-key".into());
    // No auth configured on test server, so this should work
    svc.health_check().unwrap();
}

#[test]
fn blocking_claude_run_output_not_found() {
    let url = spawn_blocking_server();
    let svc = flowstate_service::BlockingHttpService::new(&url);

    let project = svc.create_project(&create_test_project()).unwrap();
    let task = svc
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Output Task".into(),
            description: String::new(),
            status: flowstate_core::task::Status::Todo,
            priority: flowstate_core::task::Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        })
        .unwrap();

    let run = svc.trigger_claude_run(&task.id, "research").unwrap();
    // Output not yet available → error
    let err = svc.get_claude_run_output(&run.id).unwrap_err();
    assert!(matches!(err, flowstate_service::ServiceError::NotFound(_)));
}
