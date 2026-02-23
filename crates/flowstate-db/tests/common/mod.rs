// Backend-agnostic integration tests for the Database trait.
//
// Each public async function accepts `&dyn Database` so that the same logic
// can be exercised against both the SQLite and Postgres backends.

use flowstate_core::claude_run::{ClaudeAction, ClaudeRunStatus, CreateClaudeRun};
use flowstate_core::project::{CreateProject, UpdateProject};
use flowstate_core::runner::RunnerCapability;
use flowstate_core::sprint::{CreateSprint, SprintStatus, UpdateSprint};
use flowstate_core::task::{CreateTask, Priority, Status, TaskFilter, UpdateTask};
use flowstate_core::task_link::{CreateTaskLink, LinkType};
use flowstate_core::task_pr::CreateTaskPr;
use flowstate_db::Database;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_project(slug: &str) -> CreateProject {
    CreateProject {
        name: slug.to_string(),
        slug: slug.to_string(),
        description: String::new(),
        repo_url: String::new(),
    }
}

fn make_task(project_id: &str, title: &str) -> CreateTask {
    CreateTask {
        project_id: project_id.to_string(),
        title: title.to_string(),
        description: String::new(),
        status: Status::Todo,
        priority: Priority::Medium,
        parent_id: None,
        reviewer: String::new(),
        research_capability: None,
        design_capability: None,
        plan_capability: None,
        build_capability: None,
        verify_capability: None,
    }
}

// ---------------------------------------------------------------------------
// Project tests
// ---------------------------------------------------------------------------

