pub(crate) mod migrations;
pub mod queries;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::Connection;

use flowstate_core::api_key::ApiKey;
use flowstate_core::attachment::Attachment;
use flowstate_core::claude_run::{ClaudeRun, ClaudeRunStatus, CreateClaudeRun};
use flowstate_core::project::{CreateProject, Project, UpdateProject};
use flowstate_core::sprint::{CreateSprint, Sprint, UpdateSprint};
use flowstate_core::task::{CreateTask, Task, TaskFilter, UpdateTask};
use flowstate_core::task_link::{CreateTaskLink, TaskLink};
use flowstate_core::task_pr::{CreateTaskPr, TaskPr};

use crate::{Database, DbConfig, DbError};

/// Extension trait that converts `rusqlite::Result<T>` into `Result<T, DbError>`.
///
/// Because `DbError` no longer carries a `Sqlite(rusqlite::Error)` variant, every
/// rusqlite call that used plain `?` now needs an explicit mapping.  Calling
/// `.to_db()?` is the shortest way to achieve that inside the query modules.
pub(crate) trait SqliteResultExt<T> {
    fn to_db(self) -> Result<T, DbError>;
}

impl<T> SqliteResultExt<T> for rusqlite::Result<T> {
    fn to_db(self) -> Result<T, DbError> {
        self.map_err(map_sqlite_err)
    }
}

#[derive(Clone)]
pub struct SqliteDatabase {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteDatabase {
    pub fn open(config: &DbConfig) -> Result<Self, DbError> {
        let path = config
            .sqlite_path
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::data_dir().join("flowstate.db"));
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
        Self::open_path(&path)
    }

    pub fn open_path(path: &Path) -> Result<Self, DbError> {
        let conn = Connection::open(path).map_err(|e| DbError::Internal(e.to_string()))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;",
        )
        .map_err(|e| DbError::Internal(e.to_string()))?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.run_migrations()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory().map_err(|e| DbError::Internal(e.to_string()))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| DbError::Internal(e.to_string()))?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.run_migrations()?;
        Ok(db)
    }

    pub fn open_default() -> Result<Self, DbError> {
        let dir = crate::data_dir();
        std::fs::create_dir_all(&dir)?;
        Self::open_path(&dir.join("flowstate.db"))
    }

    pub(crate) fn with_conn<F, T>(&self, f: F) -> Result<T, DbError>
    where
        F: FnOnce(&Connection) -> Result<T, DbError>,
    {
        let conn = self
            .conn
            .lock()
            .map_err(|_| DbError::Internal("lock poisoned".into()))?;
        f(&conn)
    }

    fn run_migrations(&self) -> Result<(), DbError> {
        self.with_conn(|conn| {
            migrations::run(conn)?;
            Ok(())
        })
    }
}

