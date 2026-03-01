# Self-Hosting Flowstate

This guide walks through deploying Flowstate with Docker containers and connecting to it with the TUI client.

## Prerequisites

- Docker and Docker Compose (v2)
- A machine to run the server (local, VPS, etc.)
- An Anthropic API key (for the runner to call Claude)

## 1. Start the Server

Clone the repo or copy the `docker-compose.yml` and `.env.example` files:

```bash
curl -O https://raw.githubusercontent.com/ilovenuclearpower/flowstate/main/docker-compose.yml
curl -O https://raw.githubusercontent.com/ilovenuclearpower/flowstate/main/.env.example
cp .env.example .env
```

Start just the server first:

```bash
docker compose up -d server
```

The server starts with no authentication and an empty Postgres database. Data is stored in Docker volumes (`postgres-data` and `server-config`) and persists across restarts.

## 2. Run the Setup Wizard

Install the TUI client (see [Installing the TUI](#installing-the-tui) below), then connect to your server:

```bash
flowstate --server http://your-server:3710
```

On first launch the setup wizard detects that no API keys exist and walks you through generating your admin key:

1. Press **Enter** to generate the admin key.
2. The key is displayed and automatically saved to `~/.config/flowstate/tui-credentials`.
3. Press **Enter** to continue into the main TUI.

On subsequent launches, the saved key and server URL are loaded automatically — no flags needed:

```bash
flowstate
```

The credentials file location respects `$XDG_CONFIG_HOME` (default: `~/.config/flowstate/tui-credentials`).

## 3. Generate a Runner Key

The runner needs its own API key. From the TUI:

1. Press **2** to switch to the **Ops** tab.
2. Navigate to the **API Keys** section.
3. Press **g**, type a name like `runner`, and press **Enter**.
4. Copy the generated key (shown once).

Alternatively, use the API directly:

```bash
curl -s -X POST http://your-server:3710/api/admin/api-keys \
  -H "Authorization: Bearer YOUR_ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "runner"}' | jq .api_key
```

## 4. Start the Runner

Edit your `.env` file with the runner key and your Anthropic API key:

```bash
# .env
FLOWSTATE_API_KEY=fs_...      # the runner key from step 3
ANTHROPIC_API_KEY=sk-ant-...  # your Anthropic API key
```

Then start the runner:

```bash
docker compose up -d runner
```

The runner connects to the server, registers, and begins polling for work. You can verify it appeared in the Ops tab (press **2** in the TUI, navigate to **Runners**).

## 5. Create a Project and Start Working

Back in the TUI (press **1** for the Board tab):

1. Press **P** to open the project switcher.
2. Press **n** to create a new project (name and slug).
3. After creating the project, press **r** to set the repo URL (e.g., `https://github.com/you/your-repo`).
4. Press **T** to set a repo access token (GitHub PAT with repo scope).
5. Press **Esc** to return to the board.
6. Press **n** to create a task.
7. Press **Enter** on a task, then **c** to trigger a Claude action.

---

## Installing the TUI

### Nix (any platform)

The flake exports the TUI as its default package. No extra flake needed.

**Run without installing:**

```bash
nix run github:ilovenuclearpower/flowstate -- --server http://your-server:3710
```

**Install into your profile:**

```bash
nix profile install github:ilovenuclearpower/flowstate
```

**NixOS (`configuration.nix` with flakes):**

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

**From a local clone:**

```bash
nix build .#tui
./result/bin/flowstate --server http://your-server:3710
```

### macOS (pre-built binary)

Download the latest release binary for your architecture:

```bash
# Apple Silicon (M1/M2/M3/M4)
curl -L -o flowstate \
  https://github.com/ilovenuclearpower/flowstate/releases/latest/download/flowstate-macos-aarch64
chmod +x flowstate

# Intel Mac
curl -L -o flowstate \
  https://github.com/ilovenuclearpower/flowstate/releases/latest/download/flowstate-macos-x86_64
chmod +x flowstate
```

Move it somewhere on your `$PATH`:

```bash
sudo mv flowstate /usr/local/bin/
```

Then connect:

```bash
flowstate --server http://your-server:3710
```

### Ubuntu / Debian Linux (pre-built binary)

```bash
curl -L -o flowstate \
  https://github.com/ilovenuclearpower/flowstate/releases/latest/download/flowstate-linux-x86_64
chmod +x flowstate
sudo mv flowstate /usr/local/bin/
```

Then connect:

```bash
flowstate --server http://your-server:3710
```

### Build from Source

Requires Rust 1.75+ and system dependencies (pkg-config, openssl, sqlite):

```bash
# Ubuntu/Debian
sudo apt install pkg-config libssl-dev libsqlite3-dev

# macOS (with Homebrew)
brew install pkg-config openssl sqlite

# Build
git clone https://github.com/ilovenuclearpower/flowstate.git
cd flowstate
cargo build --release -p flowstate-tui
# Binary at target/release/flowstate
```

---

## Configuration Reference

### Server Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `FLOWSTATE_PORT` | `3710` | Port to listen on |
| `FLOWSTATE_BIND` | `0.0.0.0` | Bind address |
| `FLOWSTATE_DB_BACKEND` | `postgres` | Database backend (`sqlite` or `postgres`) |
| `FLOWSTATE_DATABASE_URL` | *(compose default)* | Postgres connection URL |
| `FLOWSTATE_SQLITE_PATH` | *(data dir)* | Custom SQLite file path (when using `sqlite` backend) |
| `POSTGRES_USER` | `flowstate` | Postgres user (used by compose) |
| `POSTGRES_PASSWORD` | `flowstate` | Postgres password (used by compose) |
| `POSTGRES_DB` | `flowstate` | Postgres database name (used by compose) |

### Runner Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `FLOWSTATE_SERVER_URL` | *(required)* | Server URL (e.g., `http://server:3710`) |
| `FLOWSTATE_API_KEY` | *(required)* | API key for authentication |
| `FLOWSTATE_RUNNER_CAPABILITY` | `standard` | Capability tier (`light`, `standard`, `heavy`) |
| `FLOWSTATE_AGENT_BACKEND` | `claude-cli` | Agent backend to use |
| `FLOWSTATE_MAX_CONCURRENT` | `2` | Max concurrent runs |
| `FLOWSTATE_MAX_BUILDS` | `1` | Max concurrent build actions |
| `ANTHROPIC_API_KEY` | *(required)* | Anthropic API key for Claude |

### TUI Credential Resolution

The TUI resolves its API key in this order (first match wins):

1. `--api-key` CLI flag
2. `FLOWSTATE_API_KEY` environment variable
3. Saved credentials file (`~/.config/flowstate/tui-credentials`)
4. No key (unauthenticated, triggers setup wizard if server has no keys)

### Credential File Format

```
server_url=http://your-server:3710
api_key=fs_...
```

Location: `$XDG_CONFIG_HOME/flowstate/tui-credentials` (default `~/.config/flowstate/tui-credentials`). File permissions are set to `0600`.

---

## Advanced: Customizing Postgres

The default compose uses Postgres with the user/password/database all set to `flowstate`. Override these via your `.env`:

```bash
# .env
POSTGRES_USER=myuser
POSTGRES_PASSWORD=a-strong-password
POSTGRES_DB=flowstate
```

The server's `FLOWSTATE_DATABASE_URL` is assembled from these variables automatically in the compose file.

To use an external Postgres instance (not the bundled one), remove the `db` service from compose and set the URL directly:

```bash
# .env
FLOWSTATE_DATABASE_URL=postgres://user:pass@your-postgres-host:5432/flowstate
```

Then override the server environment in compose:

```yaml
services:
  server:
    environment:
      FLOWSTATE_DB_BACKEND: postgres
      FLOWSTATE_DATABASE_URL: ${FLOWSTATE_DATABASE_URL}
```

## Advanced: SQLite Backend

For single-user or development deployments, you can use SQLite instead of Postgres:

```yaml
services:
  server:
    image: ghcr.io/ilovenuclearpower/flowstate-server:latest
    ports:
      - "${FLOWSTATE_PORT:-3710}:3710"
    environment:
      FLOWSTATE_DB_BACKEND: sqlite
      FLOWSTATE_PORT: "3710"
      FLOWSTATE_BIND: "0.0.0.0"
    volumes:
      - server-data:/root/.local/share/flowstate
      - server-config:/root/.config/flowstate
    restart: unless-stopped

volumes:
  server-data:
  server-config:
```

No separate database service is needed — the SQLite file is stored in the `server-data` volume.

## Advanced: S3-Compatible Object Store

By default, task artifacts (specs, plans, research) are stored on the local filesystem. For shared or cloud deployments, configure an S3-compatible store:

```bash
# .env
FLOWSTATE_S3_ENDPOINT=https://s3.us-east-1.amazonaws.com
FLOWSTATE_S3_BUCKET=flowstate-artifacts
FLOWSTATE_S3_ACCESS_KEY_ID=AKIA...
FLOWSTATE_S3_SECRET_ACCESS_KEY=...
FLOWSTATE_S3_REGION=us-east-1
```

This works with AWS S3, MinIO, Garage, or any S3-compatible service.
