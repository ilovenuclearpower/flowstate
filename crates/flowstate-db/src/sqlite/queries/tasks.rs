use chrono::Utc;
use rusqlite::{params, Row};

use flowstate_core::runner::RunnerCapability;
use flowstate_core::task::{
    ApprovalStatus, CreateTask, Priority, Status, Task, TaskFilter, UpdateTask,
};

use super::super::{SqliteDatabase, SqliteResultExt};
use crate::DbError;

fn row_to_task(row: &Row) -> rusqlite::Result<Task> {
    let status_str: String = row.get("status")?;
    let priority_str: String = row.get("priority")?;
    let spec_status_str: String = row.get("spec_status")?;
    let plan_status_str: String = row.get("plan_status")?;
    let research_status_str: String = row.get("research_status")?;
    let verify_status_str: String = row.get("verify_status")?;
    let research_cap_str: Option<String> = row.get("research_capability")?;
    let design_cap_str: Option<String> = row.get("design_capability")?;
    let plan_cap_str: Option<String> = row.get("plan_capability")?;
    let build_cap_str: Option<String> = row.get("build_capability")?;
    let verify_cap_str: Option<String> = row.get("verify_capability")?;
    Ok(Task {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        sprint_id: row.get("sprint_id")?,
        parent_id: row.get("parent_id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        reviewer: row.get("reviewer")?,
        research_status: ApprovalStatus::parse_str(&research_status_str)
            .unwrap_or(ApprovalStatus::None),
        spec_status: ApprovalStatus::parse_str(&spec_status_str).unwrap_or(ApprovalStatus::None),
        plan_status: ApprovalStatus::parse_str(&plan_status_str).unwrap_or(ApprovalStatus::None),
        verify_status: ApprovalStatus::parse_str(&verify_status_str)
            .unwrap_or(ApprovalStatus::None),
        spec_approved_hash: row.get("spec_approved_hash")?,
        research_approved_hash: row.get("research_approved_hash")?,
        research_feedback: row.get("research_feedback")?,
        spec_feedback: row.get("spec_feedback")?,
        plan_feedback: row.get("plan_feedback")?,
        verify_feedback: row.get("verify_feedback")?,
        status: Status::parse_str(&status_str).unwrap_or(Status::Todo),
        priority: Priority::parse_str(&priority_str).unwrap_or(Priority::Medium),
        research_capability: research_cap_str.and_then(|s| RunnerCapability::parse_str(&s)),
        design_capability: design_cap_str.and_then(|s| RunnerCapability::parse_str(&s)),
        plan_capability: plan_cap_str.and_then(|s| RunnerCapability::parse_str(&s)),
        build_capability: build_cap_str.and_then(|s| RunnerCapability::parse_str(&s)),
        verify_capability: verify_cap_str.and_then(|s| RunnerCapability::parse_str(&s)),
        sort_order: row.get("sort_order")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

impl SqliteDatabase {
    pub fn create_task_sync(&self, input: &CreateTask) -> Result<Task, DbError> {
        self.with_conn(|conn| {
            let id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now();

            // Get next sort_order for this project+status
            let max_order: f64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(sort_order), 0) FROM tasks
                     WHERE project_id = ?1 AND status = ?2",
                    params![input.project_id, input.status.as_str()],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            conn.execute(
                "INSERT INTO tasks (
                    id, project_id, parent_id, title, description, reviewer, status, priority, sort_order, created_at, updated_at,
                    research_capability, design_capability, plan_capability, build_capability, verify_capability
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    id,
                    input.project_id,
                    input.parent_id,
                    input.title,
                    input.description,
                    input.reviewer,
                    input.status.as_str(),
                    input.priority.as_str(),
                    max_order + 1.0,
                    now,
                    now,
                    input.research_capability.map(|c| c.as_str().to_string()),
                    input.design_capability.map(|c| c.as_str().to_string()),
                    input.plan_capability.map(|c| c.as_str().to_string()),
                    input.build_capability.map(|c| c.as_str().to_string()),
                    input.verify_capability.map(|c| c.as_str().to_string()),
                ],
            )
            .to_db()?;

            let task = conn
                .query_row(
                    "SELECT * FROM tasks WHERE id = ?1",
                    params![id],
                    row_to_task,
                )
                .to_db()?;
            Ok(task)
        })
    }

    pub fn get_task_sync(&self, id: &str) -> Result<Task, DbError> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM tasks WHERE id = ?1",
                params![id],
                row_to_task,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => DbError::NotFound(format!("task {id}")),
                other => DbError::Internal(other.to_string()),
            })
        })
    }

    pub fn list_tasks_sync(&self, filter: &TaskFilter) -> Result<Vec<Task>, DbError> {
        self.with_conn(|conn| {
            let mut sql = String::from("SELECT * FROM tasks WHERE 1=1");
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(ref project_id) = filter.project_id {
                param_values.push(Box::new(project_id.clone()));
                sql.push_str(&format!(" AND project_id = ?{}", param_values.len()));
            }
            if let Some(status) = filter.status {
                param_values.push(Box::new(status.as_str().to_string()));
                sql.push_str(&format!(" AND status = ?{}", param_values.len()));
            }
            if let Some(priority) = filter.priority {
                param_values.push(Box::new(priority.as_str().to_string()));
                sql.push_str(&format!(" AND priority = ?{}", param_values.len()));
            }
            if let Some(ref sprint_id) = filter.sprint_id {
                param_values.push(Box::new(sprint_id.clone()));
                sql.push_str(&format!(" AND sprint_id = ?{}", param_values.len()));
            }
            if let Some(ref parent_id_filter) = filter.parent_id {
                match parent_id_filter {
                    None => {
                        sql.push_str(" AND parent_id IS NULL");
                    }
                    Some(pid) => {
                        param_values.push(Box::new(pid.clone()));
                        sql.push_str(&format!(" AND parent_id = ?{}", param_values.len()));
                    }
                }
            }

            sql.push_str(" ORDER BY sort_order ASC");

            if let Some(limit) = filter.limit {
                param_values.push(Box::new(limit));
                sql.push_str(&format!(" LIMIT ?{}", param_values.len()));
            }

            let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn.prepare(&sql).to_db()?;
            let tasks = stmt
                .query_map(params_ref.as_slice(), row_to_task)
                .to_db()?
                .collect::<Result<Vec<_>, _>>()
                .to_db()?;
            Ok(tasks)
        })
    }

    pub fn list_child_tasks_sync(&self, parent_id: &str) -> Result<Vec<Task>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT * FROM tasks WHERE parent_id = ?1 ORDER BY sort_order ASC")
                .to_db()?;
            let tasks = stmt
                .query_map(params![parent_id], row_to_task)
                .to_db()?
                .collect::<Result<Vec<_>, _>>()
                .to_db()?;
            Ok(tasks)
        })
    }

    pub fn update_task_sync(&self, id: &str, update: &UpdateTask) -> Result<Task, DbError> {
        self.with_conn(|conn| {
            let now = Utc::now();
            let mut sets = vec!["updated_at = ?1".to_string()];
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(now)];

            if let Some(ref title) = update.title {
                param_values.push(Box::new(title.clone()));
                sets.push(format!("title = ?{}", param_values.len()));
            }
            if let Some(ref description) = update.description {
                param_values.push(Box::new(description.clone()));
                sets.push(format!("description = ?{}", param_values.len()));
            }
            if let Some(status) = update.status {
                param_values.push(Box::new(status.as_str().to_string()));
                sets.push(format!("status = ?{}", param_values.len()));
            }
            if let Some(priority) = update.priority {
                param_values.push(Box::new(priority.as_str().to_string()));
                sets.push(format!("priority = ?{}", param_values.len()));
            }
            if let Some(ref sprint_id) = update.sprint_id {
                param_values.push(Box::new(sprint_id.clone()));
                sets.push(format!("sprint_id = ?{}", param_values.len()));
            }
            if let Some(sort_order) = update.sort_order {
                param_values.push(Box::new(sort_order));
                sets.push(format!("sort_order = ?{}", param_values.len()));
            }
            if let Some(ref parent_id) = update.parent_id {
                param_values.push(Box::new(parent_id.clone()));
                sets.push(format!("parent_id = ?{}", param_values.len()));
            }
            if let Some(ref reviewer) = update.reviewer {
                param_values.push(Box::new(reviewer.clone()));
                sets.push(format!("reviewer = ?{}", param_values.len()));
            }
            if let Some(research_status) = update.research_status {
                param_values.push(Box::new(research_status.as_str().to_string()));
                sets.push(format!("research_status = ?{}", param_values.len()));
            }
            if let Some(spec_status) = update.spec_status {
                param_values.push(Box::new(spec_status.as_str().to_string()));
                sets.push(format!("spec_status = ?{}", param_values.len()));
            }
            if let Some(plan_status) = update.plan_status {
                param_values.push(Box::new(plan_status.as_str().to_string()));
                sets.push(format!("plan_status = ?{}", param_values.len()));
            }
            if let Some(verify_status) = update.verify_status {
                param_values.push(Box::new(verify_status.as_str().to_string()));
                sets.push(format!("verify_status = ?{}", param_values.len()));
            }
            if let Some(ref hash) = update.spec_approved_hash {
                param_values.push(Box::new(hash.clone()));
                sets.push(format!("spec_approved_hash = ?{}", param_values.len()));
            }
            if let Some(ref hash) = update.research_approved_hash {
                param_values.push(Box::new(hash.clone()));
                sets.push(format!("research_approved_hash = ?{}", param_values.len()));
            }
            if let Some(ref feedback) = update.research_feedback {
                param_values.push(Box::new(feedback.clone()));
                sets.push(format!("research_feedback = ?{}", param_values.len()));
            }
            if let Some(ref feedback) = update.spec_feedback {
                param_values.push(Box::new(feedback.clone()));
                sets.push(format!("spec_feedback = ?{}", param_values.len()));
            }
            if let Some(ref feedback) = update.plan_feedback {
                param_values.push(Box::new(feedback.clone()));
                sets.push(format!("plan_feedback = ?{}", param_values.len()));
            }
            if let Some(ref feedback) = update.verify_feedback {
                param_values.push(Box::new(feedback.clone()));
                sets.push(format!("verify_feedback = ?{}", param_values.len()));
            }
            if let Some(cap) = &update.research_capability {
                param_values.push(Box::new(cap.map(|c| c.as_str().to_string())));
                sets.push(format!("research_capability = ?{}", param_values.len()));
            }
            if let Some(cap) = &update.design_capability {
                param_values.push(Box::new(cap.map(|c| c.as_str().to_string())));
                sets.push(format!("design_capability = ?{}", param_values.len()));
            }
            if let Some(cap) = &update.plan_capability {
                param_values.push(Box::new(cap.map(|c| c.as_str().to_string())));
                sets.push(format!("plan_capability = ?{}", param_values.len()));
            }
            if let Some(cap) = &update.build_capability {
                param_values.push(Box::new(cap.map(|c| c.as_str().to_string())));
                sets.push(format!("build_capability = ?{}", param_values.len()));
            }
            if let Some(cap) = &update.verify_capability {
                param_values.push(Box::new(cap.map(|c| c.as_str().to_string())));
                sets.push(format!("verify_capability = ?{}", param_values.len()));
            }

            param_values.push(Box::new(id.to_string()));
            let id_param = param_values.len();

            let sql = format!(
                "UPDATE tasks SET {} WHERE id = ?{}",
                sets.join(", "),
                id_param
            );

            let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|p| p.as_ref()).collect();

            let changed = conn.execute(&sql, params_ref.as_slice()).to_db()?;
            if changed == 0 {
                return Err(DbError::NotFound(format!("task {id}")));
            }

            conn.query_row(
                "SELECT * FROM tasks WHERE id = ?1",
                params![id],
                row_to_task,
            )
            .map_err(|e| DbError::Internal(e.to_string()))
        })
    }

    pub fn delete_task_sync(&self, id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            let changed = conn
                .execute("DELETE FROM tasks WHERE id = ?1", params![id])
                .to_db()?;
            if changed == 0 {
                return Err(DbError::NotFound(format!("task {id}")));
            }
            Ok(())
        })
    }

    pub fn count_tasks_by_status_sync(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, i64)>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT status, COUNT(*) as cnt FROM tasks
                     WHERE project_id = ?1 GROUP BY status",
                )
                .to_db()?;
            let counts = stmt
                .query_map(params![project_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .to_db()?
                .collect::<Result<Vec<_>, _>>()
                .to_db()?;
            Ok(counts)
        })
    }
}