/// Test basic project CRUD: create, get, get_by_slug, list, update, delete.
pub async fn test_project_crud(db: &dyn Database) {
    let p = db
        .create_project(&CreateProject {
            name: "Test".into(),
            slug: "test-proj".into(),
            description: "desc".into(),
            repo_url: "https://example.com".into(),
        })
        .await
        .unwrap();
    assert_eq!(p.name, "Test");
    assert_eq!(p.slug, "test-proj");
    assert_eq!(p.description, "desc");
    assert_eq!(p.repo_url, "https://example.com");

    // get by id
    let fetched = db.get_project(&p.id).await.unwrap();
    assert_eq!(fetched.id, p.id);

    // get by slug
    let by_slug = db.get_project_by_slug("test-proj").await.unwrap();
    assert_eq!(by_slug.id, p.id);

    // list
    let all = db.list_projects().await.unwrap();
    assert_eq!(all.len(), 1);

    // update
    let updated = db
        .update_project(
            &p.id,
            &UpdateProject {
                name: Some("Updated".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "Updated");
    // unchanged fields preserved
    assert_eq!(updated.slug, "test-proj");

    // delete
    db.delete_project(&p.id).await.unwrap();
    assert!(db.list_projects().await.unwrap().is_empty());

    // get non-existent should return error
    assert!(db.get_project(&p.id).await.is_err());
}

// ---------------------------------------------------------------------------
// Task tests
// ---------------------------------------------------------------------------

/// Test basic task CRUD: create, get, update, list, delete.
pub async fn test_task_crud(db: &dyn Database) {
    let project = db.create_project(&make_project("task-crud")).await.unwrap();

    let task = db
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Task 1".into(),
            description: "do something".into(),
            status: Status::Todo,
            priority: Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
            research_capability: Some(RunnerCapability::Light),
            design_capability: None,
            plan_capability: None,
            build_capability: Some(RunnerCapability::Heavy),
            verify_capability: None,
        })
        .await
        .unwrap();
    assert_eq!(task.title, "Task 1");
    assert_eq!(task.status, Status::Todo);
    assert_eq!(task.priority, Priority::Medium);
    assert_eq!(task.research_capability, Some(RunnerCapability::Light));
    assert_eq!(task.build_capability, Some(RunnerCapability::Heavy));

    // get
    let fetched = db.get_task(&task.id).await.unwrap();
    assert_eq!(fetched.id, task.id);
    assert_eq!(fetched.research_capability, Some(RunnerCapability::Light));
    assert_eq!(fetched.build_capability, Some(RunnerCapability::Heavy));

    // update
    let updated = db
        .update_task(
            &task.id,
            &UpdateTask {
                status: Some(Status::Build),
                priority: Some(Priority::High),
                research_capability: Some(None), // unset
                build_capability: Some(Some(RunnerCapability::Standard)), // change
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.status, Status::Build);
    assert_eq!(updated.priority, Priority::High);
    assert_eq!(updated.research_capability, None);
    assert_eq!(updated.build_capability, Some(RunnerCapability::Standard));

    // list with filter
    let tasks = db
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(tasks.len(), 1);

    // delete
    db.delete_task(&task.id).await.unwrap();
    let tasks = db
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(tasks.is_empty());

    // get non-existent should error
    assert!(db.get_task(&task.id).await.is_err());
}

/// Test task filtering by status and priority.
pub async fn test_task_filtering(db: &dyn Database) {
    let project = db
        .create_project(&make_project("task-filter"))
        .await
        .unwrap();

    for i in 0..5 {
        db.create_task(&CreateTask {
            project_id: project.id.clone(),
            title: format!("Task {i}"),
            description: String::new(),
            status: if i < 3 { Status::Todo } else { Status::Done },
            priority: if i == 0 {
                Priority::High
            } else {
                Priority::Medium
            },
            parent_id: None,
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
        })
        .await
        .unwrap();
    }

    // all tasks in project
    let all = db
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(all.len(), 5);

    // filter by status
    let todos = db
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            status: Some(Status::Todo),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(todos.len(), 3);

    let done = db
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            status: Some(Status::Done),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(done.len(), 2);

    // filter by priority
    let high = db
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            priority: Some(Priority::High),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(high.len(), 1);

    // limit
    let limited = db
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            limit: Some(2),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(limited.len(), 2);
}

/// Test that sort_order auto-increments when creating tasks.
pub async fn test_task_sort_order(db: &dyn Database) {
    let project = db
        .create_project(&make_project("sort-order"))
        .await
        .unwrap();

    let t1 = db
        .create_task(&make_task(&project.id, "First"))
        .await
        .unwrap();
    let t2 = db
        .create_task(&make_task(&project.id, "Second"))
        .await
        .unwrap();
    let t3 = db
        .create_task(&make_task(&project.id, "Third"))
        .await
        .unwrap();

    assert!(t2.sort_order > t1.sort_order);
    assert!(t3.sort_order > t2.sort_order);
}

/// Test count_tasks_by_status.
pub async fn test_count_by_status(db: &dyn Database) {
    let project = db
        .create_project(&make_project("count-status"))
        .await
        .unwrap();

    db.create_task(&CreateTask {
        project_id: project.id.clone(),
        title: "A".into(),
        description: String::new(),
        status: Status::Todo,
        priority: Priority::Medium,
        parent_id: None,
        reviewer: String::new(),
        research_capability: None,
        design_capability: None,
        plan_capability: None,
        build_capability: None,
        verify_capability: None,
    })
    .await
    .unwrap();
    db.create_task(&CreateTask {
        project_id: project.id.clone(),
        title: "B".into(),
        description: String::new(),
        status: Status::Todo,
        priority: Priority::Medium,
        parent_id: None,
        reviewer: String::new(),
        research_capability: None,
        design_capability: None,
        plan_capability: None,
        build_capability: None,
        verify_capability: None,
    })
    .await
    .unwrap();
    db.create_task(&CreateTask {
        project_id: project.id.clone(),
        title: "C".into(),
        description: String::new(),
        status: Status::Done,
        priority: Priority::Medium,
        parent_id: None,
        reviewer: String::new(),
        research_capability: None,
        design_capability: None,
        plan_capability: None,
        build_capability: None,
        verify_capability: None,
    })
    .await
    .unwrap();

    let counts = db.count_tasks_by_status(&project.id).await.unwrap();
    let todo_count = counts.iter().find(|(s, _)| s == "todo").map(|(_, c)| *c);
    let done_count = counts.iter().find(|(s, _)| s == "done").map(|(_, c)| *c);
    assert_eq!(todo_count, Some(2));
    assert_eq!(done_count, Some(1));
}

/// Test parent/child task relationships via list_child_tasks.
pub async fn test_child_tasks(db: &dyn Database) {
    let project = db
        .create_project(&make_project("child-tasks"))
        .await
        .unwrap();

    let parent = db
        .create_task(&make_task(&project.id, "Parent"))
        .await
        .unwrap();

    let child1 = db
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Child 1".into(),
            description: String::new(),
            status: Status::Todo,
            priority: Priority::Medium,
            parent_id: Some(parent.id.clone()),
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
        })
        .await
        .unwrap();
    assert_eq!(child1.parent_id.as_deref(), Some(parent.id.as_str()));

    let child2 = db
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Child 2".into(),
            description: String::new(),
            status: Status::Todo,
            priority: Priority::Medium,
            parent_id: Some(parent.id.clone()),
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
        })
        .await
        .unwrap();

    let children = db.list_child_tasks(&parent.id).await.unwrap();
    assert_eq!(children.len(), 2);
    let child_ids: Vec<&str> = children.iter().map(|t| t.id.as_str()).collect();
    assert!(child_ids.contains(&child1.id.as_str()));
    assert!(child_ids.contains(&child2.id.as_str()));

    // A task with no children returns empty
    let no_children = db.list_child_tasks(&child1.id).await.unwrap();
    assert!(no_children.is_empty());

    // Filtering by parent_id = None (top-level only) should give only the parent
    let top_level = db
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            parent_id: Some(None), // only top-level
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(top_level.len(), 1);
    assert_eq!(top_level[0].id, parent.id);
}

