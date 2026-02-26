# Flowstate Server

Configuration, Docker deployment, and authentication for `flowstate-server`.

## Configuration

### Core

| Env Var | Default | Description |
|---------|---------|-------------|
| `FLOWSTATE_BIND` | `0.0.0.0` | Bind address |
| `FLOWSTATE_PORT` | `3710` | Listen port |
| `FLOWSTATE_API_KEY` | *(none)* | API key for authentication. If set, all requests must include this key. |
| `RUST_LOG` | `info` | Log level filter (e.g. `debug`, `flowstate_server=debug`) |

### Database

| Env Var | Default | Description |
|---------|---------|-------------|
| `FLOWSTATE_DB_BACKEND` | `sqlite` | `sqlite` or `postgres` |
| `FLOWSTATE_SQLITE_PATH` | `~/.local/share/flowstate/flowstate.db` | SQLite database file path |
| `FLOWSTATE_DATABASE_URL` | *(none)* | Postgres connection URL (required when backend is `postgres`) |
| `DATABASE_URL` | *(none)* | Fallback Postgres URL if `FLOWSTATE_DATABASE_URL` is not set |

### S3 Object Storage

Optional. When configured, artifacts (specs, plans, research) are stored in S3 instead of the local filesystem. Each `FLOWSTATE_S3_*` variable falls back to its AWS equivalent.

| Env Var | AWS Fallback | Description |
|---------|-------------|-------------|
| `FLOWSTATE_S3_ENDPOINT` | `AWS_ENDPOINT_URL` | S3-compatible endpoint URL |
| `FLOWSTATE_S3_REGION` | `AWS_REGION` | Region (e.g. `us-east-1`, `garage`) |
| `FLOWSTATE_S3_BUCKET` | `GARAGE_BUCKET` | Bucket name |
| `FLOWSTATE_S3_ACCESS_KEY_ID` | `AWS_ACCESS_KEY_ID` | Access key ID |
| `FLOWSTATE_S3_SECRET_ACCESS_KEY` | `AWS_SECRET_ACCESS_KEY` | Secret access key |

## Authentication

### Environment Variable Key

The simplest approach: set `FLOWSTATE_API_KEY` on the server. All clients must send this key in the `Authorization: Bearer <key>` header.

### DB-Backed Key Management

For multi-key setups with named, revocable keys:

```bash
# Generate a new key
flowstate-server keygen --name "runner-prod"

# List all keys
flowstate-server list-keys

# Revoke a key
flowstate-server revoke-key --name "runner-prod"
```

Keys are stored encrypted in the database. The encryption key is at `~/.config/flowstate/server.key` (or `$XDG_CONFIG_HOME/flowstate/server.key`).

## RunPod Pod Manager

The server can automatically manage RunPod GPU pods for heavy workloads. The pod manager is enabled when `FLOWSTATE_RUNPOD_API_KEY` is set.

For the full GPU runner setup guide, see [runner-gpu.md](runner-gpu.md).

### Pod Manager Configuration

| Env Var | Default | Description |
|---------|---------|-------------|
| `FLOWSTATE_RUNPOD_API_KEY` | *(required)* | RunPod API key (enables pod manager) |
| `FLOWSTATE_RUNPOD_POD_ID` | *(none)* | Existing pod ID (skips creation) |
| `FLOWSTATE_RUNPOD_TEMPLATE_IMAGE` | `ghcr.io/ilovenuclearpower/flowstate-runner-gpu-tailscale:latest` | Docker image for new pods |
| `FLOWSTATE_RUNPOD_GPU_TYPE` | `NVIDIA RTX A5000` | GPU type to request |
| `FLOWSTATE_RUNPOD_GPU_COUNT` | `1` | Number of GPUs per pod |
| `FLOWSTATE_RUNPOD_NETWORK_VOLUME` | *(none)* | RunPod network volume ID |
| `FLOWSTATE_RUNPOD_CLOUD_TYPE` | `COMMUNITY` | `COMMUNITY` or `SECURE` |
| `FLOWSTATE_RUNPOD_IDLE_TIMEOUT` | `300` | Seconds idle before draining |
| `FLOWSTATE_RUNPOD_QUEUE_THRESHOLD` | `1` | Queue depth to trigger spin-up |
| `FLOWSTATE_RUNPOD_SPINDOWN_THRESHOLD` | `0` | Queue depth below which to drain |
| `FLOWSTATE_RUNPOD_SCAN_INTERVAL` | `30` | Seconds between decision ticks |
| `FLOWSTATE_RUNPOD_MAX_DAILY_SPEND` | `5000` | Daily cost cap in cents ($50.00) |
| `FLOWSTATE_RUNPOD_DRAIN_TIMEOUT` | `600` | Seconds to wait for drain before force stop |
| `FLOWSTATE_RUNPOD_TS_AUTHKEY` | *(none)* | Tailscale auth key (injected into pod as `TS_AUTHKEY`) |

