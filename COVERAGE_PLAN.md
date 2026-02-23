# Plan: Push Test Coverage Higher (Phases 3–5)

## Context
Coverage is at 36.4% line (after Phase 1+2 brought core/prompts/verify/db/store to near-100%). The remaining gap is ~11,000 uncovered lines dominated by:
- **flowstate-server** (0%, ~1,400 uncovered lines) — auth, crypto, watchdog, all route handlers
- **flowstate-runner** (0% except plan_parser, ~2,400 uncovered lines) — config, tracker, pipeline, workspace
- **flowstate-service** (0%, ~870 uncovered lines) — local service pass-through, error mapping
- **flowstate-tui** (minimal, ~3,300 uncovered lines) — app state machine, task board navigation

Strategy: test all **unit-testable pure logic** and **integration-testable server routes** (using in-memory SQLite + test router). Skip process-spawning code and entrypoints that require external binaries. Tests grouped thematically per CLAUDE.md conventions.

## 6 Parallel Tracks

### Track A: flowstate-server — auth + crypto
**Themes: `auth`, `crypto`**

**auth.rs** — new `#[cfg(test)] mod tests`:
- `sha256_hex_known_value`: hash "hello", compare to known SHA-256 hex digest
- `sha256_hex_empty`: hash empty string
- `generate_api_key_format`: starts with "fs_", total length == 46
- `generate_api_key_uniqueness`: two calls produce different keys
- `constant_time_eq_same`: equal strings → true
- `constant_time_eq_different`: differing strings → false
- `constant_time_eq_different_lengths`: different lengths → false
- `build_auth_config_no_keys`: no env var + empty DB → returns None
- `build_auth_config_with_env_key`: set FLOWSTATE_API_KEY → returns Some
- `build_auth_config_with_db_keys`: no env var, DB has keys → returns Some

**crypto.rs** — new `#[cfg(test)] mod tests`:
- `encrypt_decrypt_roundtrip`: encrypt → decrypt → matches original
- `encrypt_different_nonce`: same plaintext → different ciphertext each time
- `decrypt_invalid_base64`: garbage → error
- `decrypt_too_short`: <12 bytes → "ciphertext too short"
- `decrypt_wrong_key`: encrypt with key A, decrypt with key B → error
- `key_file_path_with_xdg`: set XDG_CONFIG_HOME → path contains it
- `key_file_path_with_home`: unset XDG, set HOME → path under .config

**Files**: `crates/flowstate-server/src/auth.rs`, `crates/flowstate-server/src/crypto.rs`

---

### Track B: flowstate-server — watchdog + simple route handlers
**Themes: `watchdog`, `routes_health`, `routes_sprints`, `routes_task_links`, `routes_task_prs`**

**watchdog.rs** — new `#[cfg(test)] mod tests`:
- `check_stale_runs_empty_db`: no runs → no errors
- `check_stale_runs_times_out_running`: create+claim run → check with future threshold → timed out
- `check_stale_runs_times_out_salvaging`: same for salvaging status

Route integration tests: build test app via `axum::Router` + `tower::ServiceExt::oneshot()` backed by in-memory SQLite + LocalStore.

**routes/health.rs** integration tests:
- `health_returns_ok`: GET /api/health → 200, body `{"status":"ok"}`
- `system_status_returns_json`: GET /api/status → 200, has runners + stuck_runs arrays

**routes/sprints.rs** integration tests:
- `sprint_crud_lifecycle`: POST create (201) → GET → PUT update → DELETE (204)

**routes/task_links.rs** integration tests:
- `task_link_crud`: POST create (201) → GET list → DELETE (204)

**routes/task_prs.rs** integration tests:
- `task_pr_create_and_list`: POST create (201) → GET list

**Files**: `crates/flowstate-server/src/watchdog.rs`, `crates/flowstate-server/src/routes/{health,sprints,task_links,task_prs}.rs`

---

