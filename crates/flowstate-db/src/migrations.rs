use rusqlite::Connection;

use crate::DbError;

pub fn run(conn: &Connection) -> Result<(), DbError> {
    // Original schema — idempotent CREATE TABLE IF NOT EXISTS
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS projects (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            slug        TEXT NOT NULL UNIQUE,
            description TEXT NOT NULL DEFAULT '',
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS labels (
            id          TEXT PRIMARY KEY,
            project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            color       TEXT NOT NULL DEFAULT '#808080',
            created_at  TEXT NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_labels_project_name
            ON labels(project_id, name);

        CREATE TABLE IF NOT EXISTS sprints (
            id          TEXT PRIMARY KEY,
            project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            goal        TEXT NOT NULL DEFAULT '',
            starts_at   TEXT,
            ends_at     TEXT,
            status      TEXT NOT NULL DEFAULT 'planned'
                            CHECK(status IN ('planned', 'active', 'completed')),
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tasks (
            id          TEXT PRIMARY KEY,
            project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            sprint_id   TEXT REFERENCES sprints(id) ON DELETE SET NULL,
            title       TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            status      TEXT NOT NULL DEFAULT 'backlog'
                            CHECK(status IN (
                                'backlog', 'todo', 'in_progress',
                                'in_review', 'done', 'cancelled'
                            )),
            priority    TEXT NOT NULL DEFAULT 'medium'
                            CHECK(priority IN ('urgent', 'high', 'medium', 'low', 'none')),
            sort_order  REAL NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_tasks_project ON tasks(project_id);
        CREATE INDEX IF NOT EXISTS idx_tasks_status  ON tasks(project_id, status);
        CREATE INDEX IF NOT EXISTS idx_tasks_sprint  ON tasks(sprint_id);

        CREATE TABLE IF NOT EXISTS task_labels (
            task_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            label_id    TEXT NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
            PRIMARY KEY (task_id, label_id)
        );

        CREATE TABLE IF NOT EXISTS verification_profiles (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL UNIQUE,
            description TEXT NOT NULL DEFAULT '',
            created_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS verification_steps (
            id          TEXT PRIMARY KEY,
            profile_id  TEXT NOT NULL REFERENCES verification_profiles(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            command     TEXT NOT NULL,
            working_dir TEXT,
            sort_order  INTEGER NOT NULL DEFAULT 0,
            timeout_s   INTEGER NOT NULL DEFAULT 300,
            created_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_vsteps_profile
            ON verification_steps(profile_id, sort_order);

        CREATE TABLE IF NOT EXISTS task_verifications (
            id          TEXT PRIMARY KEY,
            task_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            profile_id  TEXT REFERENCES verification_profiles(id) ON DELETE SET NULL,
            name        TEXT NOT NULL,
            command     TEXT NOT NULL,
            working_dir TEXT,
            sort_order  INTEGER NOT NULL DEFAULT 0,
            timeout_s   INTEGER NOT NULL DEFAULT 300,
            created_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_tverif_task
            ON task_verifications(task_id, sort_order);

        CREATE TABLE IF NOT EXISTS verification_runs (
            id              TEXT PRIMARY KEY,
            task_id         TEXT REFERENCES tasks(id) ON DELETE CASCADE,
            profile_id      TEXT REFERENCES verification_profiles(id) ON DELETE SET NULL,
            triggered_by    TEXT NOT NULL DEFAULT 'manual'
                                CHECK(triggered_by IN ('manual', 'mcp', 'agent')),
            status          TEXT NOT NULL DEFAULT 'running'
                                CHECK(status IN ('running', 'passed', 'failed', 'error', 'cancelled')),
            started_at      TEXT NOT NULL,
            finished_at     TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_vruns_task ON verification_runs(task_id);

        CREATE TABLE IF NOT EXISTS verification_run_steps (
            id          TEXT PRIMARY KEY,
            run_id      TEXT NOT NULL REFERENCES verification_runs(id) ON DELETE CASCADE,
            step_name   TEXT NOT NULL,
            command     TEXT NOT NULL,
            exit_code   INTEGER,
            stdout      TEXT NOT NULL DEFAULT '',
            stderr      TEXT NOT NULL DEFAULT '',
            started_at  TEXT NOT NULL,
            finished_at TEXT,
            sort_order  INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_vrsteps_run
            ON verification_run_steps(run_id, sort_order);

        CREATE TABLE IF NOT EXISTS api_keys (
            id           TEXT PRIMARY KEY,
            name         TEXT NOT NULL DEFAULT '',
            key_hash     TEXT NOT NULL UNIQUE,
            created_at   TEXT NOT NULL,
            last_used_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);

        CREATE TABLE IF NOT EXISTS commit_links (
            id           TEXT PRIMARY KEY,
            task_id      TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            sha          TEXT NOT NULL,
            message      TEXT NOT NULL DEFAULT '',
            author       TEXT NOT NULL DEFAULT '',
            committed_at TEXT,
            linked_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_commits_task ON commit_links(task_id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_commits_sha_task
            ON commit_links(sha, task_id);
        ",
    )?;

    // Versioned migrations
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version    INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if current_version < 1 {
        // v1: project repo_url, task new columns, task_links, claude_runs, attachments
        // Use a helper to check if column exists before ALTER TABLE
        let has_column = |table: &str, col: &str| -> bool {
            conn.prepare(&format!("SELECT {col} FROM {table} LIMIT 0"))
                .is_ok()
        };

        if !has_column("projects", "repo_url") {
            conn.execute_batch("ALTER TABLE projects ADD COLUMN repo_url TEXT NOT NULL DEFAULT '';")?;
        }

        if !has_column("tasks", "parent_id") {
            conn.execute_batch(
                "ALTER TABLE tasks ADD COLUMN parent_id TEXT REFERENCES tasks(id) ON DELETE SET NULL;
                 ALTER TABLE tasks ADD COLUMN reviewer TEXT NOT NULL DEFAULT '';
                 ALTER TABLE tasks ADD COLUMN spec_status TEXT NOT NULL DEFAULT 'none';
                 ALTER TABLE tasks ADD COLUMN plan_status TEXT NOT NULL DEFAULT 'none';",
            )?;
        }

        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_tasks_parent ON tasks(parent_id);

             CREATE TABLE IF NOT EXISTS task_links (
                 id              TEXT PRIMARY KEY,
                 source_task_id  TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 target_task_id  TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 link_type       TEXT NOT NULL CHECK(link_type IN ('blocks','relates_to','duplicates')),
                 created_at      TEXT NOT NULL,
                 UNIQUE(source_task_id, target_task_id, link_type)
             );
             CREATE INDEX IF NOT EXISTS idx_task_links_source ON task_links(source_task_id);
             CREATE INDEX IF NOT EXISTS idx_task_links_target ON task_links(target_task_id);

             CREATE TABLE IF NOT EXISTS claude_runs (
                 id            TEXT PRIMARY KEY,
                 task_id       TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 action        TEXT NOT NULL CHECK(action IN ('design','plan','build')),
                 status        TEXT NOT NULL DEFAULT 'queued'
                                   CHECK(status IN ('queued','running','completed','failed','cancelled')),
                 error_message TEXT,
                 exit_code     INTEGER,
                 started_at    TEXT NOT NULL,
                 finished_at   TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_claude_runs_task ON claude_runs(task_id);

             CREATE TABLE IF NOT EXISTS attachments (
                 id           TEXT PRIMARY KEY,
                 task_id      TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                 filename     TEXT NOT NULL,
                 disk_path    TEXT NOT NULL,
                 size_bytes   INTEGER NOT NULL DEFAULT 0,
                 created_at   TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_attachments_task ON attachments(task_id);",
        )?;

        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (1, datetime('now'))",
            [],
        )?;
    }

    if current_version < 2 {
        let has_column = |table: &str, col: &str| -> bool {
            conn.prepare(&format!("SELECT {col} FROM {table} LIMIT 0"))
                .is_ok()
        };
        if !has_column("tasks", "spec_approved_hash") {
            conn.execute_batch(
                "ALTER TABLE tasks ADD COLUMN spec_approved_hash TEXT NOT NULL DEFAULT '';",
            )?;
        }
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (2, datetime('now'))",
            [],
        )?;
    }

    if current_version < 3 {
        let has_column = |table: &str, col: &str| -> bool {
            conn.prepare(&format!("SELECT {col} FROM {table} LIMIT 0"))
                .is_ok()
        };
        if !has_column("claude_runs", "pr_url") {
            conn.execute_batch(
                "ALTER TABLE claude_runs ADD COLUMN pr_url TEXT;
                 ALTER TABLE claude_runs ADD COLUMN pr_number INTEGER;
                 ALTER TABLE claude_runs ADD COLUMN branch_name TEXT;",
            )?;
        }
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (3, datetime('now'))",
            [],
        )?;
    }

    Ok(())
}