// ---------------------------------------------------------------------------
// Claude run tests
// ---------------------------------------------------------------------------

/// Test the full claude run lifecycle: create -> claim -> running -> completed.
pub async fn test_claude_run_lifecycle(db: &dyn Database) {
    let project = db
        .create_project(&make_project("run-lifecycle"))
        .await
        .unwrap();
    let task = db
        .create_task(&make_task(&project.id, "Run task"))
        .await
        .unwrap();

    // Create a run
    let run = db
        .create_claude_run(&CreateClaudeRun {
            task_id: task.id.clone(),
            action: ClaudeAction::Build,
            required_capability: None,
        })
        .await
        .unwrap();
    assert_eq!(run.status, ClaudeRunStatus::Queued);
    assert_eq!(run.action, ClaudeAction::Build);
    assert!(run.error_message.is_none());
    assert!(run.exit_code.is_none());
    assert!(run.finished_at.is_none());
    assert!(run.runner_id.is_none());

    // Get by id
    let fetched = db.get_claude_run(&run.id).await.unwrap();
    assert_eq!(fetched.id, run.id);

    // List runs for task
    let runs = db.list_claude_runs_for_task(&task.id).await.unwrap();
    assert_eq!(runs.len(), 1);

    // Claim the run (Queued -> Running)
    let claimed = db.claim_next_claude_run(&[]).await.unwrap().unwrap();
    assert_eq!(claimed.id, run.id);
    assert_eq!(claimed.status, ClaudeRunStatus::Running);

    // Set runner_id
    db.set_claude_run_runner(&run.id, "runner-42")
        .await
        .unwrap();
    let with_runner = db.get_claude_run(&run.id).await.unwrap();
    assert_eq!(with_runner.runner_id.as_deref(), Some("runner-42"));

    // Update progress
    db.update_claude_run_progress(&run.id, "compiling...")
        .await
        .unwrap();
    let with_progress = db.get_claude_run(&run.id).await.unwrap();
    assert_eq!(
        with_progress.progress_message.as_deref(),
        Some("compiling...")
    );

    // Update PR info
    let with_pr = db
        .update_claude_run_pr(
            &run.id,
            Some("https://github.com/org/repo/pull/99"),
            Some(99),
            Some("flowstate/feature"),
        )
        .await
        .unwrap();
    assert_eq!(
        with_pr.pr_url.as_deref(),
        Some("https://github.com/org/repo/pull/99")
    );
    assert_eq!(with_pr.pr_number, Some(99));
    assert_eq!(with_pr.branch_name.as_deref(), Some("flowstate/feature"));

    // Complete the run
    let completed = db
        .update_claude_run_status(&run.id, ClaudeRunStatus::Completed, None, Some(0))
        .await
        .unwrap();
    assert_eq!(completed.status, ClaudeRunStatus::Completed);
    assert!(completed.finished_at.is_some());
    assert_eq!(completed.exit_code, Some(0));

    // Failed run sets finished_at and error_message
    let run2 = db
        .create_claude_run(&CreateClaudeRun {
            task_id: task.id.clone(),
            action: ClaudeAction::Plan,
            required_capability: None,
        })
        .await
        .unwrap();
    let failed = db
        .update_claude_run_status(
            &run2.id,
            ClaudeRunStatus::Failed,
            Some("segfault"),
            Some(139),
        )
        .await
        .unwrap();
    assert_eq!(failed.status, ClaudeRunStatus::Failed);
    assert!(failed.finished_at.is_some());
    assert_eq!(failed.error_message.as_deref(), Some("segfault"));
    assert_eq!(failed.exit_code, Some(139));
}

