-- Add role column to api_keys
ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'admin';

-- Runner handshakes table
CREATE TABLE IF NOT EXISTS runner_handshakes (
    id          TEXT PRIMARY KEY,
    runner_id   TEXT NOT NULL UNIQUE,
    hostname    TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL DEFAULT 'pending',
    api_key_id  TEXT,
    raw_key     TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

INSERT INTO schema_version (version, applied_at) VALUES (5, NOW());