### Track C: flowstate-server — heavy route handlers
**Themes: `routes_projects`, `routes_tasks`, `routes_claude_runs`**

All integration tests using in-memory SQLite + test router + LocalStore.

**routes/projects.rs**:
- `project_crud_lifecycle`: POST (201) → GET by id → GET by slug → GET list → PUT update → DELETE (204)
- `project_not_found`: GET unknown id → 404
- `project_set_and_get_repo_token`: PUT token → GET token → roundtrip matches
- `project_token_redaction`: GET does NOT include raw token, includes `has_repo_token: true`

**routes/tasks.rs**:
- `task_crud_lifecycle`: POST (201) → GET → PUT update → DELETE (204)
- `task_list_with_filters`: create with different statuses → filter works
- `task_count_by_status`: create tasks → GET count endpoint
- `task_children_endpoint`: create parent+children → GET children
- `task_spec_write_and_read`: PUT content → GET → body matches
- `task_plan_write_and_read`: same for plan
- `task_research_write_and_read`: same for research
- `task_verification_write_and_read`: same for verification

**routes/claude_runs.rs**:
- `trigger_and_list_runs`: POST trigger → GET list
- `trigger_build_requires_approvals`: POST Build without approved spec/plan → 400
- `claim_run_lifecycle`: create → POST claim → 200
- `claim_empty_returns_204`: no queued runs → 204
- `update_run_status`: claim → PUT completed
- `update_run_progress`: PUT progress message

**Files**: `crates/flowstate-server/src/routes/{projects,tasks,claude_runs,mod}.rs`

---

### Track D: flowstate-service — local + error mapping
**Themes: `local_service`, `error_mapping`**

**local.rs** — new `#[cfg(test)] mod tests`:
- `local_service_project_crud`: create → get → list → update → delete via TaskService
- `local_service_task_crud`: full task lifecycle
- `local_service_sprint_crud`: sprint operations
- `local_service_task_links`: link operations
- `local_service_task_prs`: PR operations
- `local_service_claude_run_lifecycle`: create → get → list
- `local_service_list_attachments_empty`: empty list
- `local_service_not_found_error`: get nonexistent → ServiceError::NotFound
- `error_from_db_not_found`: DbError::NotFound → ServiceError::NotFound
- `error_from_db_internal`: DbError::Internal → ServiceError::Internal

**traits.rs** — new `#[cfg(test)] mod tests`:
- `service_error_display`: verify Display for all 3 error variants

**Files**: `crates/flowstate-service/src/{local,traits}.rs`

---

### Track E: flowstate-runner — pure logic
**Themes: `config`, `run_tracker`, `plan_parser`, `pipeline_slugify`, `workspace_inject_token`**

**config.rs** — new `#[cfg(test)] mod tests`:
- `validate_ok`: valid config passes
- `validate_max_concurrent_zero`: → error
- `validate_max_builds_zero`: → error
- `validate_max_builds_exceeds_concurrent`: → error
- `timeout_for_action_build`: returns build_timeout duration
- `timeout_for_action_research`: returns light_timeout duration
- `is_build_action_true`: Build → true
- `is_build_action_false`: Research → false
- `build_backend_claude_cli`: → Ok
- `build_backend_opencode`: → Ok
- `build_backend_unknown`: → error
- `capability_heavy`: "heavy" → Ok(Heavy)
- `capability_invalid`: "turbo" → error

**run_tracker.rs** — new `#[cfg(test)] mod tests`:
- `new_tracker_is_empty`: active_count == 0, build_count == 0
- `insert_and_count`: insert 2 runs → count == 2
- `remove_decrements`: insert → remove → count == 0
- `active_build_count_filters`: Build + Research → build_count == 1
- `snapshot_returns_entries`: insert → snapshot has correct fields
- `default_trait`: RunTracker::default() works

