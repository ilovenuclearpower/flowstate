# Flowstate Runner

Configuration, agent backends, and Docker deployment for `flowstate-runner`.

All options can be set via CLI flags or environment variables. CLI flags take precedence.

## Configuration

### Server Connection

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--server-url` | `FLOWSTATE_SERVER_URL` | `http://127.0.0.1:3710` | URL of the Flowstate server to poll for work |
| `--api-key` | `FLOWSTATE_API_KEY` | *(none)* | API key for authenticating with the server |

### Polling

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--poll-interval` | *(none)* | `5` | Seconds between poll cycles |

### Workspaces

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--workspace-root` | `FLOWSTATE_WORKSPACE_ROOT` | `~/.local/share/flowstate/workspaces` | Root directory for per-run workspace directories |

Each run gets a subdirectory keyed by run ID. Workspaces are cleaned up after the run completes.

### Timeouts

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--light-timeout` | `FLOWSTATE_LIGHT_TIMEOUT` | `1800` | Timeout (seconds) for light actions: research, design, plan, verify |
| `--build-timeout` | `FLOWSTATE_BUILD_TIMEOUT` | `3600` | Timeout (seconds) for build actions |
| `--kill-grace-period` | `FLOWSTATE_KILL_GRACE` | `10` | Seconds after SIGTERM before SIGKILL |
| `--activity-timeout` | `FLOWSTATE_ACTIVITY_TIMEOUT` | `900` | Inactivity threshold (reserved for future use) |

### Concurrency

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--max-concurrent` | `FLOWSTATE_MAX_CONCURRENT` | `5` | Maximum simultaneous runs |
| `--max-builds` | `FLOWSTATE_MAX_BUILDS` | `1` | Maximum concurrent build actions |
| `--shutdown-timeout` | `FLOWSTATE_SHUTDOWN_TIMEOUT` | `120` | Seconds to wait for in-progress runs during graceful shutdown |

**Constraints:**
- `max_concurrent` >= 1
- `max_builds` >= 1
- `max_builds` <= `max_concurrent`

### Capability Tier

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--runner-capability` | `FLOWSTATE_RUNNER_CAPABILITY` | `heavy` | `light`, `standard`, or `heavy`. A runner handles work at its tier and all lower tiers. |

## Agent Backends

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--agent-backend` | `FLOWSTATE_AGENT_BACKEND` | `claude-cli` | Backend: `claude-cli`, `gemini-cli`, or `opencode` |

### Claude CLI (default)

| Flag | Env Var | Description |
|------|---------|-------------|
| `--anthropic-base-url` | `FLOWSTATE_ANTHROPIC_BASE_URL` | Override Anthropic API base URL (for vLLM, Ollama, OpenRouter) |
| `--anthropic-auth-token` | `FLOWSTATE_ANTHROPIC_AUTH_TOKEN` | Override Anthropic auth token |
| `--anthropic-model` | `FLOWSTATE_ANTHROPIC_MODEL` | Model name hint (informational) |

Authentication (pick one):
1. **OAuth session** (interactive) — run `claude login`. Creates `~/.claude/.credentials.json`. Requires a browser, expires.
2. **Long-lived token** (headless) — run `claude setup-token` once, then set `FLOWSTATE_ANTHROPIC_AUTH_TOKEN` in your credentials file. Recommended for runners.
3. **Custom endpoint** — set `FLOWSTATE_ANTHROPIC_BASE_URL` and `FLOWSTATE_ANTHROPIC_AUTH_TOKEN` for vLLM, Ollama, OpenRouter, or any Anthropic-compatible API.

Headless setup:

```bash
# 1. Generate a long-lived token (interactive, one-time)
claude setup-token

# 2. Copy the token into your credentials file
echo 'FLOWSTATE_ANTHROPIC_AUTH_TOKEN=<your-token>' >> ~/.local/share/flowstate/runner/credentials/runner.env
```

### Gemini CLI

Requires `gemini` CLI: `npm install -g @google/gemini-cli` (Node.js >= 18).

| Flag | Env Var | Description |
|------|---------|-------------|
| `--gemini-api-key` | `FLOWSTATE_GEMINI_API_KEY` | Gemini API key |
| `--gemini-model` | `FLOWSTATE_GEMINI_MODEL` | Model identifier (e.g. `gemini-3.1-pro-preview`, `gemini-3-flash-preview`) |
| `--gemini-gcp-project` | `FLOWSTATE_GEMINI_GCP_PROJECT` | Google Cloud project ID (enables Vertex AI) |
| `--gemini-gcp-location` | `FLOWSTATE_GEMINI_GCP_LOCATION` | Google Cloud region (e.g. `us-central1`) |

