# flowstate

A task management system with Claude AI integration for designing, planning, and building software features. Flowstate gives you a terminal Kanban board backed by an HTTP API, with an approval-gated workflow that orchestrates Claude Code to produce specifications, implementation plans, and working code.

## Architecture

```
flowstate-tui (terminal UI)
    |
    | HTTP (reqwest)
    v
flowstate-server (axum REST API + Claude runner)
    |
    | LocalService
    v
flowstate-db (SQLite, WAL mode)
    |
    v
flowstate-core (domain models, zero deps)
```

Seven crates, layered by responsibility:

| Crate | Purpose |
|-------|---------|
| `flowstate-core` | Domain types: Task, Project, Sprint, ClaudeRun, ApprovalStatus |
| `flowstate-db` | SQLite persistence with versioned migrations, file path helpers |
| `flowstate-service` | `TaskService` trait with `LocalService` (direct DB) and `HttpService` (HTTP client) implementations |
| `flowstate-server` | Axum REST API, Bearer token auth, Claude Code runner |
| `flowstate-tui` | Ratatui/Crossterm terminal UI with Kanban board |
| `flowstate-verify` | Async verification step runner with timeout and fail-fast |
| `flowstate-mcp` | Model Context Protocol server (placeholder) |

## Features

### Kanban Board
Terminal-based board with columns for Backlog, Todo, In Progress, In Review, and Done. Navigate with hjkl/arrow keys, move tasks between columns, set priorities, manage multiple projects.

### Design / Plan / Build Workflow
Each task supports a three-phase AI workflow:

1. **Design** — Claude produces a technical specification (`SPECIFICATION.md`)
2. **Plan** — Claude produces a structured implementation plan (`PLAN.md`), gated on spec approval
3. **Build** — Claude implements the plan against a git checkout of the project repo

Specs and plans go through an approval cycle (Pending -> Approved/Rejected). Editing an approved spec automatically revokes approval and requires re-review.

Plan generation produces structured output with four sections:
- Directories and files to create/modify
- Ordered work phases with dependencies and deliverables
- Agent/model assignments with parallelism notes
- Validation steps (automated checks + human review checkpoints)

### Authentication
API key system with SHA256 hashing and constant-time comparison. Keys are generated via `flowstate-server keygen`, stored hashed in the DB, and validated as Bearer tokens on every request.

### Editor Integration
Press `S` or `I` in task detail to open specs/plans in `$EDITOR`. Changes are synced back to the server on save. The server handles status transitions automatically — no client-side bookkeeping needed.

### Remote Access
The TUI can connect to a remote server:
```
flowstate --server http://example.com:3710 --api-key fs_xxxxx
```
Or set `FLOWSTATE_API_KEY` in your environment.

## Quickstart

### Prerequisites
- [Nix](https://nixos.org/) with flakes enabled

### Run
```bash
# Enter dev shell (provides Rust toolchain, sqlite, git)
nix develop

# Build everything
cargo build --workspace

# Run the TUI (auto-spawns a local server on port 3710)
cargo run -p flowstate-tui

# Or run the server standalone
cargo run -p flowstate-server

# Generate an API key
cargo run -p flowstate-server -- keygen
```

### Nix build
```bash
nix build .#tui      # TUI binary
nix build .#server   # Server binary
nix build .#mcp      # MCP server binary
```

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `FLOWSTATE_PORT` | `3710` | Server listen port |
| `FLOWSTATE_BIND` | `0.0.0.0` | Server bind address |
| `FLOWSTATE_API_KEY` | (none) | API key for auth (enables auth when set) |

Data is stored at `~/.local/share/flowstate/` (or `$XDG_DATA_HOME/flowstate/`):
- `flowstate.db` — SQLite database
- `tasks/{id}/spec.md` — Task specifications
- `tasks/{id}/plan.md` — Implementation plans
- `runs/{id}/` — Claude run output and prompts

## Keyboard Shortcuts

### Board (Normal mode)
| Key | Action |
|-----|--------|
| `h/l` | Switch columns |
| `j/k` | Navigate tasks |
| `n` | New task |
| `Enter` | Task detail |
| `m/M` | Move task forward/back |
| `d` | Delete task |
| `p` | Set priority |
| `P` | Project switcher |
| `q` | Quit |

### Task Detail
| Key | Action |
|-----|--------|
| `t` | Edit title |
| `e` | Edit description |
| `p` | Set priority |
| `m` | Move to next status |
| `d` | Delete |
| `c` | Claude actions (design/plan/build) |
| `s/S` | View/edit spec |
| `i/I` | View/edit plan |
| `a` | Approve/reject pending spec or plan |
| `Esc` | Back |

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/health` | Health check (no auth) |
| `GET/POST` | `/api/tasks` | List/create tasks |
| `GET/PUT/DELETE` | `/api/tasks/{id}` | Get/update/delete task |
| `GET` | `/api/tasks/{id}/children` | List subtasks |
| `GET/PUT` | `/api/tasks/{id}/spec` | Read/write specification |
| `GET/PUT` | `/api/tasks/{id}/plan` | Read/write plan |
| `GET/POST` | `/api/tasks/{task_id}/claude-runs` | List/trigger Claude runs |
| `GET` | `/api/claude-runs/{id}` | Get run status |
| `GET` | `/api/claude-runs/{id}/output` | Get run output |
| `GET/POST` | `/api/projects` | List/create projects |
| `GET/PUT/DELETE` | `/api/projects/{id}` | Get/update/delete project |

## Roadmap

### Local Model Support
Run design/plan/build phases against local models via [Ollama](https://ollama.ai/) or [LM Studio](https://lmstudio.ai/) instead of Claude Code. This would allow fully offline operation and experimentation with open-weight models for different phases of the workflow.

### Self-Hosting
- Pluggable object store backend (S3-compatible) for spec/plan/attachment storage instead of local filesystem
- External database support (Postgres) for multi-instance deployments
- Cleaner separation between server and TUI client — the TUI currently shells out to the server binary; these should be fully independent deployment units

### Auth Improvements
- User accounts created on first login (currently API keys only, no user identity)
- Role-based permissions (admin, editor, viewer)
- OAuth/SSO integration for team environments
- Per-project access control

### Task Runner Separation
Decouple the Claude/AI task runner from the board server into a standalone worker process. The board server should only manage state and serve the API. Task runners should:
- Pull jobs from a queue
- Run in isolated environments with their own git credentials and tool access
- Report results back to the board server via API
- Scale horizontally — run multiple workers for parallel builds
- Validate their own prerequisites (gh CLI, git auth, language toolchains) on startup

## License

MIT
