# Flowstate Runner Configuration

All configuration options for the `flowstate-runner` binary. Each option can be set via a CLI flag or an environment variable. CLI flags take precedence over environment variables.

## Server Connection

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--server-url` | `FLOWSTATE_SERVER_URL` | `http://127.0.0.1:3710` | URL of the Flowstate server to poll for work. |
| `--api-key` | `FLOWSTATE_API_KEY` | *(none)* | API key for authenticating with the server. Optional; if not set, requests are unauthenticated. |

## Polling

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--poll-interval` | *(none)* | `5` | Seconds between poll cycles. Each cycle claims work up to available capacity. |

## Workspaces

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--workspace-root` | `FLOWSTATE_WORKSPACE_ROOT` | `$XDG_DATA_HOME/flowstate/workspaces` or `~/.local/share/flowstate/workspaces` | Root directory for per-run workspace directories. Each run gets a subdirectory keyed by run ID. |

## Timeouts

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--light-timeout` | `FLOWSTATE_LIGHT_TIMEOUT` | `900` | Timeout in seconds for light actions: research, design, plan, verify (and their distill variants). |
| `--build-timeout` | `FLOWSTATE_BUILD_TIMEOUT` | `3600` | Timeout in seconds for build actions. Builds typically take longer due to code generation, testing, and PR creation. |
| `--kill-grace-period` | `FLOWSTATE_KILL_GRACE` | `10` | Seconds to wait after sending SIGTERM to a Claude CLI process before escalating to SIGKILL. |
| `--activity-timeout` | `FLOWSTATE_ACTIVITY_TIMEOUT` | `900` | Inactivity threshold in seconds (reserved for future use). |

## Concurrency

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--max-concurrent` | `FLOWSTATE_MAX_CONCURRENT` | `5` | Maximum number of runs executing simultaneously. Controls total parallelism across all action types. |
| `--max-builds` | `FLOWSTATE_MAX_BUILDS` | `1` | Maximum number of concurrent Build actions. Must be less than or equal to `--max-concurrent`. Keeps builds serialized to avoid branch/PR conflicts. |
| `--shutdown-timeout` | `FLOWSTATE_SHUTDOWN_TIMEOUT` | `120` | Seconds to wait for in-progress runs to complete during graceful shutdown (SIGINT/SIGTERM). After this timeout, remaining runs are force-killed. |

**Constraints:**
- `max_concurrent` must be >= 1
- `max_builds` must be >= 1
- `max_builds` must be <= `max_concurrent`

## Health & Monitoring

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--health-port` | *(none)* | `3711` | Port for the health check HTTP endpoint (`GET /health`). |

The health endpoint returns JSON with capacity and active run information:

```json
{
  "status": "ok",
  "role": "runner",
  "runner_id": "host-1",
  "capacity": {
    "max_concurrent": 5,
    "max_builds": 1,
    "active_total": 2,
    "active_builds": 1,
    "available": 3
  },
  "active_runs": [
    {
      "run_id": "abc-123",
      "task_id": "task-1",
      "action": "research",
      "elapsed_seconds": 120
    }
  ]
}
```

## Example Configurations

### Sequential (backwards-compatible)

```bash
flowstate-runner --max-concurrent 1
```

Processes one run at a time. Equivalent to the pre-parallelism behavior.

### Moderate Parallelism

```bash
flowstate-runner --max-concurrent 4 --max-builds 1
```

Up to 4 runs at once, but only 1 Build at a time. You could see 1 Build + 3 light actions, or 4 light actions.

### High Throughput

```bash
flowstate-runner --max-concurrent 8 --max-builds 1
```

Up to 8 concurrent runs with builds serialized. Requires sufficient disk space and network bandwidth.

### Environment Variables

```bash
export FLOWSTATE_SERVER_URL=https://flowstate.example.com
export FLOWSTATE_API_KEY=secret-key
export FLOWSTATE_MAX_CONCURRENT=4
export FLOWSTATE_MAX_BUILDS=1
export FLOWSTATE_SHUTDOWN_TIMEOUT=180
flowstate-runner
```

## TUI Configuration

The `flowstate` TUI binary accepts the following options:

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--server` | *(none)* | `http://127.0.0.1:3710` | URL of the Flowstate server. If omitted, the TUI auto-spawns a local `flowstate-server` process. |
| `--api-key` | `FLOWSTATE_API_KEY` | *(none)* | API key for authenticating with the server. Shared with the runner. |

When no `--server` flag is provided, the TUI:
1. Looks for a `flowstate-server` binary next to its own executable, then falls back to `PATH`.
2. Spawns the server on `127.0.0.1:3710`.
3. Waits up to 10 seconds for the server to become ready.
4. Terminates the server on exit.

See [docs/tui.md](tui.md) for the full keymap reference and mode documentation.

## Resource Planning

When running with concurrency > 1, consider:

- **Disk space**: Each concurrent run clones the repository into its own workspace. Budget `max_concurrent * max_repo_size` of disk space.
- **Network bandwidth**: Multiple simultaneous git clones and Claude CLI invocations increase network usage.
- **CPU/Memory**: Claude CLI processes and git operations consume system resources proportional to concurrency.
