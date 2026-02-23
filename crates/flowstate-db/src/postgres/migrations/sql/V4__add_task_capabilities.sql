ALTER TABLE tasks ADD COLUMN research_capability VARCHAR(255) NULL;
ALTER TABLE tasks ADD COLUMN design_capability VARCHAR(255) NULL;
ALTER TABLE tasks ADD COLUMN plan_capability VARCHAR(255) NULL;
ALTER TABLE tasks ADD COLUMN build_capability VARCHAR(255) NULL;
ALTER TABLE tasks ADD COLUMN verify_capability VARCHAR(255) NULL;
INSERT INTO schema_version (version, applied_at) VALUES (4, NOW());