/// Test that claim_next_claude_run returns None when no queued runs exist.
pub async fn test_claim_empty(db: &dyn Database) {
    let result = db.claim_next_claude_run(&[]).await.unwrap();
    assert!(result.is_none());
}

/// Test find_stale_running_runs and timeout_claude_run.
pub async fn test_stale_runs(db: &dyn Database) {
    let project = db
        .create_project(&make_project("stale-runs"))
        .await
        .unwrap();
    let task = db
        .create_task(&make_task(&project.id, "Stale task"))
        .await
        .unwrap();

    // Create and claim a run to set it Running
    let run = db
        .create_claude_run(&CreateClaudeRun {
            task_id: task.id.clone(),
            action: ClaudeAction::Build,
            required_capability: None,
        })
        .await
        .unwrap();
    let _claimed = db.claim_next_claude_run(&[]).await.unwrap().unwrap();

    // With a threshold in the future, the run should be considered stale
    let future = chrono::Utc::now() + chrono::Duration::hours(1);
    let stale = db.find_stale_running_runs(future).await.unwrap();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].id, run.id);

    // With a threshold in the past, no runs should match
    let past = chrono::Utc::now() - chrono::Duration::hours(1);
    let not_stale = db.find_stale_running_runs(past).await.unwrap();
    assert!(not_stale.is_empty());

    // Timeout the running run
    let timed = db
        .timeout_claude_run(&run.id, "watchdog timeout")
        .await
        .unwrap();
    assert!(timed.is_some());
    let timed = timed.unwrap();
    assert_eq!(timed.status, ClaudeRunStatus::TimedOut);
    assert_eq!(timed.error_message.as_deref(), Some("watchdog timeout"));
    assert!(timed.finished_at.is_some());

    // Timeout on an already-timed-out run returns None
    let again = db
        .timeout_claude_run(&run.id, "second timeout")
        .await
        .unwrap();
    assert!(again.is_none());

    // After timing out, find_stale_running_runs should return empty
    let stale_after = db.find_stale_running_runs(future).await.unwrap();
    assert!(stale_after.is_empty());

    // Test find_stale_salvaging_runs
    let run2 = db
        .create_claude_run(&CreateClaudeRun {
            task_id: task.id.clone(),
            action: ClaudeAction::Build,
            required_capability: None,
        })
        .await
        .unwrap();
    let _claimed2 = db.claim_next_claude_run(&[]).await.unwrap().unwrap();
    // Transition to Salvaging
    db.update_claude_run_status(&run2.id, ClaudeRunStatus::Salvaging, None, None)
        .await
        .unwrap();

    let salvaging = db.find_stale_salvaging_runs(future).await.unwrap();
    assert_eq!(salvaging.len(), 1);
    assert_eq!(salvaging[0].id, run2.id);

    // Timeout the salvaging run too
    let timed2 = db
        .timeout_claude_run(&run2.id, "salvage timeout")
        .await
        .unwrap();
    assert!(timed2.is_some());
    assert_eq!(timed2.unwrap().status, ClaudeRunStatus::TimedOut);
}

