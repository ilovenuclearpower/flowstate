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
        let conn =
            Connection::open(path).map_err(|e| DbError::Internal(e.to_string()))?;
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
        let conn =
            Connection::open_in_memory().map_err(|e| DbError::Internal(e.to_string()))?;
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
    async fn update_project(
        &self,
        id: &str,
        update: &UpdateProject,
    ) -> Result<Project, DbError> {
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
    async fn update_task(
        &self,
        id: &str,
        update: &UpdateTask,
    ) -> Result<Task, DbError> {
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
    async fn count_tasks_by_status(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, i64)>, DbError> {
        let db = self.clone();
        let project_id = project_id.to_string();
        tokio::task::spawn_blocking(move || db.count_tasks_by_status_sync(&project_id))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }

    // -- Claude Runs --
    async fn create_claude_run(
        &self,
        input: &CreateClaudeRun,
    ) -> Result<ClaudeRun, DbError> {
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
    async fn list_claude_runs_for_task(
        &self,
        task_id: &str,
    ) -> Result<Vec<ClaudeRun>, DbError> {
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
            db.update_claude_run_status_sync(
                &id,
                status,
                error_message.as_deref(),
                exit_code,
            )
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
    async fn update_claude_run_progress(
        &self,
        id: &str,
        message: &str,
    ) -> Result<(), DbError> {
        let db = self.clone();
        let id = id.to_string();
        let message = message.to_string();
        tokio::task::spawn_blocking(move || {
            db.update_claude_run_progress_sync(&id, &message)
        })
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
            db.update_claude_run_pr_sync(
                &id,
                pr_url.as_deref(),
                pr_number,
                branch_name.as_deref(),
            )
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
        tokio::task::spawn_blocking(move || {
            db.find_stale_salvaging_runs_sync(older_than)
        })
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
        tokio::task::spawn_blocking(move || {
            db.timeout_claude_run_sync(&id, &error_message)
        })
        .await
        .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn set_claude_run_runner(
        &self,
        id: &str,
        runner_id: &str,
    ) -> Result<(), DbError> {
        let db = self.clone();
        let id = id.to_string();
        let runner_id = runner_id.to_string();
        tokio::task::spawn_blocking(move || {
            db.set_claude_run_runner_sync(&id, &runner_id)
        })
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
    async fn create_task_link(
        &self,
        input: &CreateTaskLink,
    ) -> Result<TaskLink, DbError> {
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
    async fn create_task_pr(
        &self,
        input: &CreateTaskPr,
    ) -> Result<TaskPr, DbError> {
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
    async fn list_attachments(
        &self,
        task_id: &str,
    ) -> Result<Vec<Attachment>, DbError> {
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
    async fn insert_api_key(
        &self,
        name: &str,
        key_hash: &str,
    ) -> Result<ApiKey, DbError> {
        let db = self.clone();
        let name = name.to_string();
        let key_hash = key_hash.to_string();
        tokio::task::spawn_blocking(move || db.insert_api_key_sync(&name, &key_hash))
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?
    }
    async fn find_api_key_by_hash(
        &self,
        key_hash: &str,
    ) -> Result<Option<ApiKey>, DbError> {
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
