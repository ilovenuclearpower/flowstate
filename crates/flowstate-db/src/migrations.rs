use rusqlite::Connection;

use crate::DbError;

pub fn run(conn: &Connection) -> Result<(), DbError> {
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
    Ok(())
}
