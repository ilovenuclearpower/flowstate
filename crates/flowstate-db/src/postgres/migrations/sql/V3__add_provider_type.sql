ALTER TABLE projects ADD COLUMN IF NOT EXISTS provider_type TEXT;
ALTER TABLE projects ADD COLUMN IF NOT EXISTS skip_tls_verify BOOLEAN NOT NULL DEFAULT FALSE;

INSERT INTO schema_version (version, applied_at) VALUES (3, NOW());
