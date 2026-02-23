# Testing

## Quick Reference

```bash
# Unit tests (no external services needed):
nix develop -c cargo test --workspace --all-features

# Full coverage (starts Postgres + Garage automatically):
nix develop -c flowstate-coverage

# Clippy:
nix develop -c cargo clippy -- -D warnings
```

## Test Tiers

Flowstate tests are split into two tiers based on whether they need external services.

### Tier 1 — Unit Tests

Run with `cargo test --workspace --all-features`. No external services required.

These tests cover all pure logic: type parsing, serialization, prompt assembly, route handlers (via in-memory SQLite), local filesystem store operations, config validation, and more.

### Tier 2 — Integration Tests (`#[ignore]`)

Run with `cargo test --workspace --all-features -- --include-ignored`, or automatically via `flowstate-coverage`.

These tests require external services (Postgres, Garage S3) and are marked `#[ignore]` so they are skipped during normal `cargo test` runs. The `flowstate-coverage` script handles starting ephemeral instances of these services before running tests with `--include-ignored`.

**Postgres parity tests** (`crates/flowstate-db/tests/postgres_parity.rs`):
Verify that every DB operation produces identical results on Postgres and SQLite. Requires an ephemeral Postgres instance on port 5711.

**S3 integration tests** (`crates/flowstate-store/src/s3.rs`):
Exercise the full S3 object store interface (CRUD, listing, concurrency, large objects, unicode). Requires an ephemeral Garage S3 instance on port 3910.

## Coverage

The `flowstate-coverage` command runs the full test suite with coverage instrumentation:

```bash
# Default threshold (90% line coverage):
flowstate-coverage

# Custom threshold:
FLOWSTATE_COV_THRESHOLD=95 flowstate-coverage
```

What it does:
1. Starts ephemeral Postgres on port 5711
2. Starts ephemeral Garage S3 on port 3910
3. Runs `cargo llvm-cov --workspace --all-features -- --include-ignored`
4. Enforces the line coverage threshold
5. Cleans up both services on exit

Files excluded from coverage reporting:
- `main.rs` (entry points)
- `mock.rs` and `test_helpers.rs` (test utilities)
- `backend/claude_cli.rs` and `backend/opencode.rs` (external process integrations)
- `repo_provider/github.rs` (external API integration)
- The `flowstate-mcp` crate (excluded entirely)

## Ephemeral Service Commands

These are available inside the nix dev shell for manual testing:

| Command | Description |
|---------|-------------|
| `pg-test-start` | Start ephemeral Postgres on port 5711 |
| `pg-test-stop` | Stop ephemeral Postgres and wipe data |
| `pg-test-info` | Show test Postgres credentials |
| `garage-test-start` | Start ephemeral Garage S3 on port 3910 |
| `garage-test-stop` | Stop ephemeral Garage and wipe data |
| `garage-test-info` | Show test S3 credentials |

Dev (persistent) variants are also available on ports 5710 (Postgres) and 3900 (Garage). See `pg-dev-*` and `garage-dev-*`.

## Test Organization

Tests are grouped by behavior domain. Each `#[cfg(test)] mod tests` block has a clear thematic purpose:

| Crate | Test Groups |
|-------|-------------|
| **flowstate-core** | `status_transitions`, `approval_lifecycle`, `parsing`, `serialization`, `capability_routing` |
| **flowstate-db** | `sqlite_parity` / `postgres_parity`, `migrations`, `connection_management`, `query_edge_cases` |
| **flowstate-store** | `local_store`, `s3_store`, `config_validation` |
| **flowstate-prompts** | `action_routing`, `context_inclusion`, `distill_feedback`, `content_format` |
| **flowstate-verify** | `command_execution`, `result_aggregation`, `timeout_enforcement`, `output_capture` |
| **flowstate-service** | `local_service`, `http_service`, `blocking_service`, `error_mapping` |
| **flowstate-server** | `auth`, `crypto`, `watchdog`, `routes_*` (projects, tasks, sprints, runs, links_prs, health) |
| **flowstate-runner** | `config`, `run_tracker`, `plan_parser`, `workspace`, `process_management`, `preflight`, `executor_dispatch`, `pipeline_build`, `salvage`, `backend` |
| **flowstate-tui** | `state_machine`, `data_operations`, `task_board`, `rendering`, `input_modes` |

## Writing New Tests

- All new code must have coverage as reported by `flowstate-coverage`.
- Tests that need external services (Postgres, S3) must be marked `#[ignore]`.
- Unit tests should work with `cargo test --workspace --all-features` alone.
- Use the existing test group names when adding tests to a crate, or create a new group with a clear thematic name.
