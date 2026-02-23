use chrono::{DateTime, Utc};
use sqlx::Row;

use flowstate_core::runner::RunnerCapability;
use flowstate_core::task::{
    ApprovalStatus, CreateTask, Priority, Status, Task, TaskFilter, UpdateTask,
};

use super::super::{pg_err, pg_not_found, PostgresDatabase};
use crate::DbError;

#[derive(sqlx::FromRow)]
struct TaskRow {
    id: String,
    project_id: String,
    sprint_id: Option<String>,
    parent_id: Option<String>,
    title: String,
    description: String,
    reviewer: String,
    status: String,
    priority: String,
    sort_order: f64,
    research_status: String,
    spec_status: String,
    plan_status: String,
    verify_status: String,
    spec_approved_hash: String,
    research_approved_hash: String,
    research_feedback: String,
    spec_feedback: String,
    plan_feedback: String,
    verify_feedback: String,
    research_capability: Option<String>,
    design_capability: Option<String>,
    plan_capability: Option<String>,
    build_capability: Option<String>,
    verify_capability: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<TaskRow> for Task {
    fn from(r: TaskRow) -> Self {
        Task {
            id: r.id,
            project_id: r.project_id,
            sprint_id: r.sprint_id,
            parent_id: r.parent_id,
            title: r.title,
            description: r.description,
            reviewer: r.reviewer,
            research_status: ApprovalStatus::parse_str(&r.research_status)
                .unwrap_or(ApprovalStatus::None),
            spec_status: ApprovalStatus::parse_str(&r.spec_status).unwrap_or(ApprovalStatus::None),
            plan_status: ApprovalStatus::parse_str(&r.plan_status).unwrap_or(ApprovalStatus::None),
            verify_status: ApprovalStatus::parse_str(&r.verify_status)
                .unwrap_or(ApprovalStatus::None),
            spec_approved_hash: r.spec_approved_hash,
            research_approved_hash: r.research_approved_hash,
            research_feedback: r.research_feedback,
            spec_feedback: r.spec_feedback,
            plan_feedback: r.plan_feedback,
            verify_feedback: r.verify_feedback,
            status: Status::parse_str(&r.status).unwrap_or(Status::Todo),
            priority: Priority::parse_str(&r.priority).unwrap_or(Priority::Medium),
            research_capability: r
                .research_capability
                .and_then(|s| RunnerCapability::parse_str(&s)),
            design_capability: r
                .design_capability
                .and_then(|s| RunnerCapability::parse_str(&s)),
            plan_capability: r
                .plan_capability
                .and_then(|s| RunnerCapability::parse_str(&s)),
            build_capability: r
                .build_capability
                .and_then(|s| RunnerCapability::parse_str(&s)),
            verify_capability: r
                .verify_capability
                .and_then(|s| RunnerCapability::parse_str(&s)),
            sort_order: r.sort_order,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

impl PostgresDatabase {
    pub(crate) async fn pg_create_task(&self, input: &CreateTask) -> Result<Task, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        // Get next sort_order for this project+status
        let max_order: f64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sort_order), 0) FROM tasks WHERE project_id = $1 AND status = $2",
        )
        .bind(&input.project_id)
        .bind(input.status.as_str())
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0.0);

        sqlx::query(
            "INSERT INTO tasks (
                 id, project_id, parent_id, title, description, reviewer, status, priority, sort_order, created_at, updated_at,
                 research_capability, design_capability, plan_capability, build_capability, verify_capability
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
        )
        .bind(&id)
        .bind(&input.project_id)
        .bind(&input.parent_id)
        .bind(&input.title)
        .bind(&input.description)
        .bind(&input.reviewer)
        .bind(input.status.as_str())
        .bind(input.priority.as_str())
        .bind(max_order + 1.0)
        .bind(now)
        .bind(now)
        .bind(input.research_capability.map(|c| c.as_str().to_string()))
        .bind(input.design_capability.map(|c| c.as_str().to_string()))
        .bind(input.plan_capability.map(|c| c.as_str().to_string()))
        .bind(input.build_capability.map(|c| c.as_str().to_string()))
        .bind(input.verify_capability.map(|c| c.as_str().to_string()))
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        let row = sqlx::query_as::<_, TaskRow>("SELECT * FROM tasks WHERE id = $1")
            .bind(&id)
            .fetch_one(&self.pool)
            .await
            .map_err(pg_err)?;

