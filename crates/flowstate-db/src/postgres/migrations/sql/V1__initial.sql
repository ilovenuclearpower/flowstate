-- V1: Full initial schema for Postgres
-- This is the Postgres equivalent of all SQLite migrations (v0 through v9).

CREATE TABLE projects (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    repo_url    TEXT NOT NULL DEFAULT '',
    repo_token  TEXT,
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
);

CREATE TABLE labels (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    color       TEXT NOT NULL DEFAULT '#808080',
    created_at  TIMESTAMPTZ NOT NULL
);
CREATE UNIQUE INDEX idx_labels_project_name ON labels(project_id, name);

CREATE TABLE sprints (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    goal        TEXT NOT NULL DEFAULT '',
    starts_at   TIMESTAMPTZ,
    ends_at     TIMESTAMPTZ,
    status      TEXT NOT NULL DEFAULT 'planned'
                    CHECK(status IN ('planned', 'active', 'completed')),
    created_at  TIMESTAMPTZ NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL
);

CREATE TABLE tasks (
    id                     TEXT PRIMARY KEY,
    project_id             TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    sprint_id              TEXT REFERENCES sprints(id) ON DELETE SET NULL,
    parent_id              TEXT REFERENCES tasks(id) ON DELETE SET NULL,
    title                  TEXT NOT NULL,
    description            TEXT NOT NULL DEFAULT '',
    reviewer               TEXT NOT NULL DEFAULT '',
    status                 TEXT NOT NULL DEFAULT 'todo'
                               CHECK(status IN (
                                   'todo', 'research', 'design', 'plan',
                                   'build', 'verify', 'done', 'cancelled'
                               )),
    priority               TEXT NOT NULL DEFAULT 'medium'
                               CHECK(priority IN ('urgent', 'high', 'medium', 'low', 'none')),
    sort_order             DOUBLE PRECISION NOT NULL DEFAULT 0,
    research_status        TEXT NOT NULL DEFAULT 'none',
    spec_status            TEXT NOT NULL DEFAULT 'none',
    plan_status            TEXT NOT NULL DEFAULT 'none',
    verify_status          TEXT NOT NULL DEFAULT 'none',
    spec_approved_hash     TEXT NOT NULL DEFAULT '',
    research_approved_hash TEXT NOT NULL DEFAULT '',
    research_feedback      TEXT NOT NULL DEFAULT '',
    spec_feedback          TEXT NOT NULL DEFAULT '',
    plan_feedback          TEXT NOT NULL DEFAULT '',
    verify_feedback        TEXT NOT NULL DEFAULT '',
    created_at             TIMESTAMPTZ NOT NULL,
    updated_at             TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_tasks_project ON tasks(project_id);
CREATE INDEX idx_tasks_status  ON tasks(project_id, status);
CREATE INDEX idx_tasks_sprint  ON tasks(sprint_id);
CREATE INDEX idx_tasks_parent  ON tasks(parent_id);

CREATE TABLE task_labels (
    task_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    label_id    TEXT NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, label_id)
);

CREATE TABLE verification_profiles (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL
);