Authentication (pick one):
1. **Gemini API key** — set `FLOWSTATE_GEMINI_API_KEY`. Works headless.
2. **Vertex AI** — set `FLOWSTATE_GEMINI_GCP_PROJECT` and `FLOWSTATE_GEMINI_GCP_LOCATION`. Uses service account or ADC.
3. **Google Login** — `gcloud auth application-default login`. Requires a browser, not suitable for headless.

### OpenCode

| Flag | Env Var | Description |
|------|---------|-------------|
| `--opencode-provider` | `FLOWSTATE_OPENCODE_PROVIDER` | Provider name (default: `anthropic`) |
| `--opencode-model` | `FLOWSTATE_OPENCODE_MODEL` | Model in `provider/model` format (e.g. `anthropic/claude-sonnet-4-5`) |
| `--opencode-api-key` | `FLOWSTATE_OPENCODE_API_KEY` | API key for the provider |
| `--opencode-base-url` | `FLOWSTATE_OPENCODE_BASE_URL` | Base URL override |

## Credentials File

Runner credentials are stored outside the repository:

```
~/.local/share/flowstate/runner/credentials/runner.env
```

This file is auto-loaded by the nix dev shell and all `runner-*` wrapper scripts.

Setup:

```bash
mkdir -p ~/.local/share/flowstate/runner/credentials
cp configuration/examples/runner.env.example ~/.local/share/flowstate/runner/credentials/runner.env
$EDITOR ~/.local/share/flowstate/runner/credentials/runner.env
chmod 600 ~/.local/share/flowstate/runner/credentials/runner.env
```

The file uses `KEY=value` format (no `export`), one variable per line. For Docker deployments, pass these as container environment variables instead.

## Health Endpoint

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--health-port` | *(none)* | `3711` | Port for the health check HTTP endpoint |

`GET /health` returns:

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

## Docker

Image: `ghcr.io/ilovenuclearpower/flowstate-runner`
Tags: `latest`, `sha-<commit>`

```bash
docker run \
  -e FLOWSTATE_SERVER_URL=http://your-server:3710 \
  -e FLOWSTATE_API_KEY=your-key \
  ghcr.io/ilovenuclearpower/flowstate-runner
```

All runner flags documented above can be set via their corresponding environment variables.

For the GPU runner with RunPod, see [runner-gpu.md](runner-gpu.md).

## Nix Dev Shell Scripts

The nix dev shell provides convenience wrappers that set the backend, source the credentials file, and exec into `flowstate-runner`:

| Command | Backend | Default Model | Health Port |
|---------|---------|---------------|-------------|
| `runner-claude` | `claude-cli` | *(from Claude CLI config)* | 3714 |
| `runner-gemini-pro` | `gemini-cli` | `gemini-3.1-pro-preview` | 3712 |
| `runner-gemini-flash` | `gemini-cli` | `gemini-3-flash-preview` | 3713 |
| `runner-opencode` | `opencode` | `anthropic/claude-sonnet-4-5` | 3715 |

All defaults can be overridden via env vars or extra CLI flags:

```bash
# Use a different model
FLOWSTATE_GEMINI_MODEL=gemini-3-pro runner-gemini-pro

# Override health port and concurrency
runner-gemini-flash --health-port 3720 --max-concurrent 2
```

## Example Configurations

### Sequential (backwards-compatible)

```bash
flowstate-runner --max-concurrent 1
```

One run at a time. Equivalent to pre-parallelism behavior.

### Moderate Parallelism

```bash
flowstate-runner --max-concurrent 4 --max-builds 1
```

Up to 4 runs at once, but only 1 build. You could see 1 build + 3 light actions, or 4 light actions.

### High Throughput

```bash
flowstate-runner --max-concurrent 8 --max-builds 1
```

Up to 8 concurrent runs with builds serialized. Requires sufficient disk and bandwidth.

### Gemini Runners Side-by-Side

Run two runners in separate terminals:

```bash
# Terminal 1: Gemini 3.1 Pro for heavy work
runner-gemini-pro

# Terminal 2: Gemini 3 Flash for lighter work
runner-gemini-flash
```

### Environment Variables Only

```bash
export FLOWSTATE_SERVER_URL=https://flowstate.example.com
export FLOWSTATE_API_KEY=secret-key
export FLOWSTATE_MAX_CONCURRENT=4
export FLOWSTATE_MAX_BUILDS=1
export FLOWSTATE_SHUTDOWN_TIMEOUT=180
flowstate-runner
```

## Resource Planning

When running with concurrency > 1:

- **Disk space**: Each concurrent run clones the repository. Budget `max_concurrent * max_repo_size`.
- **Network bandwidth**: Multiple simultaneous git clones and CLI invocations increase usage.
- **CPU/Memory**: Agent CLI processes and git operations scale with concurrency.
