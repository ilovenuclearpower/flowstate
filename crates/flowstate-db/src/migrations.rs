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

    if current_version < 4 {
        let has_column = |table: &str, col: &str| -> bool {
            conn.prepare(&format!("SELECT {col} FROM {table} LIMIT 0"))
                .is_ok()
        };
        if !has_column("claude_runs", "progress_message") {
            conn.execute_batch(
                "ALTER TABLE claude_runs ADD COLUMN progress_message TEXT;",
            )?;
        }
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (4, datetime('now'))",
            [],
        )?;
    }

    if current_version < 5 {
        let has_column = |table: &str, col: &str| -> bool {
            conn.prepare(&format!("SELECT {col} FROM {table} LIMIT 0"))
                .is_ok()
        };
        if !has_column("projects", "repo_token") {
            conn.execute_batch(
                "ALTER TABLE projects ADD COLUMN repo_token TEXT;",
            )?;
        }
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (5, datetime('now'))",
            [],
        )?;
    }

    if current_version < 6 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS task_prs (
                id            TEXT PRIMARY KEY,
                task_id       TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                claude_run_id TEXT REFERENCES claude_runs(id) ON DELETE SET NULL,
                pr_url        TEXT NOT NULL,
                pr_number     INTEGER NOT NULL,
                branch_name   TEXT NOT NULL,
                created_at    TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_task_prs_task ON task_prs(task_id);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_task_prs_url ON task_prs(pr_url);",
        )?;
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (6, datetime('now'))",
            [],
        )?;
    }

    if current_version < 7 {
        // Rename disk_path → store_key in attachments table
        let has_column = |table: &str, col: &str| -> bool {
            conn.prepare(&format!("SELECT {col} FROM {table} LIMIT 0"))
                .is_ok()
        };
        if has_column("attachments", "disk_path") {
            conn.execute_batch("ALTER TABLE attachments RENAME COLUMN disk_path TO store_key;")?;

            // Convert absolute filesystem paths to relative object keys.
            // Existing disk_path values look like:
            //   /home/user/.local/share/flowstate/tasks/abc/attachments/foo.png
            // We need to strip the data_dir prefix to produce:
            //   tasks/abc/attachments/foo.png
            let data_prefix = crate::data_dir().to_string_lossy().to_string();
            let mut stmt =
                conn.prepare("SELECT id, store_key FROM attachments WHERE store_key LIKE '/%'")?;
            let rows: Vec<(String, String)> = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            for (id, abs_path) in rows {
                let relative = abs_path
                    .strip_prefix(&data_prefix)
                    .unwrap_or(&abs_path)
                    .trim_start_matches('/');
                conn.execute(
                    "UPDATE attachments SET store_key = ?1 WHERE id = ?2",
                    rusqlite::params![relative, id],
                )?;
            }
        }
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (7, datetime('now'))",
            [],
        )?;
    }

    if current_version < 8 {
        // v8: Five-phase workflow with review-distill support

        let has_column = |table: &str, col: &str| -> bool {
            conn.prepare(&format!("SELECT {col} FROM {table} LIMIT 0"))
                .is_ok()
        };

        // Add new approval and feedback columns to tasks
        if !has_column("tasks", "research_status") {
            conn.execute_batch(
                "ALTER TABLE tasks ADD COLUMN research_status TEXT NOT NULL DEFAULT 'none';
                 ALTER TABLE tasks ADD COLUMN verify_status TEXT NOT NULL DEFAULT 'none';
                 ALTER TABLE tasks ADD COLUMN research_approved_hash TEXT NOT NULL DEFAULT '';
                 ALTER TABLE tasks ADD COLUMN research_feedback TEXT NOT NULL DEFAULT '';
                 ALTER TABLE tasks ADD COLUMN spec_feedback TEXT NOT NULL DEFAULT '';
                 ALTER TABLE tasks ADD COLUMN plan_feedback TEXT NOT NULL DEFAULT '';
                 ALTER TABLE tasks ADD COLUMN verify_feedback TEXT NOT NULL DEFAULT '';",
            )?;
        }

        // Migrate task statuses: map old values to new
        conn.execute_batch(
            "UPDATE tasks SET status = 'todo' WHERE status = 'backlog';
             UPDATE tasks SET status = 'build' WHERE status = 'in_progress';
             UPDATE tasks SET status = 'verify' WHERE status = 'in_review';",
        )?;

        // Recreate claude_runs table with expanded CHECK constraint
        // to include the new action values (research, verify, and distill variants).
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS claude_runs_new (
                id               TEXT PRIMARY KEY,
                task_id          TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                action           TEXT NOT NULL CHECK(action IN (
                    'research', 'design', 'plan', 'build', 'verify',
                    'research_distill', 'design_distill', 'plan_distill', 'verify_distill'
                )),
                status           TEXT NOT NULL DEFAULT 'queued'
                                     CHECK(status IN ('queued','running','completed','failed','cancelled')),
                error_message    TEXT,
                exit_code        INTEGER,
                pr_url           TEXT,
                pr_number        INTEGER,
                branch_name      TEXT,
                progress_message TEXT,
                started_at       TEXT NOT NULL,
                finished_at      TEXT
            );

            INSERT OR IGNORE INTO claude_runs_new
                SELECT id, task_id, action, status, error_message, exit_code,
                       pr_url, pr_number, branch_name, progress_message,
                       started_at, finished_at
                FROM claude_runs;

            DROP TABLE claude_runs;
            ALTER TABLE claude_runs_new RENAME TO claude_runs;
            CREATE INDEX IF NOT EXISTS idx_claude_runs_task ON claude_runs(task_id);",
        )?;

        // Recreate tasks table with updated status CHECK constraint.
        // We need PRAGMA foreign_keys = OFF for this since tasks has FK references.
        conn.execute_batch("PRAGMA foreign_keys = OFF;")?;

        conn.execute_batch(
            "CREATE TABLE tasks_new (
                id                     TEXT PRIMARY KEY,
                project_id             TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                sprint_id              TEXT REFERENCES sprints(id) ON DELETE SET NULL,
                title                  TEXT NOT NULL,
                description            TEXT NOT NULL DEFAULT '',
                status                 TEXT NOT NULL DEFAULT 'todo'
                                           CHECK(status IN (
                                               'todo', 'research', 'design', 'plan',
                                               'build', 'verify', 'done', 'cancelled'
                                           )),
                priority               TEXT NOT NULL DEFAULT 'medium'
                                           CHECK(priority IN ('urgent', 'high', 'medium', 'low', 'none')),
                sort_order             REAL NOT NULL DEFAULT 0,
                created_at             TEXT NOT NULL,
                updated_at             TEXT NOT NULL,
                parent_id              TEXT REFERENCES tasks_new(id) ON DELETE SET NULL,
                reviewer               TEXT NOT NULL DEFAULT '',
                spec_status            TEXT NOT NULL DEFAULT 'none',
                plan_status            TEXT NOT NULL DEFAULT 'none',
                spec_approved_hash     TEXT NOT NULL DEFAULT '',
                research_status        TEXT NOT NULL DEFAULT 'none',
                verify_status          TEXT NOT NULL DEFAULT 'none',
                research_approved_hash TEXT NOT NULL DEFAULT '',
                research_feedback      TEXT NOT NULL DEFAULT '',
                spec_feedback          TEXT NOT NULL DEFAULT '',
                plan_feedback          TEXT NOT NULL DEFAULT '',
                verify_feedback        TEXT NOT NULL DEFAULT ''
            );

            INSERT INTO tasks_new (
                id, project_id, sprint_id, title, description, status, priority,
                sort_order, created_at, updated_at, parent_id, reviewer,
                spec_status, plan_status, spec_approved_hash,
                research_status, verify_status, research_approved_hash,
                research_feedback, spec_feedback, plan_feedback, verify_feedback
            )
            SELECT
                id, project_id, sprint_id, title, description, status, priority,
                sort_order, created_at, updated_at, parent_id, reviewer,
                spec_status, plan_status, spec_approved_hash,
                research_status, verify_status, research_approved_hash,
                research_feedback, spec_feedback, plan_feedback, verify_feedback
            FROM tasks;

            DROP TABLE tasks;
            ALTER TABLE tasks_new RENAME TO tasks;

            CREATE INDEX IF NOT EXISTS idx_tasks_project ON tasks(project_id);
            CREATE INDEX IF NOT EXISTS idx_tasks_status  ON tasks(project_id, status);
            CREATE INDEX IF NOT EXISTS idx_tasks_sprint  ON tasks(sprint_id);
            CREATE INDEX IF NOT EXISTS idx_tasks_parent  ON tasks(parent_id);",
        )?;

        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        // Verify FK integrity
        let mut fk_stmt = conn.prepare("PRAGMA foreign_key_check")?;
        let fk_errors: i64 = fk_stmt.query_map([], |_row| Ok(1))?.count() as i64;
        if fk_errors > 0 {
            eprintln!("WARNING: foreign key check found {fk_errors} issues after v8 migration");
        }

        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (8, datetime('now'))",
            [],
        )?;
    }

    if current_version < 9 {
        // v9: Salvage logic support — add TimedOut/Salvaging status variants and runner_id column

        // Recreate claude_runs table with expanded status CHECK and runner_id column
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS claude_runs_new (
                id               TEXT PRIMARY KEY,
                task_id          TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                action           TEXT NOT NULL CHECK(action IN (
                    'research', 'design', 'plan', 'build', 'verify',
                    'research_distill', 'design_distill', 'plan_distill', 'verify_distill'
                )),
                status           TEXT NOT NULL DEFAULT 'queued'
                                     CHECK(status IN (
                                         'queued', 'running', 'completed', 'failed',
                                         'cancelled', 'timed_out', 'salvaging'
                                     )),
                error_message    TEXT,
                exit_code        INTEGER,
                pr_url           TEXT,
                pr_number        INTEGER,
                branch_name      TEXT,
                progress_message TEXT,
                runner_id        TEXT,
                started_at       TEXT NOT NULL,
                finished_at      TEXT
            );

            INSERT OR IGNORE INTO claude_runs_new
                SELECT id, task_id, action, status, error_message, exit_code,
                       pr_url, pr_number, branch_name, progress_message,
                       NULL as runner_id,
                       started_at, finished_at
                FROM claude_runs;

            DROP TABLE claude_runs;
            ALTER TABLE claude_runs_new RENAME TO claude_runs;
            CREATE INDEX IF NOT EXISTS idx_claude_runs_task ON claude_runs(task_id);
            CREATE INDEX IF NOT EXISTS idx_claude_runs_status ON claude_runs(status);",
        )?;

        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (9, datetime('now'))",
            [],
        )?;
    }

    Ok(())
}
