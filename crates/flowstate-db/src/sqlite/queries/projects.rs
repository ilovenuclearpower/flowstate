use chrono::Utc;
use rusqlite::{params, Row};

use flowstate_core::project::{CreateProject, Project, UpdateProject};

use crate::DbError;
use super::super::{SqliteDatabase, SqliteResultExt};

fn row_to_project(row: &Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get("id")?,
        name: row.get("name")?,
        slug: row.get("slug")?,
        description: row.get("description")?,
        repo_url: row.get("repo_url")?,
        repo_token: row.get("repo_token")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

impl SqliteDatabase {
    pub fn create_project_sync(&self, input: &CreateProject) -> Result<Project, DbError> {
        self.with_conn(|conn| {
            let id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now();
            conn.execute(
                "INSERT INTO projects (id, name, slug, description, repo_url, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id, input.name, input.slug, input.description, input.repo_url, now, now],
            )
            .to_db()?;
            let project = conn
                .query_row(
                    "SELECT * FROM projects WHERE id = ?1",
                    params![id],
                    row_to_project,
                )
                .to_db()?;
            Ok(project)
        })
    }

    pub fn get_project_sync(&self, id: &str) -> Result<Project, DbError> {
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
                other => DbError::Internal(other.to_string()),
            })
        })
    }

    pub fn get_project_by_slug_sync(&self, slug: &str) -> Result<Project, DbError> {
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
                other => DbError::Internal(other.to_string()),
            })
        })
    }

    pub fn list_projects_sync(&self) -> Result<Vec<Project>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT * FROM projects ORDER BY name").to_db()?;
            let projects = stmt
                .query_map([], row_to_project)
                .to_db()?
                .collect::<Result<Vec<_>, _>>()
                .to_db()?;
            Ok(projects)
        })
    }

    pub fn update_project_sync(
        &self,
        id: &str,
        update: &UpdateProject,
    ) -> Result<Project, DbError> {
        self.with_conn(|conn| {
            let mut sets = Vec::new();
            let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            if let Some(ref name) = update.name {
                sets.push("name = ?");
                values.push(Box::new(name.clone()));
            }
            if let Some(ref description) = update.description {
                sets.push("description = ?");
                values.push(Box::new(description.clone()));
            }
            if let Some(ref repo_url) = update.repo_url {
                sets.push("repo_url = ?");
                values.push(Box::new(repo_url.clone()));
            }
            if let Some(ref repo_token) = update.repo_token {
                sets.push("repo_token = ?");
                values.push(Box::new(repo_token.clone()));
            }

            if sets.is_empty() {
                return self.get_project_sync(id);
            }

            sets.push("updated_at = ?");
            values.push(Box::new(Utc::now()));
            values.push(Box::new(id.to_string()));

            let sql = format!(
                "UPDATE projects SET {} WHERE id = ?",
                sets.join(", ")
            );
            let params: Vec<&dyn rusqlite::ToSql> =
                values.iter().map(|v| v.as_ref()).collect();
            let changed = conn.execute(&sql, params.as_slice()).to_db()?;
            if changed == 0 {
                return Err(DbError::NotFound(format!("project {id}")));
            }

            conn.query_row(
                "SELECT * FROM projects WHERE id = ?1",
                params![id],
                row_to_project,
            )
            .map_err(|e| DbError::Internal(e.to_string()))
        })
    }

    pub fn delete_project_sync(&self, id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            let changed =
                conn.execute("DELETE FROM projects WHERE id = ?1", params![id]).to_db()?;
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
            .create_project_sync(&CreateProject {
                name: "Test Project".into(),
                slug: "test-project".into(),
                description: "A test project".into(),
                repo_url: "https://github.com/test/repo".into(),
            })
            .unwrap();

        assert_eq!(project.name, "Test Project");
        assert_eq!(project.slug, "test-project");
        assert_eq!(project.repo_url, "https://github.com/test/repo");

        let fetched = db.get_project_sync(&project.id).unwrap();
        assert_eq!(fetched.id, project.id);

        let by_slug = db.get_project_by_slug_sync("test-project").unwrap();
        assert_eq!(by_slug.id, project.id);

        let all = db.list_projects_sync().unwrap();
        assert_eq!(all.len(), 1);

        db.delete_project_sync(&project.id).unwrap();
        let all = db.list_projects_sync().unwrap();
        assert!(all.is_empty());
    }
}
