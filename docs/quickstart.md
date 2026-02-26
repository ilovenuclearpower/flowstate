# Quickstart

Local development setup using nix. No Docker required.

## Prerequisites

- [Nix](https://nixos.org/download/) with flakes enabled
- Git

## Setup

Clone the repository and enter the dev shell:

```bash
git clone https://github.com/ilovenuclearpower/flowstate.git
cd flowstate
nix develop
```

The dev shell provides the correct Rust toolchain, SQLite, Postgres, Garage S3, and all helper scripts.

## Running Locally

Open three terminals (all inside `nix develop`):

**Terminal 1 — Server:**

```bash
flowstate-server
```

Starts the server on `http://127.0.0.1:3710` with SQLite storage.

**Terminal 2 — Runner:**

```bash
runner-claude
```

Starts a runner that polls the server for work. Requires Claude CLI authentication (`claude login` or `claude setup-token`).

Other runner backends are available:

```bash
runner-gemini-pro    # Gemini 3.1 Pro (port 3712)
runner-gemini-flash  # Gemini 3 Flash (port 3713)
runner-opencode      # OpenCode CLI (port 3715)
```

**Terminal 3 — TUI:**

```bash
flowstate
```

Opens the task board. If no `--server` is given, the TUI auto-spawns its own server — skip Terminal 1 in that case.

## First Steps

1. Press `P` to create a project (set the repo URL and optional GitHub token)
2. Press `n` to create a task in the Todo column
3. Select a task and press `c` to trigger a Claude action (research, design, plan, build)
4. Watch the runner pick up work in Terminal 2

## Optional: Authentication

Generate an API key for server authentication:

```bash
flowstate-server keygen --name "dev-key"
```

Then pass it to the runner and TUI:

```bash
# Runner
runner-claude --api-key <key>

# TUI
flowstate --server http://127.0.0.1:3710 --api-key <key>
```

Or set `FLOWSTATE_API_KEY` as an environment variable.

## What's Next

- [Server configuration, Docker, and deployment](server.md)
- [Runner configuration and agent backends](runner.md)
- [GPU runner with RunPod and Tailscale](runner-gpu.md)
- [TUI keymaps and modes](tui.md)
- [Testing and coverage](testing.md)