// ---------------------------------------------------------------------------
// Task link tests
// ---------------------------------------------------------------------------

/// Test task link CRUD: create, list, delete.
pub async fn test_task_links(db: &dyn Database) {
    let project = db
        .create_project(&make_project("task-links"))
        .await
        .unwrap();
    let t1 = db
        .create_task(&make_task(&project.id, "Source"))
        .await
        .unwrap();
    let t2 = db
        .create_task(&make_task(&project.id, "Target"))
        .await
        .unwrap();

    let link = db
        .create_task_link(&CreateTaskLink {
            source_task_id: t1.id.clone(),
            target_task_id: t2.id.clone(),
            link_type: LinkType::Blocks,
        })
        .await
        .unwrap();
    assert_eq!(link.source_task_id, t1.id);
    assert_eq!(link.target_task_id, t2.id);
    assert_eq!(link.link_type, LinkType::Blocks);

    // list from source side
    let from_source = db.list_task_links(&t1.id).await.unwrap();
    assert_eq!(from_source.len(), 1);

    // list from target side
    let from_target = db.list_task_links(&t2.id).await.unwrap();
    assert_eq!(from_target.len(), 1);

    // delete
    db.delete_task_link(&link.id).await.unwrap();
    let after_delete = db.list_task_links(&t1.id).await.unwrap();
    assert!(after_delete.is_empty());

    // delete non-existent should error
    assert!(db.delete_task_link(&link.id).await.is_err());
}

// ---------------------------------------------------------------------------
// Task PR tests
// ---------------------------------------------------------------------------