**plan_parser.rs** — expand existing `mod tests`:
- `extract_with_dollar_prefix`: `$ cargo test` → strips `$`
- `extract_mixed_code_blocks_and_inline`: both styles in one plan
- `looks_like_command_known_prefixes`: cargo, npm, make, pytest, go, python, etc.
- `looks_like_command_rejects_prose`: "This is a comment" → false
- `section_boundary_ends_extraction`: next ### heading stops extraction
- `trailing_code_block_no_close`: code block at EOF with no closing section

**pipeline.rs** — new `#[cfg(test)] mod tests` (for `slugify` only):
- `slugify_simple`: "My Task" → "my-task"
- `slugify_special_chars`: "Hello, World!" → "hello-world"
- `slugify_consecutive_dashes`: "a--b" → "a-b"
- `slugify_truncates_at_50`: long title → 50 chars max
- `slugify_empty`: "" → ""

**workspace.rs** — new `#[cfg(test)] mod tests` (for `inject_token` only):
- `inject_token_https`: token + https URL → token injected
- `inject_token_non_https`: token + ssh URL → unchanged
- `inject_token_none`: None → unchanged
- `inject_token_empty_string`: Some("") → unchanged

**Files**: `crates/flowstate-runner/src/{config,run_tracker,plan_parser,pipeline,workspace}.rs`

---

### Track F: flowstate-tui — task_board + app state
**Themes: `task_board`, `state_machine`**

**task_board.rs** — expand existing `mod tests`:
- `navigate_right_at_boundary`: rightmost column → stays
- `navigate_left_at_boundary`: leftmost column → stays
- `navigate_down_past_end`: last task → stays
- `navigate_up_past_start`: first task → stays
- `empty_board_navigation`: h/j/k/l on empty → no crash
- `selected_task_after_selection`: selected_task() returns correct task

**app.rs** — new `#[cfg(test)] mod tests` (pure state tests, no terminal):
- `mode_default_is_normal`: App starts in Normal mode (requires mock service with in-memory DB)
- `project_field_enum_coverage`: exercise all ProjectField variants
- Additional mode transition tests if App::new() is constructible without terminal

**Files**: `crates/flowstate-tui/src/{components/task_board,app}.rs`

---

## Parallel Execution Matrix

All 6 tracks are independent — zero file overlap. Launch all simultaneously.

| Track | Agent | Crate | Files | Theme |
|-------|-------|-------|-------|-------|
| A | server-auth | flowstate-server | auth.rs, crypto.rs | auth, crypto |
| B | server-routes-light | flowstate-server | watchdog.rs, routes/{health,sprints,task_links,task_prs}.rs | watchdog, routes_health/sprints/links/prs |
| C | server-routes-heavy | flowstate-server | routes/{projects,tasks,claude_runs,mod}.rs | routes_projects/tasks/runs |
| D | service-tester | flowstate-service | local.rs, traits.rs | local_service, error_mapping |
| E | runner-logic | flowstate-runner | config.rs, run_tracker.rs, plan_parser.rs, pipeline.rs, workspace.rs | config, run_tracker, plan_parser, pipeline_slugify |
| F | tui-tester | flowstate-tui | task_board.rs, app.rs | task_board, state_machine |

## Verification
```bash
nix develop -c cargo test --workspace
nix develop -c cargo clippy --workspace -- -D warnings
nix develop -c cargo llvm-cov --workspace --all-features --no-fail-fast 2>&1 | tail -5
```
Expected: coverage rises from ~36% toward ~55-65%.

## What's NOT Covered (and why)
These modules require external services, process spawning, or terminal I/O:
- **runner**: main.rs (claim loop), executor.rs (HttpService calls), salvage.rs (git ops), process.rs (SIGTERM/SIGKILL), preflight.rs (binary checks), backend/*.rs (spawns claude/opencode), repo_provider/*.rs (gh CLI)
- **service**: http.rs (HTTP client to real server), blocking.rs (wraps http.rs)
- **tui**: main.rs (terminal setup/teardown)
- **mcp**: main.rs (3-line stub)
