# Flowstate — Project Rules

## Build Environment

**All cargo commands must run inside the nix dev shell:**

```bash
nix develop -c cargo build
nix develop -c cargo test
nix develop -c cargo clippy -- -D warnings
```

Or enter the shell first: `nix develop`, then run `cargo` commands directly.

The nix shell provides the correct rustc version and all required tooling (SQLite, Postgres, Garage, cargo-llvm-cov, etc.). System cargo/rustc may be too old.

## Testing

Tests are split into two tiers. See [docs/testing.md](docs/testing.md) for full details.

```bash
# Unit tests (no external services needed):
nix develop -c cargo test --workspace --all-features

# Full coverage (starts Postgres + Garage automatically):
nix develop -c flowstate-coverage
```

- **Unit tests** run with `cargo test --workspace --all-features`. No services needed.
- **Integration tests** (Postgres parity, S3 store) are marked `#[ignore]` and run automatically via `flowstate-coverage` with `--include-ignored`.
- Tests requiring external services must always use `#[ignore]`, never runtime skip guards.

## Coverage

**All new code must have coverage** as reported by `flowstate-coverage`.

```bash
# Default threshold (90% line coverage):
flowstate-coverage

# Custom threshold:
FLOWSTATE_COV_THRESHOLD=95 flowstate-coverage
```

The script starts ephemeral Postgres + Garage S3 instances, runs `cargo llvm-cov --workspace --all-features` with `--include-ignored`, and enforces a line coverage threshold.

## Clippy

Clippy runs with `-D warnings`. Common lints to watch for:
- `should_implement_trait`: use `parse_str()` instead of `from_str()` for enum parsing
- `collapsible_if`: flatten nested ifs
- `needless_borrow`: remove unnecessary `&` references
- `print_literal`: use format string directly

## Architecture

Workspace with crates: core, db, store, service, server, verify, prompts, runner, tui, mcp.

- **flowstate-core**: types only (no IO). All enum parsing uses `parse_str()` (not `from_str()`) to avoid clippy `should_implement_trait`.
- **flowstate-db**: SQLite via rusqlite (+ optional Postgres via sqlx). Versioned migrations in `migrations.rs` using a `schema_version` table.
- **flowstate-store**: Object storage abstraction. Local filesystem by default, S3-compatible (Garage) when configured. S3 support gated behind `s3` feature (on by default).
- **flowstate-service**: `TaskService` trait with `LocalService` (direct DB) and `HttpService` (REST client) implementations.
- **flowstate-server**: Axum HTTP server. Routes in `routes/` modules. `AppState = Arc<InnerAppState>` with `service: LocalService` and `db: Db`.
- **flowstate-runner**: Standalone binary that polls the server for jobs via `HttpService` and runs Claude CLI. No direct DB dependency.
- **flowstate-prompts**: Pure library for assembling prompts. No IO dependencies.
- **flowstate-verify**: Test runner that executes `VerificationStep` commands.
- **flowstate-tui**: Terminal UI with vim-style navigation.
- **flowstate-mcp**: MCP server (excluded from coverage).

## Code Patterns

- **ApprovalStatus** derives `Default` with `#[default]` on the `None` variant.
- Server routes use `AppState = Arc<InnerAppState>`.
- Runner talks to the server exclusively via `HttpService` — no direct DB dependency.
- DB migrations use a `schema_version` table with versioned migration functions.

## Test Organization

Tests are grouped by behavior domain. Each `#[cfg(test)] mod tests` block should have a clear thematic purpose:

- **flowstate-core**: `status_transitions`, `approval_lifecycle`, `parsing`, `serialization`, `capability_routing`
- **flowstate-db**: `sqlite_parity` / `postgres_parity`, `migrations`, `connection_management`, `query_edge_cases`
- **flowstate-store**: `local_store`, `s3_store`, `config_validation`
- **flowstate-prompts**: `action_routing`, `context_inclusion`, `distill_feedback`, `content_format`
- **flowstate-verify**: `command_execution`, `result_aggregation`, `timeout_enforcement`, `output_capture`
- **flowstate-service**: `local_service`, `http_service`, `blocking_service`, `error_mapping`
- **flowstate-server**: `auth`, `crypto`, `watchdog`, `routes_*` (projects, tasks, sprints, runs, links_prs, health)
- **flowstate-runner**: `config`, `run_tracker`, `plan_parser`, `workspace`, `process_management`, `preflight`, `executor_dispatch`, `pipeline_build`, `salvage`, `backend`
- **flowstate-tui**: `state_machine`, `data_operations`, `task_board`, `rendering`, `input_modes`