/// Test task PR CRUD: create and list.
pub async fn test_task_prs(db: &dyn Database) {
    let project = db.create_project(&make_project("task-prs")).await.unwrap();
    let task = db
        .create_task(&make_task(&project.id, "PR task"))
        .await
        .unwrap();

    // Create a run so we can associate it
    let run = db
        .create_claude_run(&CreateClaudeRun {
            task_id: task.id.clone(),
            action: ClaudeAction::Build,
            required_capability: None,
        })
        .await
        .unwrap();

    let pr = db
        .create_task_pr(&CreateTaskPr {
            task_id: task.id.clone(),
            claude_run_id: Some(run.id.clone()),
            pr_url: "https://github.com/owner/repo/pull/42".into(),
            pr_number: 42,
            branch_name: "flowstate/my-feature".into(),
        })
        .await
        .unwrap();
    assert_eq!(pr.task_id, task.id);
    assert_eq!(pr.claude_run_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(pr.pr_url, "https://github.com/owner/repo/pull/42");
    assert_eq!(pr.pr_number, 42);
    assert_eq!(pr.branch_name, "flowstate/my-feature");

    // list
    let prs = db.list_task_prs(&task.id).await.unwrap();
    assert_eq!(prs.len(), 1);
    assert_eq!(prs[0].id, pr.id);

    // Multiple PRs per task
    db.create_task_pr(&CreateTaskPr {
        task_id: task.id.clone(),
        claude_run_id: None,
        pr_url: "https://github.com/owner/repo/pull/43".into(),
        pr_number: 43,
        branch_name: "flowstate/second-feature".into(),
    })
    .await
    .unwrap();
    let prs = db.list_task_prs(&task.id).await.unwrap();
    assert_eq!(prs.len(), 2);

    // Duplicate pr_url should be idempotent (INSERT OR IGNORE / ON CONFLICT)
    let dup = db
        .create_task_pr(&CreateTaskPr {
            task_id: task.id.clone(),
            claude_run_id: None,
            pr_url: "https://github.com/owner/repo/pull/42".into(),
            pr_number: 42,
            branch_name: "flowstate/my-feature".into(),
        })
        .await
        .unwrap();
    assert_eq!(dup.id, pr.id);
    let prs = db.list_task_prs(&task.id).await.unwrap();
    assert_eq!(prs.len(), 2);
}

// ---------------------------------------------------------------------------
// Attachment tests
// ---------------------------------------------------------------------------

/// Test attachment CRUD: create, list, get, delete.
pub async fn test_attachments(db: &dyn Database) {
    let project = db
        .create_project(&make_project("attachments"))
        .await
        .unwrap();
    let task = db
        .create_task(&make_task(&project.id, "Attach task"))
        .await
        .unwrap();

    let att = db
        .create_attachment(&task.id, "screenshot.png", "s3://bucket/key", 12345)
        .await
        .unwrap();
    assert_eq!(att.task_id, task.id);
    assert_eq!(att.filename, "screenshot.png");
    assert_eq!(att.store_key, "s3://bucket/key");
    assert_eq!(att.size_bytes, 12345);

    // get
    let fetched = db.get_attachment(&att.id).await.unwrap();
    assert_eq!(fetched.id, att.id);

    // list
    let list = db.list_attachments(&task.id).await.unwrap();
    assert_eq!(list.len(), 1);

    // create another
    db.create_attachment(&task.id, "log.txt", "s3://bucket/key2", 100)
        .await
        .unwrap();
    let list = db.list_attachments(&task.id).await.unwrap();
    assert_eq!(list.len(), 2);

    // delete returns the deleted attachment
    let deleted = db.delete_attachment(&att.id).await.unwrap();
    assert_eq!(deleted.id, att.id);

    // After delete, only one remains
    let list = db.list_attachments(&task.id).await.unwrap();
    assert_eq!(list.len(), 1);

    // get non-existent should error
    assert!(db.get_attachment(&att.id).await.is_err());

    // delete non-existent should error
    assert!(db.delete_attachment(&att.id).await.is_err());
}

// ---------------------------------------------------------------------------
// API key tests
// ---------------------------------------------------------------------------

/// Test API key CRUD: insert, find_by_hash, has, touch, list, delete.
pub async fn test_api_keys(db: &dyn Database) {
    // Initially no keys
    assert!(!db.has_api_keys().await.unwrap());
    let keys = db.list_api_keys().await.unwrap();
    assert!(keys.is_empty());

    // Insert
    let key = db.insert_api_key("test-key", "hash_abc").await.unwrap();
    assert_eq!(key.name, "test-key");
    assert_eq!(key.key_hash, "hash_abc");
    assert!(key.last_used_at.is_none());

    // has_api_keys
    assert!(db.has_api_keys().await.unwrap());

    // find_by_hash
    let found = db.find_api_key_by_hash("hash_abc").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, key.id);

    // not found
    let missing = db.find_api_key_by_hash("nonexistent").await.unwrap();
    assert!(missing.is_none());

    // touch
    db.touch_api_key(&key.id).await.unwrap();
    let touched = db.find_api_key_by_hash("hash_abc").await.unwrap().unwrap();
    assert!(touched.last_used_at.is_some());

    // list
    let keys = db.list_api_keys().await.unwrap();
    assert_eq!(keys.len(), 1);

    // Insert a second key
    db.insert_api_key("key-two", "hash_def").await.unwrap();
    let keys = db.list_api_keys().await.unwrap();
    assert_eq!(keys.len(), 2);

    // delete
    db.delete_api_key(&key.id).await.unwrap();
    let keys = db.list_api_keys().await.unwrap();
    assert_eq!(keys.len(), 1);

    // delete non-existent should error
    assert!(db.delete_api_key(&key.id).await.is_err());
}

// ---------------------------------------------------------------------------
// Sprint tests
// ---------------------------------------------------------------------------

