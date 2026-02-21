# Flowstate TUI

The Flowstate TUI is a terminal-based interface for managing tasks through the Flowstate workflow. It connects to a Flowstate server and provides vim-style keyboard navigation.

## Starting the TUI

```bash
# Auto-spawn a local server and connect
flowstate

# Connect to an existing server
flowstate --server http://your-server:3710

# Authenticate with an API key
flowstate --server http://your-server:3710 --api-key YOUR_KEY
```

If `--server` is not provided, the TUI automatically spawns a local `flowstate-server` process on port 3710 and connects to it. The server is terminated when the TUI exits.

The `FLOWSTATE_API_KEY` environment variable can be used instead of the `--api-key` flag.

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
