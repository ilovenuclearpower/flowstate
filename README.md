# flowstate

A task management system with AI integration for designing, planning, and building software features. Flowstate gives you a terminal Kanban board backed by an HTTP API, with an approval-gated workflow that orchestrates AI agents to produce research, specifications, implementation plans, and working code.

## Architecture

```
flowstate (TUI client)           flowstate-runner (worker)
    |                                 |
    | HTTP                            | HTTP (polling)
    v                                 v
flowstate-server (axum REST API)
    |                |
    v                v
flowstate-db     flowstate-store
(SQLite/Postgres)  (local / S3)
```

Ten crates, layered by responsibility:

| Crate | Purpose |
|-------|---------|
| `flowstate-core` | Domain types: Task, Project, Sprint, ClaudeRun, ApprovalStatus |
| `flowstate-db` | SQLite and Postgres persistence with versioned migrations |
| `flowstate-store` | Object store abstraction (local filesystem or S3-compatible) |
| `flowstate-service` | `TaskService` trait with `LocalService` (direct DB) and `HttpService` (HTTP client) |
| `flowstate-server` | Axum REST API, Bearer token auth, admin API, pod manager |
| `flowstate-runner` | Standalone worker that polls for jobs and runs AI agents |
| `flowstate-prompts` | Prompt assembly for AI actions (pure library, no IO) |
| `flowstate-tui` | Ratatui/Crossterm terminal UI with Kanban board and Ops dashboard |
| `flowstate-verify` | Async verification step runner with timeout and fail-fast |
| `flowstate-mcp` | Model Context Protocol server |

## Installation

### Pre-built Binaries