/// Test sprint CRUD: create, get, list, update, delete.
pub async fn test_sprint_crud(db: &dyn Database) {
    let project = db
        .create_project(&make_project("sprint-crud"))
        .await
        .unwrap();

    // Create
    let sprint = db
        .create_sprint(&CreateSprint {
            project_id: project.id.clone(),
            name: "Sprint 1".into(),
            goal: "Ship MVP".into(),
            starts_at: None,
            ends_at: None,
        })
        .await
        .unwrap();
    assert_eq!(sprint.name, "Sprint 1");
    assert_eq!(sprint.goal, "Ship MVP");
    assert_eq!(sprint.status, SprintStatus::Planned);
    assert_eq!(sprint.project_id, project.id);

    // Get by id
    let fetched = db.get_sprint(&sprint.id).await.unwrap();
    assert_eq!(fetched.id, sprint.id);
    assert_eq!(fetched.name, "Sprint 1");

    // List
    let sprints = db.list_sprints(&project.id).await.unwrap();
    assert_eq!(sprints.len(), 1);

    // Create another
    db.create_sprint(&CreateSprint {
        project_id: project.id.clone(),
        name: "Sprint 2".into(),
        goal: String::new(),
        starts_at: None,
        ends_at: None,
    })
    .await
    .unwrap();
    let sprints = db.list_sprints(&project.id).await.unwrap();
    assert_eq!(sprints.len(), 2);

    // Update
    let updated = db
        .update_sprint(
            &sprint.id,
            &UpdateSprint {
                name: Some("Sprint 1 (renamed)".into()),
                status: Some(SprintStatus::Active),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "Sprint 1 (renamed)");
    assert_eq!(updated.status, SprintStatus::Active);
    assert_eq!(updated.goal, "Ship MVP"); // unchanged

    // Delete
    db.delete_sprint(&sprint.id).await.unwrap();
    let sprints = db.list_sprints(&project.id).await.unwrap();
    assert_eq!(sprints.len(), 1);

    // Get non-existent should error
    assert!(db.get_sprint(&sprint.id).await.is_err());
}

// ---------------------------------------------------------------------------
// Subtask workflow tests
// ---------------------------------------------------------------------------

/// Test subtask creation with parent context and sprint assignment via tasks.
pub async fn test_subtask_workflow(db: &dyn Database) {
    let project = db
        .create_project(&make_project("subtask-wf"))
        .await
        .unwrap();

    // Create a sprint and a parent task assigned to it
    let sprint = db
        .create_sprint(&CreateSprint {
            project_id: project.id.clone(),
            name: "Sprint A".into(),
            goal: String::new(),
            starts_at: None,
            ends_at: None,
        })
        .await
        .unwrap();

    let parent = db
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Parent Feature".into(),
            description: "Big feature".into(),
            status: Status::Build,
            priority: Priority::High,
            parent_id: None,
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
        })
        .await
        .unwrap();

    // Assign parent to sprint
    let parent = db
        .update_task(
            &parent.id,
            &UpdateTask {
                sprint_id: Some(Some(sprint.id.clone())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(parent.sprint_id.as_deref(), Some(sprint.id.as_str()));

    // Create subtask inheriting project_id from parent
    let subtask = db
        .create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Subtask 1".into(),
            description: String::new(),
            status: Status::Todo,
            priority: Priority::High,
            parent_id: Some(parent.id.clone()),
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
        })
        .await
        .unwrap();
    assert_eq!(subtask.parent_id.as_deref(), Some(parent.id.as_str()));
    assert!(subtask.is_subtask());
    assert!(!parent.is_subtask());

    // List children of parent
    let children = db.list_child_tasks(&parent.id).await.unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, subtask.id);

    // Filter tasks by sprint_id should return parent only (subtask not assigned to sprint)
    let sprint_tasks = db
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            sprint_id: Some(sprint.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(sprint_tasks.len(), 1);
    assert_eq!(sprint_tasks[0].id, parent.id);
}

// ---------------------------------------------------------------------------
// Edge case tests
// ---------------------------------------------------------------------------

/// Test that update_task with default (no-op) UpdateTask returns the task unchanged.
pub async fn test_update_task_no_changes(db: &dyn Database) {
    let project = db
        .create_project(&make_project("update-noop"))
        .await
        .unwrap();
    let task = db
        .create_task(&make_task(&project.id, "Unchanged"))
        .await
        .unwrap();

    let updated = db
        .update_task(&task.id, &UpdateTask::default())
        .await
        .unwrap();
    assert_eq!(updated.title, "Unchanged");
    assert_eq!(updated.status, Status::Todo);
    assert_eq!(updated.priority, Priority::Medium);
}