#[cfg(test)]
mod tests {
    use flowstate_core::project::CreateProject;
    use flowstate_core::task::{CreateTask, Priority, Status, TaskFilter, UpdateTask};

    use crate::Db;

    fn setup() -> (Db, String) {
        let db = Db::open_in_memory().unwrap();
        let project = db
            .create_project_sync(&CreateProject {
                name: "Test".into(),
                slug: "test".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .unwrap();
        (db, project.id)
    }

    #[test]
    fn test_task_crud() {
        let (db, project_id) = setup();

        let task = db
            .create_task_sync(&CreateTask {
                project_id: project_id.clone(),
                title: "First task".into(),
                description: "Do something".into(),
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
            .unwrap();

        assert_eq!(task.title, "First task");
        assert_eq!(task.status, Status::Todo);
        assert_eq!(task.priority, Priority::High);

        let fetched = db.get_task_sync(&task.id).unwrap();
        assert_eq!(fetched.id, task.id);

        let updated = db
            .update_task_sync(
                &task.id,
                &UpdateTask {
                    status: Some(Status::Build),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.status, Status::Build);

        db.delete_task_sync(&task.id).unwrap();
        assert!(db.get_task_sync(&task.id).is_err());
    }

    #[test]
    fn test_task_filtering() {
        let (db, project_id) = setup();

        for i in 0..5 {
            db.create_task_sync(&CreateTask {
                project_id: project_id.clone(),
                title: format!("Task {i}"),
                description: String::new(),
                status: if i < 3 { Status::Todo } else { Status::Done },
                priority: Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
                research_capability: None,
                design_capability: None,
                plan_capability: None,
                build_capability: None,
                verify_capability: None,
            })
            .unwrap();
        }

        let all = db
            .list_tasks_sync(&TaskFilter {
                project_id: Some(project_id.clone()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(all.len(), 5);

        let todos = db
            .list_tasks_sync(&TaskFilter {
                project_id: Some(project_id.clone()),
                status: Some(Status::Todo),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(todos.len(), 3);

        let limited = db
            .list_tasks_sync(&TaskFilter {
                project_id: Some(project_id.clone()),
                limit: Some(2),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn test_sort_order_auto_increment() {
        let (db, project_id) = setup();

        let t1 = db
            .create_task_sync(&CreateTask {
                project_id: project_id.clone(),
                title: "First".into(),
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
            .unwrap();

        let t2 = db
            .create_task_sync(&CreateTask {
                project_id: project_id.clone(),
                title: "Second".into(),
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
            .unwrap();

        assert!(t2.sort_order > t1.sort_order);
    }

    #[test]
    fn test_count_by_status() {
        let (db, project_id) = setup();

        db.create_task_sync(&CreateTask {
            project_id: project_id.clone(),
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
        .unwrap();
        db.create_task_sync(&CreateTask {
            project_id: project_id.clone(),
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
        .unwrap();
        db.create_task_sync(&CreateTask {
            project_id: project_id.clone(),
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
        .unwrap();

        let counts = db.count_tasks_by_status_sync(&project_id).unwrap();
        let todo_count = counts.iter().find(|(s, _)| s == "todo").map(|(_, c)| *c);
        let done_count = counts.iter().find(|(s, _)| s == "done").map(|(_, c)| *c);
        assert_eq!(todo_count, Some(2));
        assert_eq!(done_count, Some(1));
    }
}