CREATE TABLE verification_steps (
    id          TEXT PRIMARY KEY,
    profile_id  TEXT NOT NULL REFERENCES verification_profiles(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    command     TEXT NOT NULL,
    working_dir TEXT,
    sort_order  BIGINT NOT NULL DEFAULT 0,
    timeout_s   BIGINT NOT NULL DEFAULT 300,
    created_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_vsteps_profile ON verification_steps(profile_id, sort_order);

CREATE TABLE task_verifications (
    id          TEXT PRIMARY KEY,
    task_id     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    profile_id  TEXT REFERENCES verification_profiles(id) ON DELETE SET NULL,
    name        TEXT NOT NULL,
    command     TEXT NOT NULL,
    working_dir TEXT,
    sort_order  BIGINT NOT NULL DEFAULT 0,
    timeout_s   BIGINT NOT NULL DEFAULT 300,
    created_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_tverif_task ON task_verifications(task_id, sort_order);

CREATE TABLE verification_runs (
    id              TEXT PRIMARY KEY,
    task_id         TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    profile_id      TEXT REFERENCES verification_profiles(id) ON DELETE SET NULL,
    triggered_by    TEXT NOT NULL DEFAULT 'manual'
                        CHECK(triggered_by IN ('manual', 'mcp', 'agent')),
    status          TEXT NOT NULL DEFAULT 'running'
                        CHECK(status IN ('running', 'passed', 'failed', 'error', 'cancelled')),
    started_at      TIMESTAMPTZ NOT NULL,
    finished_at     TIMESTAMPTZ
);
CREATE INDEX idx_vruns_task ON verification_runs(task_id);

CREATE TABLE verification_run_steps (
    id          TEXT PRIMARY KEY,
    run_id      TEXT NOT NULL REFERENCES verification_runs(id) ON DELETE CASCADE,
    step_name   TEXT NOT NULL,
    command     TEXT NOT NULL,
    exit_code   INTEGER,
    stdout      TEXT NOT NULL DEFAULT '',
    stderr      TEXT NOT NULL DEFAULT '',
    started_at  TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ,
    sort_order  BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX idx_vrsteps_run ON verification_run_steps(run_id, sort_order);

CREATE TABLE api_keys (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL DEFAULT '',
    key_hash     TEXT NOT NULL UNIQUE,
    created_at   TEXT NOT NULL,
    last_used_at TEXT
);
CREATE INDEX idx_api_keys_hash ON api_keys(key_hash);

CREATE TABLE commit_links (
    id           TEXT PRIMARY KEY,
    task_id      TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    sha          TEXT NOT NULL,
    message      TEXT NOT NULL DEFAULT '',
    author       TEXT NOT NULL DEFAULT '',
    committed_at TIMESTAMPTZ,
    linked_at    TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_commits_task ON commit_links(task_id);
CREATE UNIQUE INDEX idx_commits_sha_task ON commit_links(sha, task_id);

CREATE TABLE task_links (
    id              TEXT PRIMARY KEY,
    source_task_id  TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    target_task_id  TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    link_type       TEXT NOT NULL CHECK(link_type IN ('blocks', 'relates_to', 'duplicates')),
    created_at      TIMESTAMPTZ NOT NULL,
    UNIQUE(source_task_id, target_task_id, link_type)
);
CREATE INDEX idx_task_links_source ON task_links(source_task_id);
CREATE INDEX idx_task_links_target ON task_links(target_task_id);

CREATE TABLE claude_runs (
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
    pr_number        BIGINT,
    branch_name      TEXT,
    progress_message TEXT,
    runner_id        TEXT,
    started_at       TIMESTAMPTZ NOT NULL,
    finished_at      TIMESTAMPTZ
);
CREATE INDEX idx_claude_runs_task ON claude_runs(task_id);
CREATE INDEX idx_claude_runs_status ON claude_runs(status);

CREATE TABLE attachments (
    id           TEXT PRIMARY KEY,
    task_id      TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    filename     TEXT NOT NULL,
    store_key    TEXT NOT NULL,
    size_bytes   BIGINT NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_attachments_task ON attachments(task_id);

CREATE TABLE task_prs (
    id            TEXT PRIMARY KEY,
    task_id       TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    claude_run_id TEXT REFERENCES claude_runs(id) ON DELETE SET NULL,
    pr_url        TEXT NOT NULL,
    pr_number     BIGINT NOT NULL,
    branch_name   TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_task_prs_task ON task_prs(task_id);
CREATE UNIQUE INDEX idx_task_prs_url ON task_prs(pr_url);

CREATE TABLE IF NOT EXISTS schema_version (
    version    INTEGER PRIMARY KEY,
    applied_at TIMESTAMPTZ NOT NULL
);
INSERT INTO schema_version (version, applied_at) VALUES (1, NOW());
