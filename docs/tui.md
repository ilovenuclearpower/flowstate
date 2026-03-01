# Flowstate TUI

The Flowstate TUI is a terminal-based interface for managing tasks through the Flowstate workflow. It connects to a Flowstate server and provides vim-style keyboard navigation.

## Installation

### Nix (any platform)

The flake exports the TUI as its default package:

```bash
# Run directly without installing
nix run github:ilovenuclearpower/flowstate

# Install into your profile
nix profile install github:ilovenuclearpower/flowstate

# From a local clone
nix build .#tui
./result/bin/flowstate
```

**NixOS (`configuration.nix` with flakes):**

Add the flake input and include the package in your system configuration:

```nix
# flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flowstate.url = "github:ilovenuclearpower/flowstate";
  };

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

**home-manager (standalone or as NixOS module):**

```nix
# flake.nix inputs
inputs.flowstate.url = "github:ilovenuclearpower/flowstate";

# In your home-manager config
home.packages = [
  inputs.flowstate.packages.${pkgs.system}.default
];
```

### macOS (pre-built binary)

Download from [GitHub Releases](https://github.com/ilovenuclearpower/flowstate/releases):

```bash
# Apple Silicon (M1/M2/M3/M4)
curl -L -o flowstate \
  https://github.com/ilovenuclearpower/flowstate/releases/latest/download/flowstate-macos-aarch64

# Intel Mac
curl -L -o flowstate \
  https://github.com/ilovenuclearpower/flowstate/releases/latest/download/flowstate-macos-x86_64

chmod +x flowstate
sudo mv flowstate /usr/local/bin/
```

### Ubuntu / Linux (pre-built binary)

```bash
curl -L -o flowstate \
  https://github.com/ilovenuclearpower/flowstate/releases/latest/download/flowstate-linux-x86_64
chmod +x flowstate
sudo mv flowstate /usr/local/bin/
```

### Build from Source

Requires Rust 1.75+ and system dependencies:

```bash
# Ubuntu/Debian
sudo apt install pkg-config libssl-dev libsqlite3-dev

# macOS (with Homebrew)
brew install pkg-config openssl sqlite

git clone https://github.com/ilovenuclearpower/flowstate.git
cd flowstate
cargo build --release -p flowstate-tui
# Binary at target/release/flowstate
```

## Configuration

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--server` | *(none)* | `http://127.0.0.1:3710` | URL of the Flowstate server |
| `--api-key` | `FLOWSTATE_API_KEY` | *(none)* | API key for authenticating with the server |

### Credential Persistence

The TUI saves your API key and server URL to `~/.config/flowstate/tui-credentials` (respects `$XDG_CONFIG_HOME`). After initial setup, no flags are needed on subsequent launches.

**Credential resolution order** (first match wins):

1. `--api-key` CLI flag
2. `FLOWSTATE_API_KEY` environment variable
3. Saved credentials file
4. No key (unauthenticated)

The server URL is also saved — if you first connect with `--server http://my-server:3710`, future launches remember that URL.

### Auto-Spawn Behavior

When no `--server` flag is provided and no saved server URL exists, the TUI:
1. Looks for a `flowstate-server` binary next to its own executable, then falls back to `PATH`.
2. Spawns the server on `127.0.0.1:3710`.
3. Waits up to 10 seconds for the server to become ready.
4. Terminates the server on exit.

### Remote Connection

```bash
flowstate --server http://your-server:3710 --api-key YOUR_KEY
```

### First-Time Setup Wizard

When connecting to a server that has no API keys configured, the TUI runs a setup wizard:

1. Generates your admin API key.
2. Saves it to the credentials file.
3. On subsequent launches, the key is loaded automatically.

See [Self-Hosting](self-hosting.md) for the full setup walkthrough.

## Tabs

The TUI has two tabs, shown in the title bar:

| Key | Tab | Description |
|-----|-----|-------------|
| `1` | **Board** | Kanban task board (default) |
| `2` | **Ops** | Operations dashboard |

Press `Tab` to toggle between them.

## Board Tab

### Workflow Columns

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

Subtasks use a simplified flow: Todo -> Build -> Verify -> Done.

## Ops Tab

The Ops tab provides server administration and monitoring. It has four sections:

### Overview

Server connectivity status, runner count, and queued runs at a glance.

### API Keys

Manage API keys for the server. This is the primary way to create keys for runners and other clients.

| Key | Action |
|-----|--------|
| `g` | Generate a new API key (enter name, press Enter) |
| `d` | Revoke selected key |
| `j` / `k` | Navigate key list |
| `r` | Refresh |

### Runners

View connected runners and their status: capability tier, active/max runs, saturation percentage, and last heartbeat.

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate runner list |
| `r` | Refresh |

### GPU (RunPod)

Monitor and control GPU pods when RunPod is configured.

| Key | Action |
|-----|--------|
| `s` | Start GPU pod |
| `S` | Stop GPU pod (drain) |
| `r` | Refresh |

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

### Normal Mode (Board Tab)

| Key | Action |
|-----|--------|
| `h` / `<-` | Move to left column |
| `l` / `->` | Move to right column |
| `j` / Down | Move selection down in column |
| `k` / Up | Move selection up in column |
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
| `1` | Switch to Board tab |
| `2` | Switch to Ops tab |
| `Tab` | Toggle tabs |
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
| `j` / Down | Scroll down |
| `k` / Up | Scroll up |
| `Esc` / `q` | Back |

### Claude Running Mode

| Key | Action |
|-----|--------|
| `Esc` | Return to task detail (run continues in background) |

### Project List Mode

| Key | Action |
|-----|--------|
| `j` / Down | Move selection down |
| `k` / Up | Move selection up |
| `Enter` | Switch to selected project |
| `n` | Create new project |
| `d` | Delete selected project |
| `u` | Edit repo URL |
| `t` | Edit repo token |
| `Esc` | Cancel |

### Sprint List Mode

| Key | Action |
|-----|--------|
| `j` / Down | Move selection down |
| `k` / Up | Move selection up |
| `Enter` | Filter board by selected sprint |
| `n` | Create new sprint |
| `Esc` | Cancel |