Download from [GitHub Releases](https://github.com/ilovenuclearpower/flowstate/releases):

```bash
# macOS (Apple Silicon)
curl -L -o flowstate https://github.com/ilovenuclearpower/flowstate/releases/latest/download/flowstate-macos-aarch64

# macOS (Intel)
curl -L -o flowstate https://github.com/ilovenuclearpower/flowstate/releases/latest/download/flowstate-macos-x86_64

# Linux (x86_64)
curl -L -o flowstate https://github.com/ilovenuclearpower/flowstate/releases/latest/download/flowstate-linux-x86_64

chmod +x flowstate
sudo mv flowstate /usr/local/bin/
```

### Nix

```bash
# Run without installing
nix run github:ilovenuclearpower/flowstate

# Install into your profile
nix profile install github:ilovenuclearpower/flowstate
```

**NixOS (`configuration.nix`):**

```nix
{
  inputs.flowstate.url = "github:ilovenuclearpower/flowstate";

  outputs = { nixpkgs, flowstate, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ({ pkgs, ... }: {
          environment.systemPackages = [
            flowstate.packages.${pkgs.system}.default
          ];
        })
      ];
    };
  };
}
```

**home-manager:**

```nix
inputs.flowstate.url = "github:ilovenuclearpower/flowstate";

# In your home config:
home.packages = [ inputs.flowstate.packages.${pkgs.system}.default ];
```

### Docker (server + runner)

```bash
curl -O https://raw.githubusercontent.com/ilovenuclearpower/flowstate/main/docker-compose.yml
curl -O https://raw.githubusercontent.com/ilovenuclearpower/flowstate/main/.env.example
cp .env.example .env
docker compose up -d server
```

See [Self-Hosting Guide](docs/self-hosting.md) for the full walkthrough.

## Features

### Kanban Board
Terminal-based board with columns for Todo, Research, Design, Plan, Build, Verify, and Done. Navigate with hjkl/arrow keys, move tasks between columns, set priorities, manage multiple projects and sprints.

### Research / Design / Plan / Build / Verify Workflow
Each task supports a multi-phase AI workflow:

1. **Research** -- AI gathers context about the problem domain
2. **Design** -- AI produces a technical specification
3. **Plan** -- AI produces a structured implementation plan (gated on spec approval)
4. **Build** -- AI implements the plan against a git checkout, opens a PR
5. **Verify** -- Automated validation commands confirm the build works

Specs and plans go through an approval cycle (Pending -> Approved/Rejected). Editing an approved spec automatically revokes approval and requires re-review.

### Ops Dashboard
Press `2` to switch to the Ops tab for server administration:
- **API Keys** -- Generate, list, and revoke API keys
- **Runners** -- Monitor connected runners, their capability tiers, and saturation
- **GPU** -- Start/stop RunPod GPU pods when configured

### Setup Wizard
On first connection to a fresh server, the TUI runs a setup wizard that generates your admin API key and saves it locally. No CLI access to the server container needed.

### Credential Persistence
The TUI saves your API key and server URL to `~/.config/flowstate/tui-credentials`. After initial setup, subsequent launches need no flags.

### Authentication
API key system with SHA256 hashing and constant-time comparison. Keys can be managed via:
- The **setup wizard** (first key, automatic)
- The **Ops tab** in the TUI (generate/revoke keys)
- The **admin API** (`/api/admin/api-keys`)
- The **CLI** (`flowstate-server keygen`)

Auth activates at runtime when the first key is created -- no server restart needed.

### Multiple Agent Backends
The runner supports multiple AI backends:
- `claude-cli` -- Claude Code CLI (default)
- `gemini-cli` -- Gemini Pro / Flash
- `opencode` -- OpenCode CLI

### Editor Integration
Press `S` or `I` in task detail to open specs/plans in `$EDITOR`. Changes are synced back to the server on save.

### Subtasks
Create subtasks from task detail with `n`. Subtasks use a simplified flow: Todo -> Build -> Verify -> Done.

### Sprints
Group tasks into sprints. Press `x` to open the sprint list, `X` to clear the sprint filter.

## Self-Hosting with Docker

The recommended way to deploy Flowstate:

1. `docker compose up -d server` -- start the server
2. `flowstate --server http://your-server:3710` -- setup wizard generates your admin key
3. Press `2` in the TUI, generate a runner key from the Ops tab
4. Set `FLOWSTATE_API_KEY` and `ANTHROPIC_API_KEY` in `.env`
5. `docker compose up -d runner` -- start the runner

See [docs/self-hosting.md](docs/self-hosting.md) for the full guide including Postgres and S3 configuration.

## Quickstart (Local Development)

Requires [Nix](https://nixos.org/) with flakes enabled.

```bash
nix develop
cargo build --workspace

# Run the TUI (auto-spawns a local server on port 3710)
cargo run -p flowstate-tui
```

Or run components separately:

```bash
# Terminal 1: Server
cargo run -p flowstate-server

# Terminal 2: Runner (requires Claude CLI auth)
runner-claude

# Terminal 3: TUI
cargo run -p flowstate-tui
```

Other runner backends are available in the dev shell: `runner-gemini-pro`, `runner-gemini-flash`, `runner-opencode`.

## Keyboard Shortcuts

### Board (Normal Mode)

| Key | Action |
|-----|--------|
| `h`/`l` | Switch columns |
| `j`/`k` | Navigate tasks |
| `n` | New task |
| `Enter` | Task detail |
| `m`/`M` | Move task forward/back |
| `d` | Delete task |
| `p` | Set priority |
| `P` | Project switcher |
| `x` | Sprint list |
| `X` | Clear sprint filter |
| `H` | Health checks |
| `1`/`2` | Switch to Board/Ops tab |
| `Tab` | Toggle tabs |
| `q` | Quit |

### Task Detail

| Key | Action |
|-----|--------|
| `t` | Edit title |
| `e` | Edit description |
| `n` | Create subtask |
| `p` | Set priority |
| `m` | Move to next status |
| `d` | Delete |
| `c` | AI actions (research/design/plan/build/verify) |
| `s`/`S` | View/edit spec |
| `i`/`I` | View/edit plan |
| `w`/`W` | View/edit research |
| `v`/`V` | View/edit verification |
| `a` | Approve/reject artifact |
| `Esc` | Back |

### Ops Tab

| Key | Action |
|-----|--------|
| `j`/`k` | Navigate lists |
| `g` | Generate API key |
| `d` | Revoke selected key |
| `s`/`S` | Start/stop GPU pod |
| `r` | Refresh |

## API Endpoints

### Public (no auth)

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/health` | Health check |
| `GET` | `/api/setup/status` | Check if setup is needed |
| `POST` | `/api/setup/init` | Generate first API key |

### Admin

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/admin/api-keys` | List API keys |
| `POST` | `/api/admin/api-keys` | Generate new API key |
| `DELETE` | `/api/admin/api-keys/{id}` | Revoke API key |

### Projects

| Method | Path | Description |
|--------|------|-------------|
| `GET/POST` | `/api/projects` | List/create projects |
| `GET/PUT/DELETE` | `/api/projects/{id}` | Get/update/delete project |
| `GET` | `/api/projects/by-slug/{slug}` | Get project by slug |

### Tasks

| Method | Path | Description |
|--------|------|-------------|
| `GET/POST` | `/api/tasks` | List/create tasks |
| `GET/PUT/DELETE` | `/api/tasks/{id}` | Get/update/delete task |
| `GET` | `/api/tasks/{id}/children` | List subtasks |
| `GET` | `/api/tasks/count-by-status` | Count tasks per status |
| `GET/PUT` | `/api/tasks/{id}/spec` | Read/write specification |
| `GET/PUT` | `/api/tasks/{id}/plan` | Read/write plan |
| `GET/PUT` | `/api/tasks/{id}/research` | Read/write research |
| `GET/PUT` | `/api/tasks/{id}/verification` | Read/write verification |

### Claude Runs

| Method | Path | Description |
|--------|------|-------------|
| `GET/POST` | `/api/tasks/{id}/claude-runs` | List/trigger runs |
| `GET` | `/api/claude-runs/{id}` | Get run status |
| `GET` | `/api/claude-runs/{id}/output` | Get run output |
| `PUT` | `/api/claude-runs/{id}/status` | Update run status |
| `POST` | `/api/claude-runs/claim` | Claim next queued run (runner) |
| `POST` | `/api/runners/register` | Register/heartbeat runner |

### Infrastructure

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/status` | System status with runner info |
| `GET` | `/api/infra/gpu-status` | GPU pod status |
| `POST` | `/api/infra/gpu/start` | Start GPU pod |
| `POST` | `/api/infra/gpu/stop` | Stop GPU pod (drain) |
| `GET` | `/api/infra/runners` | List registered runners |

### Sprints, Links, PRs

| Method | Path | Description |
|--------|------|-------------|
| `GET/POST` | `/api/sprints` | List/create sprints |
| `GET/PUT/DELETE` | `/api/sprints/{id}` | Get/update/delete sprint |
| `GET/POST` | `/api/tasks/{id}/links` | List/create task links |
| `GET/POST` | `/api/tasks/{id}/prs` | List/create task PRs |

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `FLOWSTATE_PORT` | `3710` | Server listen port |
| `FLOWSTATE_BIND` | `0.0.0.0` | Server bind address |
| `FLOWSTATE_DB_BACKEND` | `sqlite` | Database backend (`sqlite` or `postgres`) |
| `FLOWSTATE_API_KEY` | *(none)* | API key for auth |
| `FLOWSTATE_S3_ENDPOINT` | *(none)* | S3 endpoint (enables S3 store) |
| `FLOWSTATE_S3_BUCKET` | *(none)* | S3 bucket name |
| `ANTHROPIC_API_KEY` | *(none)* | Anthropic API key (runner) |

Data is stored at `~/.local/share/flowstate/` (or `$XDG_DATA_HOME/flowstate/`).

See [docs/server.md](docs/server.md) and [docs/runner.md](docs/runner.md) for full configuration reference.

## Documentation

| Guide | Description |
|-------|-------------|
| [Self-Hosting](docs/self-hosting.md) | Docker Compose setup, first-time wizard, TUI installation |
| [TUI](docs/tui.md) | Installation, credential persistence, full keymap reference |
| [Server](docs/server.md) | Server configuration, Docker, Postgres, S3, RunPod |
| [Runner](docs/runner.md) | Runner configuration, agent backends, concurrency |
| [GPU Runner](docs/runner-gpu.md) | RunPod GPU runner with Tailscale networking |
| [Quickstart](docs/quickstart.md) | Local development setup with nix |
| [Testing](docs/testing.md) | Test tiers, coverage, CI |

## License

MIT
