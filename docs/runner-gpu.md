# GPU Runner (RunPod + Tailscale + vLLM)

Flowstate can automatically manage RunPod GPU pods for running builds with self-hosted models. The system uses a pull-based architecture: the runner inside the pod polls the server via Tailscale userspace networking (outbound-only, no inbound ports).

## Architecture

```
┌─────────────────────────────────────────────┐
│  RunPod GPU Pod                              │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐ │
│  │  vLLM    │  │ Runner   │  │ Tailscale │ │
│  │ :8000    │←─│          │──│ SOCKS5    │─┼──→ Tailnet → Flowstate Server
│  └──────────┘  └──────────┘  │ :1055     │ │
│                              └───────────┘ │
└─────────────────────────────────────────────┘
```

- **vLLM** serves the model locally (e.g. MiniMax-M2.5)
- **Runner** polls the server for work, executes builds using the local vLLM endpoint
- **Tailscale** provides outbound-only networking via a SOCKS5 proxy

## Prerequisites

- A [RunPod](https://www.runpod.io/) account with API access
- A [Tailscale](https://tailscale.com/) account with an auth key
- The Flowstate server accessible on your tailnet

## How Connectivity Works

The GPU pod reaches your server through Tailscale. Here's how the pieces fit together:

1. **Your server must already be on a Tailscale network** (it has a tailnet IP like `100.64.x.x`).
2. **Set `FLOWSTATE_RUNPOD_TS_AUTHKEY`** on the server — this Tailscale auth key lets the pod join your tailnet.
3. **Set `FLOWSTATE_RUNPOD_POD_SERVER_IP`** on the server — your server's tailnet IP address.
4. When the pod manager creates a pod, it **injects these values** as `TS_AUTHKEY` and `TS_SERVER_IP` in the pod environment.
5. The pod entrypoint **starts Tailscale in userspace mode**, joins your tailnet, and exposes a SOCKS5 proxy on `localhost:1055`.
6. The runner inside the pod sets `ALL_PROXY=socks5://localhost:1055` and reaches the server at `http://{TS_SERVER_IP}:3710`.

No inbound ports are opened on the pod. All traffic is outbound through Tailscale.

## Server-Side Configuration

Set these on the Flowstate server. See [server.md](server.md#runpod-pod-manager) for the full pod manager configuration reference.

Key variables:

| Env Var | Description |
|---------|-------------|
| `FLOWSTATE_RUNPOD_API_KEY` | RunPod API key (enables the pod manager) |
| `FLOWSTATE_RUNPOD_TS_AUTHKEY` | Tailscale auth key for the pod |
| `FLOWSTATE_RUNPOD_POD_SERVER_IP` | Your server's tailnet IP |
| `FLOWSTATE_RUNPOD_POD_SERVER_URL` | Server URL using the tailnet IP (e.g. `http://100.64.x.x:3710`) |
| `FLOWSTATE_RUNPOD_POD_API_KEY` | API key the runner uses to authenticate |

## Pod-Side Environment Variables

These are set inside the pod (injected automatically by the pod manager, or set manually for existing pods):

| Env Var | Description |
|---------|-------------|
| `TS_AUTHKEY` | Tailscale auth key |
| `TS_SERVER_IP` | Server's tailnet IP address |
| `FLOWSTATE_SERVER_URL` | Server URL for the runner to connect to |
| `FLOWSTATE_API_KEY` | API key for runner auth |
| `FLOWSTATE_RUNNER_CAPABILITY` | Capability tier (e.g. `heavy`) |
| `FLOWSTATE_AGENT_BACKEND` | Agent backend name |
| `FLOWSTATE_MAX_CONCURRENT` | Max concurrent runs |
| `FLOWSTATE_MAX_BUILDS` | Max concurrent builds |
| `VLLM_MODEL` | Model for vLLM to serve |
| `VLLM_PORT` | vLLM listen port (default: `8000`) |
| `VLLM_MAX_MODEL_LEN` | Max context length for vLLM (default: `80000`) |
| `HF_TOKEN` | HuggingFace token (required for gated models like Llama, Mistral) |

### HuggingFace Model Cache

If a RunPod network volume is attached (mounted at `/runpod-volume`), the entrypoint sets `HF_HOME=/runpod-volume/huggingface`. This persists downloaded models across pod restarts so they don't re-download on each spin-up.

Configure the network volume via `FLOWSTATE_RUNPOD_NETWORK_VOLUME` on the server (see [server.md](server.md#runpod-pod-manager)).

## Docker Image

Image: `ghcr.io/ilovenuclearpower/flowstate-runner-gpu-tailscale`
Tags: `latest`, `sha-<commit>`

The image uses `vllm/vllm-openai` as its runtime base (includes CUDA, Python, PyTorch, and vLLM), with Tailscale and the Flowstate runner layered on top. The nix builder stage compiles the runner binary. No system Rust installation is needed.

### Building Locally

```bash
docker build -t flowstate-runner-gpu-tailscale -f docker/runpod/Dockerfile .
```

### Manual Push

```bash
docker tag flowstate-runner-gpu-tailscale ghcr.io/ilovenuclearpower/flowstate-runner-gpu-tailscale:latest
docker push ghcr.io/ilovenuclearpower/flowstate-runner-gpu-tailscale:latest
```

### Building Without Docker

```bash
nix build .#runner
# Binary at result/bin/flowstate-runner
```

### Entrypoint Flow

The pod entrypoint (`docker/runpod/entrypoint.sh`):

1. Starts Tailscale in userspace mode (`--tun=userspace-networking`)
2. Joins the tailnet using `TS_AUTHKEY`
3. Exports `ALL_PROXY=socks5://localhost:1055`
4. Optionally starts vLLM if `VLLM_MODEL` is set
5. Starts the Flowstate runner

## Pod Manager Decision Logic

The pod manager runs as a background task in the server process:

1. **Spin Up**: Queue depth >= threshold AND pod stopped AND not cost-capped
2. **Stay Warm**: Pod running AND queue > 0 — reset idle timer
3. **Drain**: Pod running AND queue <= spindown threshold AND idle > timeout
4. **Stop**: Runner reports drained — stop pod
5. **Force Stop**: Drain timeout exceeded — force stop pod
6. **Cost Cap**: Daily spend > max — drain + stop, no more spin-ups today

## Infra API Endpoints

Monitor and control GPU pods:

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/infra/gpu-status` | Pod status, cost, queue depth |
| `POST` | `/api/infra/gpu/start` | Manual spin-up |
| `POST` | `/api/infra/gpu/stop` | Graceful drain and stop |
| `GET` | `/api/infra/runners` | List runners with utilization metrics |
| `PUT` | `/api/infra/runners/{id}/config` | Set pending config (poll interval, drain) |

## Cost Management

The pod manager tracks cost per decision tick and enforces a daily cap:

- **`FLOWSTATE_RUNPOD_MAX_DAILY_SPEND`**: Daily cap in cents (default: `5000` = $50.00)
- When the cap is reached, the pod is drained and stopped. No new spin-ups until the next day.

## Drain Flow

1. Pod manager sets `pending_config` with `drain: true` in the runner's registration
2. Runner picks up the pending config on its next poll
3. Runner stops claiming new work and finishes in-progress runs
4. Runner reports `drained` status to the server
5. Pod manager stops the pod

If the runner doesn't report drained within `FLOWSTATE_RUNPOD_DRAIN_TIMEOUT` (default: 600s), the pod is force-stopped.

## Tailscale Userspace Networking

RunPod pods run without `--privileged` and don't have `/dev/net/tun`. Tailscale operates in userspace mode:

```bash
tailscaled --tun=userspace-networking --socks5-server=localhost:1055 --state=/tmp/tailscale &
tailscale up --authkey="$TS_AUTHKEY" --hostname="flowstate-gpu-$(hostname)"
```

All outbound runner traffic goes through the SOCKS5 proxy (`ALL_PROXY=socks5://localhost:1055`), reaching the server via its tailnet IP.

## Troubleshooting

**Pod won't start**: Check `FLOWSTATE_RUNPOD_API_KEY` is valid. Check the daily cost cap hasn't been reached.

**Runner can't reach server**: Verify Tailscale is connected (`tailscale status`). Check `TS_SERVER_IP` matches the server's tailnet IP. Ensure `ALL_PROXY` is set.

**vLLM won't load model**: Check GPU memory is sufficient. Try reducing `--max-model-len`. Check model name is correct.

**Drain hangs**: The drain timeout (default 600s) will force-stop the pod. Check runner logs for stuck runs.
