use chrono::Utc;
use rusqlite::{params, Row};

use flowstate_core::task_pr::{CreateTaskPr, TaskPr};

use crate::DbError;
use super::super::{SqliteDatabase, SqliteResultExt};

fn row_to_task_pr(row: &Row) -> rusqlite::Result<TaskPr> {
    Ok(TaskPr {
        id: row.get("id")?,
        task_id: row.get("task_id")?,
        claude_run_id: row.get("claude_run_id")?,
        pr_url: row.get("pr_url")?,
        pr_number: row.get("pr_number")?,
        branch_name: row.get("branch_name")?,
        created_at: row.get("created_at")?,
    })
}

impl SqliteDatabase {
    pub fn create_task_pr_sync(&self, input: &CreateTaskPr) -> Result<TaskPr, DbError> {
        self.with_conn(|conn| {
            let id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now();
            conn.execute(
                "INSERT OR IGNORE INTO task_prs (id, task_id, claude_run_id, pr_url, pr_number, branch_name, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    input.task_id,
                    input.claude_run_id,
                    input.pr_url,
                    input.pr_number,
                    input.branch_name,
                    now,
                ],
            )
            .to_db()?;
            conn.query_row(
                "SELECT * FROM task_prs WHERE pr_url = ?1",
                params![input.pr_url],
                row_to_task_pr,
            )
            .map_err(|e| DbError::Internal(e.to_string()))
        })
    }

    pub fn list_task_prs_sync(&self, task_id: &str) -> Result<Vec<TaskPr>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT * FROM task_prs WHERE task_id = ?1 ORDER BY created_at DESC",
                )
                .to_db()?;
            let prs = stmt
                .query_map(params![task_id], row_to_task_pr)
                .to_db()?
                .collect::<Result<Vec<_>, _>>()
                .to_db()?;
            Ok(prs)
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Db;
    use flowstate_core::claude_run::{ClaudeAction, CreateClaudeRun};
    use flowstate_core::project::CreateProject;
    use flowstate_core::task::{CreateTask, Priority, Status};
    use flowstate_core::task_pr::CreateTaskPr;
    use rusqlite::params;

    fn setup_db() -> (Db, String, String) {
        let db = Db::open_in_memory().unwrap();
        let project = db
            .create_project_sync(&CreateProject {
                name: "Test".into(),
                slug: "test".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .unwrap();
        let task = db
            .create_task_sync(&CreateTask {
                project_id: project.id.clone(),
                title: "Task 1".into(),
                description: String::new(),
                status: Status::Todo,
                priority: Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
            })
            .unwrap();
        (db, project.id, task.id)
    }

    #[test]
    fn test_create_and_list_task_prs() {
        let (db, _project_id, task_id) = setup_db();
        let run = db
            .create_claude_run_sync(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Build,
            })
            .unwrap();

        let pr = db
            .create_task_pr_sync(&CreateTaskPr {
                task_id: task_id.clone(),
                claude_run_id: Some(run.id.clone()),
                pr_url: "https://github.com/owner/repo/pull/42".into(),
                pr_number: 42,
                branch_name: "flowstate/my-feature".into(),
            })
            .unwrap();

        assert_eq!(pr.task_id, task_id);
        assert_eq!(pr.claude_run_id.as_deref(), Some(run.id.as_str()));
        assert_eq!(pr.pr_url, "https://github.com/owner/repo/pull/42");
        assert_eq!(pr.pr_number, 42);
        assert_eq!(pr.branch_name, "flowstate/my-feature");

        let prs = db.list_task_prs_sync(&task_id).unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].id, pr.id);
    }

    #[test]
    fn test_duplicate_pr_url_is_idempotent() {
        let (db, _project_id, task_id) = setup_db();

        let pr1 = db
            .create_task_pr_sync(&CreateTaskPr {
                task_id: task_id.clone(),
                claude_run_id: None,
                pr_url: "https://github.com/owner/repo/pull/1".into(),
                pr_number: 1,
                branch_name: "flowstate/feat-1".into(),
            })
            .unwrap();

        let pr2 = db
            .create_task_pr_sync(&CreateTaskPr {
                task_id: task_id.clone(),
                claude_run_id: None,
                pr_url: "https://github.com/owner/repo/pull/1".into(),
                pr_number: 1,
                branch_name: "flowstate/feat-1".into(),
            })
            .unwrap();

        assert_eq!(pr1.id, pr2.id);
        let prs = db.list_task_prs_sync(&task_id).unwrap();
        assert_eq!(prs.len(), 1);
    }

    #[test]
    fn test_multiple_prs_per_task() {
        let (db, _project_id, task_id) = setup_db();

        db.create_task_pr_sync(&CreateTaskPr {
            task_id: task_id.clone(),
            claude_run_id: None,
            pr_url: "https://github.com/owner/repo/pull/10".into(),
            pr_number: 10,
            branch_name: "flowstate/feat-a".into(),
        })
        .unwrap();

        db.create_task_pr_sync(&CreateTaskPr {
            task_id: task_id.clone(),
            claude_run_id: None,
            pr_url: "https://github.com/owner/repo/pull/11".into(),
            pr_number: 11,
            branch_name: "flowstate/feat-b".into(),
        })
        .unwrap();

        let prs = db.list_task_prs_sync(&task_id).unwrap();
        assert_eq!(prs.len(), 2);
    }

    #[test]
    fn test_cascade_delete_task() {
        let (db, _project_id, task_id) = setup_db();

        db.create_task_pr_sync(&CreateTaskPr {
            task_id: task_id.clone(),
            claude_run_id: None,
            pr_url: "https://github.com/owner/repo/pull/5".into(),
            pr_number: 5,
            branch_name: "flowstate/feat".into(),
        })
        .unwrap();

        db.delete_task_sync(&task_id).unwrap();
        let prs = db.list_task_prs_sync(&task_id).unwrap();
        assert!(prs.is_empty());
    }

    #[test]
    fn test_set_null_on_run_delete() {
        let (db, _project_id, task_id) = setup_db();
        let run = db
            .create_claude_run_sync(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Build,
            })
            .unwrap();

        let pr = db
            .create_task_pr_sync(&CreateTaskPr {
                task_id: task_id.clone(),
                claude_run_id: Some(run.id.clone()),
                pr_url: "https://github.com/owner/repo/pull/7".into(),
                pr_number: 7,
                branch_name: "flowstate/feat".into(),
            })
            .unwrap();

        assert!(pr.claude_run_id.is_some());

        // Delete the claude_run directly via SQL
        db.with_conn(|conn| {
            conn.execute("DELETE FROM claude_runs WHERE id = ?1", params![run.id])
                .map_err(|e| crate::DbError::Internal(e.to_string()))?;
            Ok(())
        })
        .unwrap();

        let prs = db.list_task_prs_sync(&task_id).unwrap();
        assert_eq!(prs.len(), 1);
        assert!(prs[0].claude_run_id.is_none());
    }
}