### Pod Environment Injection

When the pod manager creates a pod, these server-side variables are mapped into the pod's environment:

| Server Env Var | Pod Env Var | Description |
|----------------|-------------|-------------|
| `FLOWSTATE_RUNPOD_TS_AUTHKEY` | `TS_AUTHKEY` | Tailscale auth key |
| `FLOWSTATE_RUNPOD_POD_SERVER_IP` | `TS_SERVER_IP` | Server's tailnet IP address |
| `FLOWSTATE_RUNPOD_POD_SERVER_URL` | `FLOWSTATE_SERVER_URL` | Server URL (use tailnet IP) |
| `FLOWSTATE_RUNPOD_POD_API_KEY` | `FLOWSTATE_API_KEY` | API key for runner auth |
| `FLOWSTATE_RUNPOD_POD_CAPABILITY` | `FLOWSTATE_RUNNER_CAPABILITY` | Capability tier (e.g. `heavy`) |
| `FLOWSTATE_RUNPOD_POD_BACKEND` | `FLOWSTATE_AGENT_BACKEND` | Agent backend name |
| `FLOWSTATE_RUNPOD_POD_MAX_CONCURRENT` | `FLOWSTATE_MAX_CONCURRENT` | Max concurrent runs |
| `FLOWSTATE_RUNPOD_POD_MAX_BUILDS` | `FLOWSTATE_MAX_BUILDS` | Max concurrent builds |
| `FLOWSTATE_RUNPOD_POD_VLLM_MODEL` | `VLLM_MODEL` | Model for vLLM to serve |
| `FLOWSTATE_RUNPOD_POD_VLLM_MAX_MODEL_LEN` | `VLLM_MAX_MODEL_LEN` | Max context length for vLLM (default: `80000`) |
| `FLOWSTATE_RUNPOD_POD_HF_TOKEN` | `HF_TOKEN` | HuggingFace token (required for gated models) |

For an existing pod (`FLOWSTATE_RUNPOD_POD_ID`), set the pod-side names directly in the RunPod template or pod config.

## Docker

Image: `ghcr.io/ilovenuclearpower/flowstate-server`
Tags: `latest`, `sha-<commit>`

### Basic

```bash
docker run -p 3710:3710 ghcr.io/ilovenuclearpower/flowstate-server
```

### With Persistent Storage

```bash
docker run -p 3710:3710 \
  -v flowstate-data:/data \
  -e FLOWSTATE_DATA_DIR=/data \
  ghcr.io/ilovenuclearpower/flowstate-server
```

### With Postgres

```bash
docker run -p 3710:3710 \
  -e FLOWSTATE_DB_BACKEND=postgres \
  -e FLOWSTATE_DATABASE_URL=postgres://user:pass@db:5432/flowstate \
  ghcr.io/ilovenuclearpower/flowstate-server
```

### With S3 Storage

```bash
docker run -p 3710:3710 \
  -e FLOWSTATE_S3_ENDPOINT=https://s3.example.com \
  -e FLOWSTATE_S3_REGION=us-east-1 \
  -e FLOWSTATE_S3_BUCKET=flowstate-artifacts \
  -e FLOWSTATE_S3_ACCESS_KEY_ID=AKIA... \
  -e FLOWSTATE_S3_SECRET_ACCESS_KEY=secret \
  ghcr.io/ilovenuclearpower/flowstate-server
```

### With Auth + RunPod

```bash
docker run -p 3710:3710 \
  -v flowstate-data:/data \
  -e FLOWSTATE_DATA_DIR=/data \
  -e FLOWSTATE_API_KEY=your-secret-key \
  -e FLOWSTATE_RUNPOD_API_KEY=your-runpod-key \
  -e FLOWSTATE_RUNPOD_TS_AUTHKEY=tskey-auth-... \
  -e FLOWSTATE_RUNPOD_POD_SERVER_IP=100.64.x.x \
  -e FLOWSTATE_RUNPOD_POD_SERVER_URL=http://100.64.x.x:3710 \
  -e FLOWSTATE_RUNPOD_POD_API_KEY=your-secret-key \
  ghcr.io/ilovenuclearpower/flowstate-server
```
