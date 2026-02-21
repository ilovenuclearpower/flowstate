-- V2: Add required_capability column to claude_runs for capability-based routing.
ALTER TABLE claude_runs ADD COLUMN IF NOT EXISTS required_capability TEXT;

INSERT INTO schema_version (version, applied_at) VALUES (2, NOW());
