use chrono::Utc;
use rusqlite::{params, Row};

use flowstate_core::project::{CreateProject, Project};

use crate::{Db, DbError};

fn row_to_project(row: &Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get("id")?,
        name: row.get("name")?,
        slug: row.get("slug")?,
        description: row.get("description")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

impl Db {
    pub fn create_project(&self, input: &CreateProject) -> Result<Project, DbError> {
        self.with_conn(|conn| {
            let id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now();
            conn.execute(
                "INSERT INTO projects (id, name, slug, description, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, input.name, input.slug, input.description, now, now],
            )?;
            let project = conn.query_row(
                "SELECT * FROM projects WHERE id = ?1",
                params![id],
                row_to_project,
            )?;
            Ok(project)
        })
    }

    pub fn get_project(&self, id: &str) -> Result<Project, DbError> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM projects WHERE id = ?1",
                params![id],
                row_to_project,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    DbError::NotFound(format!("project {id}"))
                }
                other => DbError::Sqlite(other),
            })
        })
    }

    pub fn get_project_by_slug(&self, slug: &str) -> Result<Project, DbError> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM projects WHERE slug = ?1",
                params![slug],
                row_to_project,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    DbError::NotFound(format!("project with slug '{slug}'"))
                }
                other => DbError::Sqlite(other),
            })
        })
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT * FROM projects ORDER BY name")?;
            let projects = stmt
                .query_map([], row_to_project)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(projects)
        })
    }

    pub fn delete_project(&self, id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            let changed = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
            if changed == 0 {
                return Err(DbError::NotFound(format!("project {id}")));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Db;
    use flowstate_core::project::CreateProject;

    #[test]
    fn test_project_crud() {
        let db = Db::open_in_memory().unwrap();

        let project = db
            .create_project(&CreateProject {
                name: "Test Project".into(),
                slug: "test-project".into(),
                description: "A test project".into(),
            })
            .unwrap();

        assert_eq!(project.name, "Test Project");
        assert_eq!(project.slug, "test-project");

        let fetched = db.get_project(&project.id).unwrap();
        assert_eq!(fetched.id, project.id);

        let by_slug = db.get_project_by_slug("test-project").unwrap();
        assert_eq!(by_slug.id, project.id);

        let all = db.list_projects().unwrap();
        assert_eq!(all.len(), 1);

        db.delete_project(&project.id).unwrap();
        let all = db.list_projects().unwrap();
        assert!(all.is_empty());
    }
}