/// Test filtering tasks by status + priority + limit simultaneously.
pub async fn test_list_tasks_combined_filters(db: &dyn Database) {
    let project = db
        .create_project(&make_project("combined-filter"))
        .await
        .unwrap();

    // Create 3 high-priority Todo tasks and 2 medium-priority Todo tasks
    for i in 0..3 {
        db.create_task(&CreateTask {
            project_id: project.id.clone(),
            title: format!("High {i}"),
            description: String::new(),
            status: Status::Todo,
            priority: Priority::High,
            parent_id: None,
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
        })
        .await
        .unwrap();
    }
    for i in 0..2 {
        db.create_task(&CreateTask {
            project_id: project.id.clone(),
            title: format!("Med {i}"),
            description: String::new(),
            status: Status::Todo,
            priority: Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
        })
        .await
        .unwrap();
    }

    // Filter: status=Todo, priority=High, limit=2
    let results = db
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            status: Some(Status::Todo),
            priority: Some(Priority::High),
            limit: Some(2),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    for t in &results {
        assert_eq!(t.priority, Priority::High);
    }
}

/// Test update_project with all fields set at once.
pub async fn test_update_project_all_fields(db: &dyn Database) {
    let project = db
        .create_project(&make_project("update-all"))
        .await
        .unwrap();

    let updated = db
        .update_project(
            &project.id,
            &UpdateProject {
                name: Some("New Name".into()),
                description: Some("New desc".into()),
                repo_url: Some("https://new-url.com".into()),
                repo_token: Some("tok_123".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "New Name");
    assert_eq!(updated.description, "New desc");
    assert_eq!(updated.repo_url, "https://new-url.com");
}

/// Test update_project with default (no-op) returns project unchanged.
pub async fn test_update_project_no_changes(db: &dyn Database) {
    let project = db.create_project(&make_project("proj-noop")).await.unwrap();

    let updated = db
        .update_project(&project.id, &UpdateProject::default())
        .await
        .unwrap();
    assert_eq!(updated.name, project.name);
    assert_eq!(updated.slug, project.slug);
}

/// Test get_project_by_slug with non-existent slug returns error.
pub async fn test_get_project_by_slug_not_found(db: &dyn Database) {
    let result = db.get_project_by_slug("nonexistent-slug-xyz").await;
    assert!(result.is_err());
}

/// Test filtering tasks by sprint_id returns only assigned tasks.
pub async fn test_task_filter_by_sprint_id(db: &dyn Database) {
    let project = db
        .create_project(&make_project("sprint-filter"))
        .await
        .unwrap();
    let sprint = db
        .create_sprint(&CreateSprint {
            project_id: project.id.clone(),
            name: "Sprint X".into(),
            goal: String::new(),
            starts_at: None,
            ends_at: None,
        })
        .await
        .unwrap();

    // Task assigned to sprint
    let t1 = db
        .create_task(&make_task(&project.id, "In Sprint"))
        .await
        .unwrap();
    db.update_task(
        &t1.id,
        &UpdateTask {
            sprint_id: Some(Some(sprint.id.clone())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Task not assigned to sprint
    let _t2 = db
        .create_task(&make_task(&project.id, "No Sprint"))
        .await
        .unwrap();

    let filtered = db
        .list_tasks(&TaskFilter {
            project_id: Some(project.id.clone()),
            sprint_id: Some(sprint.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].title, "In Sprint");
}

/// Test update_sprint with default (no-op) returns sprint unchanged.
pub async fn test_update_sprint_no_changes(db: &dyn Database) {
    let project = db
        .create_project(&make_project("sprint-noop"))
        .await
        .unwrap();
    let sprint = db
        .create_sprint(&CreateSprint {
            project_id: project.id.clone(),
            name: "Original".into(),
            goal: "Goal".into(),
            starts_at: None,
            ends_at: None,
        })
        .await
        .unwrap();

    let updated = db
        .update_sprint(&sprint.id, &UpdateSprint::default())
        .await
        .unwrap();
    assert_eq!(updated.name, "Original");
    assert_eq!(updated.goal, "Goal");
    assert_eq!(updated.status, SprintStatus::Planned);
}
