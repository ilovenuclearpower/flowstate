use chrono::Utc;
use rusqlite::{params, Row};

use flowstate_core::sprint::{CreateSprint, Sprint, SprintStatus, UpdateSprint};

use super::super::{SqliteDatabase, SqliteResultExt};
use crate::DbError;

fn row_to_sprint(row: &Row) -> rusqlite::Result<Sprint> {
    let status_str: String = row.get("status")?;
    Ok(Sprint {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        name: row.get("name")?,
        goal: row.get("goal")?,
        starts_at: row.get("starts_at")?,
        ends_at: row.get("ends_at")?,
        status: SprintStatus::parse_str(&status_str).unwrap_or(SprintStatus::Planned),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

impl SqliteDatabase {
    pub fn create_sprint_sync(&self, input: &CreateSprint) -> Result<Sprint, DbError> {
        self.with_conn(|conn| {
            let id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now();
            conn.execute(
                "INSERT INTO sprints (id, project_id, name, goal, starts_at, ends_at, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![id, input.project_id, input.name, input.goal, input.starts_at, input.ends_at, "planned", now, now],
            )
            .to_db()?;
            conn.query_row(
                "SELECT * FROM sprints WHERE id = ?1",
                params![id],
                row_to_sprint,
            )
            .to_db()
        })
    }

    pub fn get_sprint_sync(&self, id: &str) -> Result<Sprint, DbError> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM sprints WHERE id = ?1",
                params![id],
                row_to_sprint,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => DbError::NotFound(format!("sprint {id}")),
                other => DbError::Internal(other.to_string()),
            })
        })
    }

    pub fn list_sprints_sync(&self, project_id: &str) -> Result<Vec<Sprint>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT * FROM sprints WHERE project_id = ?1 ORDER BY created_at DESC")
                .to_db()?;
            let sprints = stmt
                .query_map(params![project_id], row_to_sprint)
                .to_db()?
                .collect::<Result<Vec<_>, _>>()
                .to_db()?;
            Ok(sprints)
        })
    }

    pub fn update_sprint_sync(&self, id: &str, update: &UpdateSprint) -> Result<Sprint, DbError> {
        self.with_conn(|conn| {
            let mut sets = Vec::new();
            let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            if let Some(ref name) = update.name {
                sets.push("name = ?");
                values.push(Box::new(name.clone()));
            }
            if let Some(ref goal) = update.goal {
                sets.push("goal = ?");
                values.push(Box::new(goal.clone()));
            }
            if let Some(ref status) = update.status {
                sets.push("status = ?");
                values.push(Box::new(status.as_str().to_string()));
            }
            if let Some(ref starts_at) = update.starts_at {
                sets.push("starts_at = ?");
                values.push(Box::new(*starts_at));
            }
            if let Some(ref ends_at) = update.ends_at {
                sets.push("ends_at = ?");
                values.push(Box::new(*ends_at));
            }

            if sets.is_empty() {
                return conn
                    .query_row(
                        "SELECT * FROM sprints WHERE id = ?1",
                        params![id],
                        row_to_sprint,
                    )
                    .map_err(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => {
                            DbError::NotFound(format!("sprint {id}"))
                        }
                        other => DbError::Internal(other.to_string()),
                    });
            }

            sets.push("updated_at = ?");
            values.push(Box::new(Utc::now()));
            values.push(Box::new(id.to_string()));

            let sql = format!("UPDATE sprints SET {} WHERE id = ?", sets.join(", "));
            let params: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
            let changed = conn.execute(&sql, params.as_slice()).to_db()?;
            if changed == 0 {
                return Err(DbError::NotFound(format!("sprint {id}")));
            }

            conn.query_row(
                "SELECT * FROM sprints WHERE id = ?1",
                params![id],
                row_to_sprint,
            )
            .map_err(|e| DbError::Internal(e.to_string()))
        })
    }

    pub fn delete_sprint_sync(&self, id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            let changed = conn
                .execute("DELETE FROM sprints WHERE id = ?1", params![id])
                .to_db()?;
            if changed == 0 {
                return Err(DbError::NotFound(format!("sprint {id}")));
            }
            Ok(())
        })
    }
}