/// Map a `rusqlite::Error` into a `DbError::Internal`.
pub(crate) fn map_sqlite_err(e: rusqlite::Error) -> DbError {
    DbError::Internal(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowstate_core::claude_run::{ClaudeAction, CreateClaudeRun};
    use flowstate_core::project::CreateProject;
    use flowstate_core::sprint::CreateSprint;
    use flowstate_core::task::{CreateTask, Priority, Status, TaskFilter, UpdateTask};
    use flowstate_core::task_link::{CreateTaskLink, LinkType};
    use flowstate_core::task_pr::CreateTaskPr;

    #[test]
    fn open_in_memory_returns_working_db() {
        let db = SqliteDatabase::open_in_memory().unwrap();
        // Verify we can access the connection
        db.with_conn(|conn| {
            let count: i64 = conn
                .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))
                .map_err(|e| DbError::Internal(e.to_string()))?;
            assert!(count > 0); // migrations created tables
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn open_path_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("test.db");
        assert!(!db_path.exists());

        let _db = SqliteDatabase::open_path(&db_path).unwrap();
        assert!(db_path.exists());
    }

    #[test]
    fn map_sqlite_err_produces_internal() {
        let err = map_sqlite_err(rusqlite::Error::QueryReturnedNoRows);
        match err {
            DbError::Internal(msg) => assert!(msg.contains("Query returned no rows")),
            other => panic!("expected Internal, got: {other:?}"),
        }
    }

    #[test]
    fn open_config_with_path() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("configured.db");
        let config = DbConfig {
            backend: "sqlite".into(),
            database_url: None,
            sqlite_path: Some(db_path.to_string_lossy().into()),
        };
        let _db = SqliteDatabase::open(&config).unwrap();
        assert!(db_path.exists());
    }

    // -- Async Database trait wrappers --
    // These exercise the spawn_blocking wrappers in the `impl Database` block.

    #[tokio::test]
    async fn async_project_crud() {
        let db = SqliteDatabase::open_in_memory().unwrap();
        let project = db
            .create_project(&CreateProject {
                name: "Async Test".into(),
                slug: "async-test".into(),
                description: "desc".into(),
                repo_url: String::new(),
            })
            .await
            .unwrap();
        assert_eq!(project.name, "Async Test");

        let fetched = db.get_project(&project.id).await.unwrap();
        assert_eq!(fetched.slug, "async-test");

        let by_slug = db.get_project_by_slug("async-test").await.unwrap();
        assert_eq!(by_slug.id, project.id);

        let all = db.list_projects().await.unwrap();
        assert_eq!(all.len(), 1);

        let updated = db
            .update_project(
                &project.id,
                &UpdateProject {
                    name: Some("Updated".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Updated");

        db.delete_project(&project.id).await.unwrap();
        let all = db.list_projects().await.unwrap();
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn async_task_crud() {
        let db = SqliteDatabase::open_in_memory().unwrap();
        let project = db
            .create_project(&CreateProject {
                name: "P".into(),
                slug: "p".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "Task 1".into(),
                description: "desc".into(),
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
        assert_eq!(task.title, "Task 1");

        let fetched = db.get_task(&task.id).await.unwrap();
        assert_eq!(fetched.id, task.id);

        let tasks = db
            .list_tasks(&TaskFilter {
                project_id: Some(project.id.clone()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(tasks.len(), 1);

        let updated = db
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

        let counts = db.count_tasks_by_status(&project.id).await.unwrap();
        assert!(!counts.is_empty());

        // Child tasks
        let child = db
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "Child".into(),
                description: String::new(),
                status: Status::Todo,
                priority: Priority::Low,
                parent_id: Some(task.id.clone()),
                reviewer: String::new(),
                research_capability: None,
                design_capability: None,
                plan_capability: None,
                build_capability: None,
                verify_capability: None,
            })
            .await
            .unwrap();
        let children = db.list_child_tasks(&task.id).await.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, child.id);

        db.delete_task(&child.id).await.unwrap();
        db.delete_task(&task.id).await.unwrap();
    }

    #[tokio::test]
    async fn async_claude_run_lifecycle() {
        let db = SqliteDatabase::open_in_memory().unwrap();
        let project = db
            .create_project(&CreateProject {
                name: "P".into(),
                slug: "p".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();
        let task = db
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "T".into(),
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

        let run = db
            .create_claude_run(&CreateClaudeRun {
                task_id: task.id.clone(),
                action: ClaudeAction::Build,
                required_capability: None,
            })
            .await
            .unwrap();

        let fetched = db.get_claude_run(&run.id).await.unwrap();
        assert_eq!(fetched.id, run.id);

        let runs = db.list_claude_runs_for_task(&task.id).await.unwrap();
        assert_eq!(runs.len(), 1);

        db.update_claude_run_progress(&run.id, "doing stuff")
            .await
            .unwrap();
        db.set_claude_run_runner(&run.id, "runner-1").await.unwrap();

        let updated = db
            .update_claude_run_pr(
                &run.id,
                Some("https://pr"),
                Some(42),
                Some("feature-branch"),
            )
            .await
            .unwrap();
        assert_eq!(updated.pr_url.as_deref(), Some("https://pr"));

        let completed = db
            .update_claude_run_status(&run.id, ClaudeRunStatus::Completed, None, Some(0))
            .await
            .unwrap();
        assert_eq!(completed.status, ClaudeRunStatus::Completed);
    }

    #[tokio::test]
    async fn async_sprint_crud() {
        let db = SqliteDatabase::open_in_memory().unwrap();
        let project = db
            .create_project(&CreateProject {
                name: "P".into(),
                slug: "p".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();

        let sprint = db
            .create_sprint(&CreateSprint {
                project_id: project.id.clone(),
                name: "Sprint 1".into(),
                goal: "goal".into(),
                starts_at: None,
                ends_at: None,
            })
            .await
            .unwrap();
        assert_eq!(sprint.name, "Sprint 1");

        let fetched = db.get_sprint(&sprint.id).await.unwrap();
        assert_eq!(fetched.id, sprint.id);

        let sprints = db.list_sprints(&project.id).await.unwrap();
        assert_eq!(sprints.len(), 1);

        let updated = db
            .update_sprint(
                &sprint.id,
                &flowstate_core::sprint::UpdateSprint {
                    name: Some("Updated".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Updated");

        db.delete_sprint(&sprint.id).await.unwrap();
        let sprints = db.list_sprints(&project.id).await.unwrap();
        assert!(sprints.is_empty());
    }

    #[tokio::test]
    async fn async_task_link_crud() {
        let db = SqliteDatabase::open_in_memory().unwrap();
        let project = db
            .create_project(&CreateProject {
                name: "P".into(),
                slug: "p".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();
        let t1 = db
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "T1".into(),
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
        let t2 = db
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "T2".into(),
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

        let link = db
            .create_task_link(&CreateTaskLink {
                source_task_id: t1.id.clone(),
                target_task_id: t2.id.clone(),
                link_type: LinkType::Blocks,
            })
            .await
            .unwrap();

        let links = db.list_task_links(&t1.id).await.unwrap();
        assert_eq!(links.len(), 1);

        db.delete_task_link(&link.id).await.unwrap();
        let links = db.list_task_links(&t1.id).await.unwrap();
        assert!(links.is_empty());
    }

    #[tokio::test]
    async fn async_task_pr_crud() {
        let db = SqliteDatabase::open_in_memory().unwrap();
        let project = db
            .create_project(&CreateProject {
                name: "P".into(),
                slug: "p".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();
        let task = db
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "T".into(),
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
                pr_url: "https://pr/1".into(),
                pr_number: 1,
                branch_name: "feature".into(),
            })
            .await
            .unwrap();
        assert_eq!(pr.pr_number, 1);

        let prs = db.list_task_prs(&task.id).await.unwrap();
        assert_eq!(prs.len(), 1);
    }

    #[tokio::test]
    async fn async_api_key_crud() {
        let db = SqliteDatabase::open_in_memory().unwrap();
        assert!(!db.has_api_keys().await.unwrap());

        let key = db.insert_api_key("test-key", "hash123").await.unwrap();
        assert_eq!(key.name, "test-key");

        assert!(db.has_api_keys().await.unwrap());

        let found = db.find_api_key_by_hash("hash123").await.unwrap();
        assert!(found.is_some());

        let not_found = db.find_api_key_by_hash("nope").await.unwrap();
        assert!(not_found.is_none());

        db.touch_api_key(&key.id).await.unwrap();

        let all = db.list_api_keys().await.unwrap();
        assert_eq!(all.len(), 1);

        db.delete_api_key(&key.id).await.unwrap();
        assert!(!db.has_api_keys().await.unwrap());
    }

    #[tokio::test]
    async fn async_attachment_crud() {
        let db = SqliteDatabase::open_in_memory().unwrap();
        let project = db
            .create_project(&CreateProject {
                name: "P".into(),
                slug: "p".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();
        let task = db
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "T".into(),
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

        let att = db
            .create_attachment(&task.id, "readme.md", "store/key", 1024)
            .await
            .unwrap();
        assert_eq!(att.filename, "readme.md");

        let fetched = db.get_attachment(&att.id).await.unwrap();
        assert_eq!(fetched.id, att.id);

        let list = db.list_attachments(&task.id).await.unwrap();
        assert_eq!(list.len(), 1);

        let deleted = db.delete_attachment(&att.id).await.unwrap();
        assert_eq!(deleted.id, att.id);

        let list = db.list_attachments(&task.id).await.unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn async_claim_next_claude_run() {
        let db = SqliteDatabase::open_in_memory().unwrap();
        let project = db
            .create_project(&CreateProject {
                name: "P".into(),
                slug: "p".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();
        let task = db
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "T".into(),
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

        // No pending runs -> None
        let claimed = db.claim_next_claude_run(&["heavy"]).await.unwrap();
        assert!(claimed.is_none());

        // Create a pending run
        let _run = db
            .create_claude_run(&CreateClaudeRun {
                task_id: task.id.clone(),
                action: ClaudeAction::Build,
                required_capability: None,
            })
            .await
            .unwrap();

        // Claim it
        let claimed = db.claim_next_claude_run(&["heavy"]).await.unwrap();
        assert!(claimed.is_some());
    }

    #[tokio::test]
    async fn async_stale_run_queries() {
        let db = SqliteDatabase::open_in_memory().unwrap();
        // No runs -> empty results
        let stale = db
            .find_stale_running_runs(chrono::Utc::now())
            .await
            .unwrap();
        assert!(stale.is_empty());
        let stale = db
            .find_stale_salvaging_runs(chrono::Utc::now())
            .await
            .unwrap();
        assert!(stale.is_empty());
    }

    #[tokio::test]
    async fn async_timeout_claude_run() {
        let db = SqliteDatabase::open_in_memory().unwrap();
        let project = db
            .create_project(&CreateProject {
                name: "P".into(),
                slug: "p".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();
        let task = db
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "T".into(),
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
        let run = db
            .create_claude_run(&CreateClaudeRun {
                task_id: task.id.clone(),
                action: ClaudeAction::Build,
                required_capability: None,
            })
            .await
            .unwrap();

        // Claim it first so it's in Running state
        let _ = db.claim_next_claude_run(&["heavy"]).await.unwrap();

        // Timeout it
        let result = db.timeout_claude_run(&run.id, "timed out").await.unwrap();
        assert!(result.is_some());
        let timed_out = result.unwrap();
        assert_eq!(timed_out.status, ClaudeRunStatus::TimedOut);
    }
}

#[async_trait]
impl Database for SqliteDatabase {
    // -- Projects --
    async fn create_project(&self, input: &CreateProject) -> Result<Project, DbError> {
        let db = self.clone();
        let input = input.clone();
        tokio::task::spawn_blocking(move || db.create_project_sync(&input))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn get_project(&self, id: &str) -> Result<Project, DbError> {
        let db = self.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || db.get_project_sync(&id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn get_project_by_slug(&self, slug: &str) -> Result<Project, DbError> {
        let db = self.clone();
        let slug = slug.to_string();
        tokio::task::spawn_blocking(move || db.get_project_by_slug_sync(&slug))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn list_projects(&self) -> Result<Vec<Project>, DbError> {
        let db = self.clone();
        tokio::task::spawn_blocking(move || db.list_projects_sync())
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn update_project(&self, id: &str, update: &UpdateProject) -> Result<Project, DbError> {
        let db = self.clone();
        let id = id.to_string();
        let update = update.clone();
        tokio::task::spawn_blocking(move || db.update_project_sync(&id, &update))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn delete_project(&self, id: &str) -> Result<(), DbError> {
        let db = self.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || db.delete_project_sync(&id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }

    // -- Tasks --
    async fn create_task(&self, input: &CreateTask) -> Result<Task, DbError> {
        let db = self.clone();
        let input = input.clone();
        tokio::task::spawn_blocking(move || db.create_task_sync(&input))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn get_task(&self, id: &str) -> Result<Task, DbError> {
        let db = self.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || db.get_task_sync(&id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, DbError> {
        let db = self.clone();
        let filter = filter.clone();
        tokio::task::spawn_blocking(move || db.list_tasks_sync(&filter))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn list_child_tasks(&self, parent_id: &str) -> Result<Vec<Task>, DbError> {
        let db = self.clone();
        let parent_id = parent_id.to_string();
        tokio::task::spawn_blocking(move || db.list_child_tasks_sync(&parent_id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn update_task(&self, id: &str, update: &UpdateTask) -> Result<Task, DbError> {
        let db = self.clone();
        let id = id.to_string();
        let update = update.clone();
        tokio::task::spawn_blocking(move || db.update_task_sync(&id, &update))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn delete_task(&self, id: &str) -> Result<(), DbError> {
        let db = self.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || db.delete_task_sync(&id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn count_tasks_by_status(&self, project_id: &str) -> Result<Vec<(String, i64)>, DbError> {
        let db = self.clone();
        let project_id = project_id.to_string();
        tokio::task::spawn_blocking(move || db.count_tasks_by_status_sync(&project_id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }

    // -- Claude Runs --
    async fn create_claude_run(&self, input: &CreateClaudeRun) -> Result<ClaudeRun, DbError> {
        let db = self.clone();
        let input = input.clone();
        tokio::task::spawn_blocking(move || db.create_claude_run_sync(&input))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn get_claude_run(&self, id: &str) -> Result<ClaudeRun, DbError> {
        let db = self.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || db.get_claude_run_sync(&id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn list_claude_runs_for_task(&self, task_id: &str) -> Result<Vec<ClaudeRun>, DbError> {
        let db = self.clone();
        let task_id = task_id.to_string();
        tokio::task::spawn_blocking(move || db.list_claude_runs_for_task_sync(&task_id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn update_claude_run_status(
        &self,
        id: &str,
        status: ClaudeRunStatus,
        error_message: Option<&str>,
        exit_code: Option<i32>,
    ) -> Result<ClaudeRun, DbError> {
        let db = self.clone();
        let id = id.to_string();
        let error_message = error_message.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || {
            db.update_claude_run_status_sync(&id, status, error_message.as_deref(), exit_code)
        })
        .await
        .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn claim_next_claude_run(
        &self,
        capabilities: &[&str],
    ) -> Result<Option<ClaudeRun>, DbError> {
        let db = self.clone();
        let caps: Vec<String> = capabilities.iter().map(|s| s.to_string()).collect();
        tokio::task::spawn_blocking(move || {
            let cap_refs: Vec<&str> = caps.iter().map(|s| s.as_str()).collect();
            db.claim_next_claude_run_sync(&cap_refs)
        })
        .await
        .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn update_claude_run_progress(&self, id: &str, message: &str) -> Result<(), DbError> {
        let db = self.clone();
        let id = id.to_string();
        let message = message.to_string();
        tokio::task::spawn_blocking(move || db.update_claude_run_progress_sync(&id, &message))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn update_claude_run_pr(
        &self,
        id: &str,
        pr_url: Option<&str>,
        pr_number: Option<i64>,
        branch_name: Option<&str>,
    ) -> Result<ClaudeRun, DbError> {
        let db = self.clone();
        let id = id.to_string();
        let pr_url = pr_url.map(|s| s.to_string());
        let branch_name = branch_name.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || {
            db.update_claude_run_pr_sync(&id, pr_url.as_deref(), pr_number, branch_name.as_deref())
        })
        .await
        .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn find_stale_running_runs(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<Vec<ClaudeRun>, DbError> {
        let db = self.clone();
        tokio::task::spawn_blocking(move || db.find_stale_running_runs_sync(older_than))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn find_stale_salvaging_runs(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<Vec<ClaudeRun>, DbError> {
        let db = self.clone();
        tokio::task::spawn_blocking(move || db.find_stale_salvaging_runs_sync(older_than))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn timeout_claude_run(
        &self,
        id: &str,
        error_message: &str,
    ) -> Result<Option<ClaudeRun>, DbError> {
        let db = self.clone();
        let id = id.to_string();
        let error_message = error_message.to_string();
        tokio::task::spawn_blocking(move || db.timeout_claude_run_sync(&id, &error_message))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn set_claude_run_runner(&self, id: &str, runner_id: &str) -> Result<(), DbError> {
        let db = self.clone();
        let id = id.to_string();
        let runner_id = runner_id.to_string();
        tokio::task::spawn_blocking(move || db.set_claude_run_runner_sync(&id, &runner_id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn count_queued_runs(&self) -> Result<i64, DbError> {
        let db = self.clone();
        tokio::task::spawn_blocking(move || db.count_queued_runs_sync())
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }

    // -- Sprints --
    async fn create_sprint(&self, input: &CreateSprint) -> Result<Sprint, DbError> {
        let db = self.clone();
        let input = input.clone();
        tokio::task::spawn_blocking(move || db.create_sprint_sync(&input))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn get_sprint(&self, id: &str) -> Result<Sprint, DbError> {
        let db = self.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || db.get_sprint_sync(&id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn list_sprints(&self, project_id: &str) -> Result<Vec<Sprint>, DbError> {
        let db = self.clone();
        let project_id = project_id.to_string();
        tokio::task::spawn_blocking(move || db.list_sprints_sync(&project_id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn update_sprint(&self, id: &str, update: &UpdateSprint) -> Result<Sprint, DbError> {
        let db = self.clone();
        let id = id.to_string();
        let update = update.clone();
        tokio::task::spawn_blocking(move || db.update_sprint_sync(&id, &update))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn delete_sprint(&self, id: &str) -> Result<(), DbError> {
        let db = self.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || db.delete_sprint_sync(&id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }

    // -- Task Links --
    async fn create_task_link(&self, input: &CreateTaskLink) -> Result<TaskLink, DbError> {
        let db = self.clone();
        let input = input.clone();
        tokio::task::spawn_blocking(move || db.create_task_link_sync(&input))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn list_task_links(&self, task_id: &str) -> Result<Vec<TaskLink>, DbError> {
        let db = self.clone();
        let task_id = task_id.to_string();
        tokio::task::spawn_blocking(move || db.list_task_links_sync(&task_id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn delete_task_link(&self, id: &str) -> Result<(), DbError> {
        let db = self.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || db.delete_task_link_sync(&id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }

    // -- Task PRs --
    async fn create_task_pr(&self, input: &CreateTaskPr) -> Result<TaskPr, DbError> {
        let db = self.clone();
        let input = input.clone();
        tokio::task::spawn_blocking(move || db.create_task_pr_sync(&input))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn list_task_prs(&self, task_id: &str) -> Result<Vec<TaskPr>, DbError> {
        let db = self.clone();
        let task_id = task_id.to_string();
        tokio::task::spawn_blocking(move || db.list_task_prs_sync(&task_id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }

    // -- Attachments --
    async fn create_attachment(
        &self,
        task_id: &str,
        filename: &str,
        store_key: &str,
        size_bytes: i64,
    ) -> Result<Attachment, DbError> {
        let db = self.clone();
        let task_id = task_id.to_string();
        let filename = filename.to_string();
        let store_key = store_key.to_string();
        tokio::task::spawn_blocking(move || {
            db.create_attachment_sync(&task_id, &filename, &store_key, size_bytes)
        })
        .await
        .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn list_attachments(&self, task_id: &str) -> Result<Vec<Attachment>, DbError> {
        let db = self.clone();
        let task_id = task_id.to_string();
        tokio::task::spawn_blocking(move || db.list_attachments_sync(&task_id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn get_attachment(&self, id: &str) -> Result<Attachment, DbError> {
        let db = self.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || db.get_attachment_sync(&id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn delete_attachment(&self, id: &str) -> Result<Attachment, DbError> {
        let db = self.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || db.delete_attachment_sync(&id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }

    // -- API Keys --
    async fn insert_api_key(&self, name: &str, key_hash: &str) -> Result<ApiKey, DbError> {
        let db = self.clone();
        let name = name.to_string();
        let key_hash = key_hash.to_string();
        tokio::task::spawn_blocking(move || db.insert_api_key_sync(&name, &key_hash))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>, DbError> {
        let db = self.clone();
        let key_hash = key_hash.to_string();
        tokio::task::spawn_blocking(move || db.find_api_key_by_hash_sync(&key_hash))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn touch_api_key(&self, id: &str) -> Result<(), DbError> {
        let db = self.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || db.touch_api_key_sync(&id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn has_api_keys(&self) -> Result<bool, DbError> {
        let db = self.clone();
        tokio::task::spawn_blocking(move || db.has_api_keys_sync())
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn list_api_keys(&self) -> Result<Vec<ApiKey>, DbError> {
        let db = self.clone();
        tokio::task::spawn_blocking(move || db.list_api_keys_sync())
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn delete_api_key(&self, id: &str) -> Result<(), DbError> {
        let db = self.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || db.delete_api_key_sync(&id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
}
