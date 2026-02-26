# Flowstate TUI

The Flowstate TUI is a terminal-based interface for managing tasks through the Flowstate workflow. It connects to a Flowstate server and provides vim-style keyboard navigation.

## Configuration

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--server` | *(none)* | `http://127.0.0.1:3710` | URL of the Flowstate server |
| `--api-key` | `FLOWSTATE_API_KEY` | *(none)* | API key for authenticating with the server |

### Auto-Spawn Behavior

When no `--server` flag is provided, the TUI:
1. Looks for a `flowstate-server` binary next to its own executable, then falls back to `PATH`.
2. Spawns the server on `127.0.0.1:3710`.
3. Waits up to 10 seconds for the server to become ready.
4. Terminates the server on exit.

### Remote Connection

```bash
flowstate --server http://your-server:3710 --api-key YOUR_KEY
```

## Workflow Columns

The board displays tasks across 7 workflow columns:

| Column | Description |
|--------|-------------|
| Todo | New tasks, not yet started |
| Research | Gathering information and context |
| Design | Defining the specification |
| Plan | Creating an implementation plan |
| Build | Active development |
| Verify | Testing and verification |
| Done | Completed tasks |

Tasks move forward through columns with `m` and backward with `M`.

Subtasks use a simplified flow: Todo → Build → Verify → Done.

## Modes

The TUI operates in several modes:

- **Normal** — Board navigation and task actions.
- **TaskDetail** — Viewing and acting on a single task.
- **NewTask** — Typing a new task title.
- **EditTitle** / **EditDescription** — Editing task fields inline.
- **ConfirmDelete** — Confirming task deletion.
- **PriorityPick** — Selecting a priority level.
- **ProjectList** / **NewProject** — Switching or creating projects.
- **SprintList** / **NewSprint** — Managing sprints.
- **ClaudeActionPick** / **ClaudeRunning** / **ClaudeOutput** — Triggering and monitoring Claude runs.
- **ApprovalPick** / **FeedbackInput** — Approving or rejecting artifacts.
- **ViewSpec** / **ViewPlan** / **ViewResearch** / **ViewVerification** — Read-only scrollable viewers.
- **Health** — System health checks.

## Keymap Reference

### Normal Mode

| Key | Action |
|-----|--------|
| `h` / `←` | Move to left column |
| `l` / `→` | Move to right column |
| `j` / `↓` | Move selection down in column |
| `k` / `↑` | Move selection up in column |
| `g` | Jump to first task in column |
| `G` | Jump to last task in column |
| `Enter` | Open task detail |
| `n` | Create new task in active column |
| `m` | Move task forward (next status) |
| `M` | Move task backward (previous status) |
| `d` | Delete task (with confirmation) |
| `p` | Change task priority |
| `P` | Open project switcher |
| `x` | Open sprint list |
| `X` | Clear sprint filter |
| `H` | System health checks |
| `q` | Quit |
| `Ctrl+C` | Force quit |

### Task Detail Mode

| Key | Action |
|-----|--------|
| `Esc` / `q` | Back to board |
| `t` | Edit title |
| `e` | Edit description |
| `n` | Create subtask |
| `p` | Change priority |
| `m` | Move task forward |
| `d` | Delete task |
| `c` | Claude action picker |
| `s` | View spec |
| `S` | Edit spec in `$EDITOR` |
| `i` | View plan |
| `I` | Edit plan in `$EDITOR` |
| `w` | View research |
| `W` | Edit research in `$EDITOR` |
| `v` | View verification |
| `V` | Edit verification in `$EDITOR` |
| `a` | Approve/reject pending artifact |

### Text Input Modes (NewTask, EditTitle, NewSprint, etc.)

| Key | Action |
|-----|--------|
| `Enter` | Submit |
| `Esc` | Cancel |
| `Backspace` | Delete character |
| Any character | Append to input |

### Edit Description Mode

| Key | Action |
|-----|--------|
| `Ctrl+S` | Save description |
| `Esc` | Cancel |
| `Enter` | New line |
| `Backspace` | Delete character |
| Any character | Append to input |

### Confirm Delete Mode

| Key | Action |
|-----|--------|
| `y` | Confirm deletion |
| `n` / `Esc` | Cancel |

### Priority Pick Mode

| Key | Action |
|-----|--------|
| `1` | Urgent |
| `2` | High |
| `3` | Medium |
| `4` | Low |
| `5` | None |
| `Esc` | Cancel |

### Scrollable View Modes (Spec, Plan, Research, Verification)

| Key | Action |
|-----|--------|
| `j` / `↓` | Scroll down |
| `k` / `↑` | Scroll up |
| `Esc` / `q` | Back |

### Claude Running Mode

| Key | Action |
|-----|--------|
| `Esc` | Return to task detail (run continues in background) |

### Project List Mode

| Key | Action |
|-----|--------|
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `Enter` | Switch to selected project |
| `n` | Create new project |
| `d` | Delete selected project |
| `u` | Edit repo URL |
| `t` | Edit repo token |
| `Esc` | Cancel |

### Sprint List Mode

| Key | Action |
|-----|--------|
| `j` / `↓` | Move selection down |
| `k` / `↑` | Move selection up |
| `Enter` | Filter board by selected sprint |
| `n` | Create new sprint |
| `Esc` | Cancel |