        Ok(row.into())
    }

    pub(crate) async fn pg_get_task(&self, id: &str) -> Result<Task, DbError> {
        let row = sqlx::query_as::<_, TaskRow>("SELECT * FROM tasks WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(pg_err)?
            .ok_or_else(|| pg_not_found(&format!("task {id}")))?;

        Ok(row.into())
    }

    pub(crate) async fn pg_list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, DbError> {
        let mut sql = String::from("SELECT * FROM tasks WHERE 1=1");
        let mut param_idx = 1usize;

        // We'll collect bind values as trait objects
        // Since sqlx doesn't easily support dynamic bind lists,
        // we build the SQL string and use a manual approach.

        // Collect filter values in order
        struct StrParam(String);
        let mut params: Vec<StrParam> = Vec::new();
        let mut has_limit = false;
        let mut limit_val: i64 = 0;

        if let Some(ref project_id) = filter.project_id {
            sql.push_str(&format!(" AND project_id = ${param_idx}"));
            params.push(StrParam(project_id.clone()));
            param_idx += 1;
        }
        if let Some(status) = filter.status {
            sql.push_str(&format!(" AND status = ${param_idx}"));
            params.push(StrParam(status.as_str().to_string()));
            param_idx += 1;
        }
        if let Some(priority) = filter.priority {
            sql.push_str(&format!(" AND priority = ${param_idx}"));
            params.push(StrParam(priority.as_str().to_string()));
            param_idx += 1;
        }
        if let Some(ref sprint_id) = filter.sprint_id {
            sql.push_str(&format!(" AND sprint_id = ${param_idx}"));
            params.push(StrParam(sprint_id.clone()));
            param_idx += 1;
        }
        if let Some(ref parent_id_filter) = filter.parent_id {
            match parent_id_filter {
                None => {
                    sql.push_str(" AND parent_id IS NULL");
                }
                Some(pid) => {
                    sql.push_str(&format!(" AND parent_id = ${param_idx}"));
                    params.push(StrParam(pid.clone()));
                    param_idx += 1;
                }
            }
        }

        sql.push_str(" ORDER BY sort_order ASC");

        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT ${param_idx}"));
            has_limit = true;
            limit_val = limit;
        }

        // Build the query with sequential binds
        let mut query = sqlx::query_as::<_, TaskRow>(&sql);
        for p in &params {
            query = query.bind(&p.0);
        }
        if has_limit {
            query = query.bind(limit_val);
        }

        let rows = query.fetch_all(&self.pool).await.map_err(pg_err)?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub(crate) async fn pg_list_child_tasks(&self, parent_id: &str) -> Result<Vec<Task>, DbError> {
        let rows = sqlx::query_as::<_, TaskRow>(
            "SELECT * FROM tasks WHERE parent_id = $1 ORDER BY sort_order ASC",
        )
        .bind(parent_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub(crate) async fn pg_update_task(
        &self,
        id: &str,
        update: &UpdateTask,
    ) -> Result<Task, DbError> {
        let now = Utc::now();
        let mut sets = vec![String::new()]; // placeholder for updated_at at index 0
        let mut param_idx = 1usize;

        // We'll track all string values and special typed values
        enum ParamValue {
            Str(String),
            OptStr(Option<String>),
            Float(f64),
            Timestamp(DateTime<Utc>),
        }
        let mut params: Vec<ParamValue> = Vec::new();

        // updated_at is always first
        sets[0] = format!("updated_at = ${param_idx}");
        params.push(ParamValue::Timestamp(now));
        param_idx += 1;

        if let Some(ref title) = update.title {
            sets.push(format!("title = ${param_idx}"));
            params.push(ParamValue::Str(title.clone()));
            param_idx += 1;
        }
        if let Some(ref description) = update.description {
            sets.push(format!("description = ${param_idx}"));
            params.push(ParamValue::Str(description.clone()));
            param_idx += 1;
        }
        if let Some(status) = update.status {
            sets.push(format!("status = ${param_idx}"));
            params.push(ParamValue::Str(status.as_str().to_string()));
            param_idx += 1;
        }
        if let Some(priority) = update.priority {
            sets.push(format!("priority = ${param_idx}"));
            params.push(ParamValue::Str(priority.as_str().to_string()));
            param_idx += 1;
        }
        if let Some(ref sprint_id) = update.sprint_id {
            sets.push(format!("sprint_id = ${param_idx}"));
            params.push(ParamValue::OptStr(sprint_id.clone()));
            param_idx += 1;
        }
        if let Some(sort_order) = update.sort_order {
            sets.push(format!("sort_order = ${param_idx}"));
            params.push(ParamValue::Float(sort_order));
            param_idx += 1;
        }
        if let Some(ref parent_id) = update.parent_id {
            sets.push(format!("parent_id = ${param_idx}"));
            params.push(ParamValue::OptStr(parent_id.clone()));
            param_idx += 1;
        }
        if let Some(ref reviewer) = update.reviewer {
            sets.push(format!("reviewer = ${param_idx}"));
            params.push(ParamValue::Str(reviewer.clone()));
            param_idx += 1;
        }
        if let Some(research_status) = update.research_status {
            sets.push(format!("research_status = ${param_idx}"));
            params.push(ParamValue::Str(research_status.as_str().to_string()));
            param_idx += 1;
        }
        if let Some(spec_status) = update.spec_status {
            sets.push(format!("spec_status = ${param_idx}"));
            params.push(ParamValue::Str(spec_status.as_str().to_string()));
            param_idx += 1;
        }
        if let Some(plan_status) = update.plan_status {
            sets.push(format!("plan_status = ${param_idx}"));
            params.push(ParamValue::Str(plan_status.as_str().to_string()));
            param_idx += 1;
        }
        if let Some(verify_status) = update.verify_status {
            sets.push(format!("verify_status = ${param_idx}"));
            params.push(ParamValue::Str(verify_status.as_str().to_string()));
            param_idx += 1;
        }
        if let Some(ref hash) = update.spec_approved_hash {
            sets.push(format!("spec_approved_hash = ${param_idx}"));
            params.push(ParamValue::Str(hash.clone()));
            param_idx += 1;
        }
        if let Some(ref hash) = update.research_approved_hash {
            sets.push(format!("research_approved_hash = ${param_idx}"));
            params.push(ParamValue::Str(hash.clone()));
            param_idx += 1;
        }
        if let Some(ref feedback) = update.research_feedback {
            sets.push(format!("research_feedback = ${param_idx}"));
            params.push(ParamValue::Str(feedback.clone()));
            param_idx += 1;
        }
        if let Some(ref feedback) = update.spec_feedback {
            sets.push(format!("spec_feedback = ${param_idx}"));
            params.push(ParamValue::Str(feedback.clone()));
            param_idx += 1;
        }
        if let Some(ref feedback) = update.plan_feedback {
            sets.push(format!("plan_feedback = ${param_idx}"));
            params.push(ParamValue::Str(feedback.clone()));
            param_idx += 1;
        }
        if let Some(ref feedback) = update.verify_feedback {
            sets.push(format!("verify_feedback = ${param_idx}"));
            params.push(ParamValue::Str(feedback.clone()));
            param_idx += 1;
        }
        if let Some(cap) = &update.research_capability {
            sets.push(format!("research_capability = ${param_idx}"));
            params.push(ParamValue::OptStr(cap.map(|c| c.as_str().to_string())));
            param_idx += 1;
        }
        if let Some(cap) = &update.design_capability {
            sets.push(format!("design_capability = ${param_idx}"));
            params.push(ParamValue::OptStr(cap.map(|c| c.as_str().to_string())));
            param_idx += 1;
        }
        if let Some(cap) = &update.plan_capability {
            sets.push(format!("plan_capability = ${param_idx}"));
            params.push(ParamValue::OptStr(cap.map(|c| c.as_str().to_string())));
            param_idx += 1;
        }
        if let Some(cap) = &update.build_capability {
            sets.push(format!("build_capability = ${param_idx}"));
            params.push(ParamValue::OptStr(cap.map(|c| c.as_str().to_string())));
            param_idx += 1;
        }
        if let Some(cap) = &update.verify_capability {
            sets.push(format!("verify_capability = ${param_idx}"));
            params.push(ParamValue::OptStr(cap.map(|c| c.as_str().to_string())));
            param_idx += 1;
        }

        let id_param = param_idx;

        let sql = format!(
            "UPDATE tasks SET {} WHERE id = ${id_param}",
            sets.join(", ")
        );

        let mut query = sqlx::query(&sql);
        for p in &params {
            match p {
                ParamValue::Str(s) => query = query.bind(s),
                ParamValue::OptStr(s) => query = query.bind(s),
                ParamValue::Float(f) => query = query.bind(f),
                ParamValue::Timestamp(t) => query = query.bind(t),
            }
        }
        query = query.bind(id);

        let result = query.execute(&self.pool).await.map_err(pg_err)?;

        if result.rows_affected() == 0 {
            return Err(pg_not_found(&format!("task {id}")));
        }

        self.pg_get_task(id).await
    }

    pub(crate) async fn pg_delete_task(&self, id: &str) -> Result<(), DbError> {
        let result = sqlx::query("DELETE FROM tasks WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;

        if result.rows_affected() == 0 {
            return Err(pg_not_found(&format!("task {id}")));
        }

        Ok(())
    }

    pub(crate) async fn pg_count_tasks_by_status(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, i64)>, DbError> {
        let rows = sqlx::query(
            "SELECT status, COUNT(*) as cnt FROM tasks WHERE project_id = $1 GROUP BY status",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        let counts = rows
            .iter()
            .map(|row| {
                let status: String = row.get("status");
                let cnt: i64 = row.get("cnt");
                (status, cnt)
            })
            .collect();

        Ok(counts)
    }
}
