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

## Coverage

**All new code must have 100% line coverage** as reported by `flowstate-coverage`.

Run coverage:
```bash
# Inside nix dev shell:
flowstate-coverage

# Or from outside:
nix develop -c flowstate-coverage

# Override threshold (default is 29):
FLOWSTATE_COV_THRESHOLD=50 flowstate-coverage
```

The script starts ephemeral Postgres + Garage S3 instances, runs `cargo llvm-cov --workspace --all-features` with `--include-ignored` (to run postgres parity tests), and enforces a line coverage threshold.

## Clippy

Clippy runs with `-D warnings`. Common lints to watch for:
- `should_implement_trait`: use `parse_str()` instead of `from_str()` for enum parsing
- `collapsible_if`: flatten nested ifs
- `needless_borrow`: remove unnecessary `&` references
- `print_literal`: use format string directly

## Code Patterns

- **flowstate-core** is types-only (no IO). All enum parsing uses `parse_str()` (not `from_str()`) to avoid clippy `should_implement_trait`.
- **ApprovalStatus** derives `Default` with `#[default]` on the `None` variant.
- **Server routes** use `AppState = Arc<InnerAppState>` with `service: LocalService` and `db: Db`.
- **Runner** talks to the server exclusively via `HttpService` — no direct DB dependency.
- **DB migrations** use a `schema_version` table with versioned migration functions.

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
