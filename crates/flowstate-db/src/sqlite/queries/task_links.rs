use chrono::Utc;
use rusqlite::{params, Row};

use flowstate_core::task_link::{CreateTaskLink, LinkType, TaskLink};

use crate::DbError;
use super::super::{SqliteDatabase, SqliteResultExt};

fn row_to_task_link(row: &Row) -> rusqlite::Result<TaskLink> {
    let link_type_str: String = row.get("link_type")?;
    Ok(TaskLink {
        id: row.get("id")?,
        source_task_id: row.get("source_task_id")?,
        target_task_id: row.get("target_task_id")?,
        link_type: LinkType::parse_str(&link_type_str).unwrap_or(LinkType::RelatesTo),
        created_at: row.get("created_at")?,
    })
}

impl SqliteDatabase {
    pub fn create_task_link_sync(
        &self,
        input: &CreateTaskLink,
    ) -> Result<TaskLink, DbError> {
        self.with_conn(|conn| {
            let id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now();
            conn.execute(
                "INSERT INTO task_links (id, source_task_id, target_task_id, link_type, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    input.source_task_id,
                    input.target_task_id,
                    input.link_type.as_str(),
                    now,
                ],
            )
            .to_db()?;
            conn.query_row(
                "SELECT * FROM task_links WHERE id = ?1",
                params![id],
                row_to_task_link,
            )
            .map_err(|e| DbError::Internal(e.to_string()))
        })
    }

    pub fn list_task_links_sync(&self, task_id: &str) -> Result<Vec<TaskLink>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT * FROM task_links
                     WHERE source_task_id = ?1 OR target_task_id = ?1
                     ORDER BY created_at DESC",
                )
                .to_db()?;
            let links = stmt
                .query_map(params![task_id], row_to_task_link)
                .to_db()?
                .collect::<Result<Vec<_>, _>>()
                .to_db()?;
            Ok(links)
        })
    }

    pub fn delete_task_link_sync(&self, id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            let changed = conn
                .execute("DELETE FROM task_links WHERE id = ?1", params![id])
                .to_db()?;
            if changed == 0 {
                return Err(DbError::NotFound(format!("task_link {id}")));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Db;
    use flowstate_core::project::CreateProject;
    use flowstate_core::task::{CreateTask, Priority, Status};
    use flowstate_core::task_link::{CreateTaskLink, LinkType};

    #[test]
    fn test_task_link_crud() {
        let db = Db::open_in_memory().unwrap();
        let project = db
            .create_project_sync(&CreateProject {
                name: "Test".into(),
                slug: "test".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .unwrap();

        let t1 = db
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
        let t2 = db
            .create_task_sync(&CreateTask {
                project_id: project.id,
                title: "Task 2".into(),
                description: String::new(),
                status: Status::Todo,
                priority: Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
            })
            .unwrap();

        let link = db
            .create_task_link_sync(&CreateTaskLink {
                source_task_id: t1.id.clone(),
                target_task_id: t2.id.clone(),
                link_type: LinkType::Blocks,
            })
            .unwrap();
        assert_eq!(link.link_type, LinkType::Blocks);

        let links = db.list_task_links_sync(&t1.id).unwrap();
        assert_eq!(links.len(), 1);

        let links = db.list_task_links_sync(&t2.id).unwrap();
        assert_eq!(links.len(), 1);

        db.delete_task_link_sync(&link.id).unwrap();
        let links = db.list_task_links_sync(&t1.id).unwrap();
        assert!(links.is_empty());
    }
}
