# RunPod GPU Pod Management for Flowstate

## Context

GPU time costs money whether you use it or not. The goal is to run more AI cheaper by self-hosting MiniMax-M2.5 (INT4-AWQ quantized) on RunPod, with smart spin-up/down to only pay when there's work queued.

**Target setup**: 2x H100 NVL @ $3.07/hr, configurable to 4x+ for FP8 if needed later.

**Model**: MiniMax-M2.5 INT4-AWQ (~120GB weights, fits 188GB VRAM). Full-precision M2.5 scores 80.2% SWE-Bench Verified (0.6% behind Opus 4.6). INT4-AWQ: no published degradation benchmarks, anecdotal "no visible difference." MoE architecture (256 experts, 8 active) quantizes gracefully. Attention stays FP8/BF16.

**Capability**: Runner advertises **heavy** — trusting verification to catch quant-induced issues. MiniMax with additional verification does allow for "heavy" usage. Future: research/design/planning (heavy) produce well-specified subtasks.

### Economic Reality Check

**RunPod vs Claude Max ($100/mo):**

| | Claude Max $100/mo | Claude Pro $20/mo + RunPod | RunPod Only |
|---|---|---|---|
| Interactive (chat/debug) | 6-8 hrs/day, throttled windows | 3-4 hrs/day, throttled | None |
| Automated runner | Uses same quota pool | Unlimited, $3.07/hr | Unlimited, $3.07/hr |
| Rate limits | Yes, 5-hr rolling windows | Yes (interactive), No (runner) | No |
| Monthly runner budget | $0 (included) | $80 saved → 26 GPU-hrs | Full budget |

**The honest math**: $80/mo savings buys ~26 hrs GPU time. At 100 tok/s and ~2min/run, that's ~780 automated runs/month. The Claude Max subscription probably lets you do similar run volume via API — but it's throttled, shared with your interactive usage, and subject to variable rate limits.

**Where self-hosted wins**: Not raw cost savings for a single user — it's **throughput consistency**. When Claude Max throttles you at 5-hour windows during a heavy sprint, the RunPod runner keeps going. No rate limits, no quota sharing, no throttling. It's a dedicated build machine that costs $3.07/hr only when you're using it.

**Where it doesn't win yet**: For a single user doing <30 automated runs/day, just keep the Max subscription. The economics tip at scale: multiple projects, team members, or subtask decomposition generating 100+ runs/day.

**$200/mo Claude comparison**: At ~$6.67/day, that's 2.2 hours of RunPod time. If the $200 tier gives you 10x Pro throughput unthrottled, it's probably better for a single user unless you're doing heavy automated batch work. The self-hosted runner becomes clearly better at 3+ hours/day of automated builds.

**Bottom line**: Build the infrastructure now. It pays for itself when subtask decomposition fills the queue. Until then, keep it as an on-demand overflow valve — spin up when Claude is throttled or when you have a batch sprint.

---

## Design Decision: Pull-Based Runner Control

Runners are controllable via pull — the runner polls the server for pending config changes on each poll cycle. No inbound connectivity to the runner is needed. This eliminates callback URLs, ephemeral token auth on the runner side, port exposure, and RunPod proxy complexity.

**Why pull, not push:** RunPod containers are non-privileged (no `/dev/net/tun`), so Tailscale kernel-mode networking isn't available. Tailscale userspace mode handles outbound only — incoming TCP to the pod is unreliable. Rather than building a fragile push path through RunPod's Cloudflare-fronted HTTP proxy, we make all communication runner→server (outbound from the pod) and let the runner poll for config changes.

**Extended registration:**

The existing `POST /api/runners/register` already sends `{ runner_id, backend_name, capability }`. We extend it with utilization metrics reported on every poll cycle:

```
POST /api/runners/register  (existing, extended payload)
{
    runner_id, backend_name, capability,          // existing
    poll_interval, max_concurrent, max_builds,    // new: current config
    active_count, active_builds                   // new: current utilization
}
```

The runner re-sends this on every poll cycle (piggybacked on the claim request or as a separate heartbeat), keeping `RunnerInfo` fresh on the server. The server responds with any pending config:

```
Response (on claim or heartbeat):
{
    pending_config: {                 // null if no changes pending
        poll_interval: 2,
        drain: true
    }
}
```

**Config change flow:**
1. TUI/admin calls `PUT /api/infra/runners/{id}/config` → `{ poll_interval: 2, drain: true }`
2. Server stores pending config in `RunnerInfo.pending_config`
3. Runner's next poll cycle sees the pending config in the response
4. Runner applies changes to its `RuntimeConfig`, acknowledges via next heartbeat
5. Server clears `pending_config` once acknowledged

**Drain flow (clean spindown):**
1. Pod manager (or TUI via infra API) decides to spin down
2. Server sets `pending_config: { drain: true }` on the RunPod runner
3. Runner picks up drain on next poll (within 2-20 seconds depending on poll_interval)
4. Runner stops claiming new work, finishes active runs
5. Runner reports `{ status: "drained", active_count: 0 }` in its next heartbeat
6. Pod manager sees drained → `stop_pod()` safely, no work killed

The idle timeout still exists as a **fallback** — if a runner has been finding no work for N minutes and has nothing active, the pod manager can stop it directly without drain (nothing's running anyway).

**Aggressiveness profiles** (configurable via TUI):

| Profile | poll_interval | max_concurrent | max_builds | Use case |
|---------|--------------|----------------|------------|----------|
| Hyper aggressive | 2s | 5 | 3 | RunPod GPU runner (saturate first) |
| Standard | 5s | 2 | 1 | Balanced |
| Lackadaisical | 20s | 1 | 1 | Subscription runner (overflow only) |

TUI calls `PUT /api/infra/runners/{id}/config` → server stores pending config → runner picks it up within one poll cycle → adjusts live. All runners still compete via FIFO claims — aggressiveness is purely how fast and how many slots they bring to the table.

**Utilization metrics**: Updated every poll cycle via the extended registration/heartbeat. Server exposes per-runner utilization in `GET /api/infra/runners`. TUI renders saturation percentage per runner.

### RuntimeConfig (hot-reloadable subset)

```rust
pub struct RuntimeConfig {
    pub poll_interval: u64,
    pub drain: bool,
}
```

Initialized from `RunnerConfig` at startup. The main poll loop reads `poll_interval` and `drain` from `Arc<RwLock<RuntimeConfig>>`. When `drain` becomes true, the poll loop stops claiming and enters the existing graceful-shutdown path (`main.rs:214-242`).

**Semaphore resizing is deferred.** `max_concurrent` and `max_builds` are reported to the server for utilization tracking but not hot-reloaded on the runner (Tokio `Semaphore` doesn't support resizing). Changes to these values log a warning — "will take effect on next runner restart." Phase B can replace `Semaphore` with a custom capacity tracker if needed.

### Runner Health Server — Unchanged

The existing health server (`main.rs:484-493`) stays as-is: axum on `127.0.0.1:{health_port}`, single `/health` route, no auth. It's a local diagnostic endpoint — not involved in server↔runner communication. No `--health-host`, no `--callback-url`, no inbound ports.

## Design Decision: Use Existing `runpod` Crate

The [`runpod`](https://crates.io/crates/runpod) crate (v0.1.30, MIT, async/tokio, [agentsea/runpod.rs](https://github.com/agentsea/runpod.rs)) already provides typed interfaces:
- `create_on_demand_pod()`, `create_spot_pod()`
- `list_pods()`, `get_pod()`, `stop_pod()`
- GPU type queries, env vars, network volumes

**What we need to verify**: whether the crate supports `start_pod()` (resume a stopped pod) and `terminate_pod()`. If missing, we add a thin extension using reqwest directly (we already have it in the workspace).

**Approach**: Add `runpod` as a dependency. Write a thin `FlowstateRunPod` wrapper in `crates/flowstate-server/src/pod_manager.rs` that provides exactly the interface we need, delegating to the `runpod` crate where possible and filling gaps with raw REST calls.

No separate `flowstate-runpod` crate — the `runpod` crate IS our RunPod client. Our code is just the pod lifecycle management logic.

## Design Decision: Networking — Tailscale Userspace + Pull-Only

All communication is **outbound from the RunPod pod**. No inbound ports, no proxy URLs, no Cloudflare in the path.

```
flowstate-server (your tailnet, e.g. 100.x.y.z:3710)
    ↑ receives poll/claim/progress/heartbeat from runner (HTTPS over tailnet)
    ↑ manages pods via RunPod REST API (public internet)

RunPod pod:
    tailscaled (userspace mode, SOCKS5 proxy on 127.0.0.1:1055)
    flowstate-runner → flowstate-server (via SOCKS5 → tailnet)
    flowstate-runner → vLLM (127.0.0.1:8000, never exposed)
    no inbound ports exposed
```

### How it works

1. **Tailscale userspace networking** runs inside the pod container without `/dev/net/tun` (non-privileged containers). It starts `tailscaled --tun=userspace-networking --socks5-server=localhost:1055` and authenticates with an ephemeral auth key.

2. **Runner HTTP client** uses `localhost:1055` as a SOCKS5 proxy. All requests to the flowstate server go through the Tailscale tunnel. The server URL is its tailnet IP (e.g., `http://100.x.y.z:3710`).

3. **Object store is not accessed directly.** The runner reads/writes specs, plans, research, verification, and run outputs through the **server's HTTP API** (`GET /api/tasks/{id}/spec`, `PUT /api/tasks/{id}/plan`, etc.). The server proxies these to the object store (Garage S3 or local filesystem). The runner has zero knowledge of the store backend and doesn't need network access to it.

4. **Config/drain is pull-based.** The runner polls the server for pending config changes on each cycle (see "Pull-Based Runner Control" above). No inbound connectivity to the pod needed.

### Why Tailscale userspace (not kernel mode)

RunPod community containers are [non-privileged](https://www.answeroverflow.com/m/1232985784189976667) — `/dev/net/tun` is unavailable. [Tailscale userspace mode](https://tailscale.com/kb/1112/userspace-networking) works without TUN by running a SOCKS5/HTTP proxy. Outbound connections work reliably; inbound TCP is [unreliable](https://github.com/tailscale/tailscale/issues/2642) in userspace mode, but we don't need inbound (pull-based design).

### Why not direct internet access

The flowstate server doesn't need to be internet-accessible. It stays on your tailnet, behind your firewall. The RunPod pod joins the tailnet via Tailscale and reaches the server through the encrypted WireGuard tunnel. No public IP, no cloudflared, no port forwarding.

### Tailscale auth for pods

- Pod manager injects a **Tailscale ephemeral auth key** (`TS_AUTHKEY`) into the pod env at creation time
- Ephemeral keys auto-expire and the node is removed from the tailnet when it disconnects — clean for pods that spin up and down
- Auth key created via `tailscale` CLI or Tailscale API, stored in `FLOWSTATE_RUNPOD_TS_AUTHKEY` env var on the server
- Tailscale ACLs can restrict the pod node to only reach the flowstate server's IP:port

### Networking prerequisites (documented in `docs/runpod-setup.md`)

1. flowstate-server is running on a machine with Tailscale installed and connected to your tailnet
2. You have a Tailscale ephemeral auth key (or OAuth client for automated key generation)
3. `FLOWSTATE_SERVER_URL` is set to the server's tailnet IP (e.g., `http://100.x.y.z:3710`)
4. Tailscale ACLs allow the pod's ephemeral node to reach the server


## Design Decision: Config via DB + TUI (Phased)

**Phase A (this PR)**: Env vars for bootstrap + pull-based runner control.
- Pod manager reads config from env
- Runner reports utilization on every poll cycle; server responds with pending config if any
- Aggressiveness is controlled via poll_interval, adjustable at runtime via pending config
- Config is ephemeral (lives in-memory on server + runner, lost on restart)

**Phase B (follow-up)**: DB-backed runner config + admin API keys.
- `runner_configs` table: aggressiveness profiles, pod env overrides (Tailscale auth key, server tailnet IP), defaults per runner type
- Admin CRUD API, `api_keys.role` column: `user` vs `admin`
- Pod manager reads from DB (env fallback)
- On runner registration, server auto-sets pending config from persisted profile

**Phase C (follow-up)**: TUI infrastructure tab.
- Queue depth, active runners (type/capability/backend/saturation%), RunPod pod status
- Cost metrics: daily spend, cost/run, budget remaining
- Admin actions: start/stop pod (with drain), change runner aggressiveness profiles, edit pod env config

---

## Phase 1: Pod Manager — Server Background Task

Since we're using the `runpod` crate, Phase 1 is the pod manager itself — no separate API client crate.

Follows `watchdog.rs` pattern (`crates/flowstate-server/src/watchdog.rs:20-28`).

**New file:** `crates/flowstate-server/src/pod_manager.rs`

**Configuration** (env vars; pod manager disabled if `RUNPOD_API_KEY` unset):
```
FLOWSTATE_RUNPOD_API_KEY          — RunPod API key (required to enable)
FLOWSTATE_RUNPOD_POD_ID           — Existing pod ID (skip first creation)
FLOWSTATE_RUNPOD_TEMPLATE_IMAGE   — Docker image for new pod
FLOWSTATE_RUNPOD_GPU_TYPE         — default "NVIDIA H100 NVL"
FLOWSTATE_RUNPOD_GPU_COUNT        — default 2
FLOWSTATE_RUNPOD_NETWORK_VOLUME   — network volume ID for model weights
FLOWSTATE_RUNPOD_IDLE_TIMEOUT     — seconds idle before stopping (default 600)
FLOWSTATE_RUNPOD_QUEUE_THRESHOLD  — min queued runs to spin up (default 1)
FLOWSTATE_RUNPOD_SCAN_INTERVAL    — seconds between checks (default 30)
FLOWSTATE_RUNPOD_MAX_DAILY_SPEND  — cost cap in dollars (default: unlimited)
FLOWSTATE_RUNPOD_SPINDOWN_THRESHOLD — queued runs below which to start drain (default 0)
FLOWSTATE_RUNPOD_DRAIN_TIMEOUT    — seconds to wait for drain before force-stop (default 600)
FLOWSTATE_RUNPOD_CLOUD_TYPE       — "SECURE" or "COMMUNITY" (default "COMMUNITY")
FLOWSTATE_RUNPOD_POD_ENV          — JSON of env vars injected into pod
FLOWSTATE_RUNPOD_TS_AUTHKEY       — Tailscale ephemeral auth key for pod networking
```

**Hybrid lifecycle:**
1. First spin-up: `create_on_demand_pod()` → persist pod ID to `$FLOWSTATE_DATA_DIR/runpod_pod_id`
2. Subsequent: `start_pod(stored_id)` / `stop_pod(stored_id)`
3. Explicit cleanup: `terminate_pod()` via admin API only

**State** (`Arc<Mutex<PodManagerState>>` in `InnerAppState`):
```rust
pub struct PodManagerState {
    pub pod_id: Option<String>,
    pub pod_status: PodStatus,     // Stopped, Starting, Running, Draining, Drained
    pub last_work_seen: Instant,
    pub daily_cost_cents: u64,
    pub day_start: DateTime<Utc>,
    pub cost_capped: bool,
    pub drain_requested_at: Option<Instant>,
}
```

**Decision loop** (every `scan_interval` seconds):
```
1. queued = db.count_queued_runs()
2. pod_state = runpod.get_pod(pod_id) if known
3. runner_state = lookup RunPod runner in runners HashMap (by backend/capability)

SPIN UP:
  queued >= queue_threshold AND pod stopped/none AND NOT cost_capped
  → start_pod(pod_id) or create_on_demand_pod() on first run
  → update pod_status

STAY WARM:
  pod running AND queued > 0
  → reset last_work_seen

DRAIN (intentional spindown):
  pod running AND queued <= spindown_threshold
  AND idle > idle_timeout AND NOT already draining
  → set pending_config { drain: true } on the RunPod runner in runners HashMap
  → set pod_status = Draining, record drain_requested_at
  → runner picks up drain on next poll, stops claiming, finishes active work

DRAIN COMPLETE:
  pod_status == Draining AND runner reported status "drained" (via heartbeat)
  → stop_pod(pod_id)
  → set pod_status = Stopped

DRAIN TIMEOUT (defense-in-depth):
  pod_status == Draining AND drain_requested_at > drain_timeout (e.g. 10 min)
  → log warning, stop_pod(pod_id) anyway (runner may have crashed)

COST CAP:
  daily_spend > max_daily_spend
  → drain first if running, then stop_pod, set cost_capped = true
```

**DB addition:**
- `count_queued_runs() -> Result<i64>` on `Database` trait
- `SELECT COUNT(*) FROM claude_runs WHERE status = 'queued'`
- Implement in SQLite + Postgres query modules

**Files to create:**
- `crates/flowstate-server/src/pod_manager.rs`

**Files to modify:**
- `crates/flowstate-server/Cargo.toml` — add `runpod` dependency
- `crates/flowstate-server/src/lib.rs` — add `pub mod pod_manager;`, spawn in `serve()`
- `crates/flowstate-server/src/routes/mod.rs` — `PodManagerState` in `InnerAppState`; extend `RunnerInfo` with `active_count`, `max_concurrent`, `active_builds`, `max_builds`, `poll_interval`, `pending_config: Option<PendingConfig>`, `status: RunnerStatus` (active/drained)
- `crates/flowstate-server/src/routes/claude_runs.rs` — extend `POST /api/runners/register` to accept utilization metrics and return pending config in response; extend claim response to include pending config
- `crates/flowstate-db/src/lib.rs` — add `count_queued_runs()` to `Database` trait
- `crates/flowstate-db/src/sqlite/queries/claude_runs.rs` — implement
- `crates/flowstate-db/src/postgres/queries/claude_runs.rs` — implement
- `crates/flowstate-runner/src/config.rs` — add `RuntimeConfig { poll_interval, drain }` behind `Arc<RwLock<>>`
- `crates/flowstate-runner/src/main.rs` — read `RuntimeConfig` in poll loop; report utilization in registration/heartbeat; parse pending config from claim/heartbeat response; apply drain

**Tests**:
- Pod manager decision logic: mock DB counts, mock RunPod calls. Spin-up, drain, drain-timeout, cost cap, hybrid lifecycle, first-creation persistence
- Extended registration: utilization metrics stored in RunnerInfo, pending config returned in response
- Pending config: server stores pending config → runner receives it on next poll → runner applies and acknowledges → server clears
- Drain flow: pod manager sets pending drain → runner picks up on poll → runner stops claiming → reports drained via heartbeat → pod manager stops pod

## Phase 2: Docker Image & Runner Config

**New files:**
- `docker/runpod/Dockerfile`
- `docker/runpod/entrypoint.sh`
- `docs/runpod-setup.md`

**Entrypoint** (`docker/runpod/entrypoint.sh`):
```bash
#!/bin/bash
set -euo pipefail

# 1. Start Tailscale in userspace mode (no TUN device needed)
tailscaled --tun=userspace-networking \
    --socks5-server=localhost:1055 \
    --state=/tmp/tailscale-state &
sleep 2
tailscale up --authkey="${TS_AUTHKEY}" --hostname="flowstate-runner-${RUNPOD_POD_ID:-unknown}"
echo "Tailscale connected."

# 2. Start vLLM on local GPUs (localhost only, never exposed externally)
SAFETENSORS_FAST_GPU=1 vllm serve \
    "${VLLM_MODEL:-MiniMaxAI/MiniMax-M2.5}" \
    --trust-remote-code \
    --tensor-parallel-size "${VLLM_TP_SIZE:-2}" \
    --quantization "${VLLM_QUANTIZATION:-compressed-tensors}" \
    --enable-auto-tool-choice --tool-call-parser minimax_m2 \
    --reasoning-parser minimax_m2_append_think \
    --host 127.0.0.1 --port 8000 &

# 3. Wait for vLLM health
echo "Waiting for vLLM..."
until curl -sf http://127.0.0.1:8000/health > /dev/null 2>&1; do sleep 5; done
echo "vLLM ready."

# 4. Start flowstate-runner
# SOCKS5 proxy at localhost:1055 routes traffic through tailnet to the server
exec flowstate-runner \
    --agent-backend opencode \
    --runner-capability heavy \
    --max-concurrent "${MAX_CONCURRENT:-3}" \
    --max-builds "${MAX_BUILDS:-2}"
```

**SOCKS5 proxy configuration:** The runner's HTTP client (`HttpService`) needs to route through `localhost:1055`. Options:
- Set `ALL_PROXY=socks5://127.0.0.1:1055` in the env (reqwest respects this)
- Or add a `--socks-proxy` CLI flag to the runner

The simplest approach: set `ALL_PROXY` in the entrypoint before `exec flowstate-runner`. This also routes git clone/push through the tunnel if the git repo is on the tailnet.

**Pod env vars** (injected at creation via `FLOWSTATE_RUNPOD_POD_ENV`):
```
FLOWSTATE_SERVER_URL=http://100.x.y.z:3710          ← server's tailnet IP
FLOWSTATE_API_KEY=<runner-api-key>
FLOWSTATE_AGENT_BACKEND=opencode
FLOWSTATE_OPENCODE_PROVIDER=openai
FLOWSTATE_OPENCODE_BASE_URL=http://127.0.0.1:8000/v1  ← local vLLM on same pod
FLOWSTATE_OPENCODE_MODEL=MiniMaxAI/MiniMax-M2.5
FLOWSTATE_OPENCODE_API_KEY=not-needed
FLOWSTATE_RUNNER_CAPABILITY=heavy
TS_AUTHKEY=tskey-auth-xxxxx                            ← ephemeral Tailscale auth key
ALL_PROXY=socks5://127.0.0.1:1055                      ← route through tailnet
```

## Phase 3: Server API — Pod Status, Cost, Manual Control

**New file:** `crates/flowstate-server/src/routes/infra.rs`

**Endpoints** (protected, eventually admin-only):
```
GET  /api/infra/gpu-status    — { pod_id, status, uptime_secs, cost_today_cents,
                                   queue_depth, drain_status }
POST /api/infra/gpu/start     — manual spin-up (returns pod status)
POST /api/infra/gpu/stop      — set drain on RunPod runner, pod manager handles stop after drained
GET  /api/infra/runners       — list registered runners with utilization:
                                 { id, capability, backend, last_seen, status,
                                   active_count, max_concurrent, active_builds, max_builds,
                                   saturation_pct, poll_interval }
PUT  /api/infra/runners/{id}/config — set pending config on a runner:
                                 { poll_interval, drain }
                                 Stored in RunnerInfo.pending_config.
                                 Runner picks it up on next poll cycle.
```

Foundation for TUI infrastructure tab (Phase C follow-up). The `PUT .../config` endpoint is how the TUI controls runner aggressiveness — select a preset profile or set custom values. The server stores the pending config; the runner picks it up on its next poll cycle (within 2-20s depending on current poll_interval). No server→runner connectivity needed.

**Files to modify:**
- `crates/flowstate-server/src/routes/mod.rs` — add `pub mod infra;`, merge into protected routes

---

## Future Work (Depends on Subtask PR)

### Orchestrator Branch Strategy
- Parent build creates `flowstate/{parent-slug}` branch
- Subtask builds use parent branch as base
- Impact: `pipeline.rs` base branch resolution

### Merge Queue
- Server-side `merge_queue.rs` background task
- Subtask PR passes verification → auto-merge into parent task branch
- Conflict → re-build subtask with conflict context
- All done → final PR from task branch → main

### Queue Saturation
- Planning phase decomposes work into subtasks
- Runner creates subtasks via `HttpService::create_task()`
- Queue fills → pod spins up → GPU saturated
- MiniMax at heavy capability handles both planning AND building with verification

### Enhanced Verification (Two-Layer Strategy)
- Pull PR diff via `RepoProvider::get_pr_diff`
- Run coverage target, parse report for uncovered diff lines
- **Showstoppers** (fix in-PR, same commit):
  - Compilation errors, test failures
  - Missing critical coverage on new code
  - **Spec non-compliance** — "does this actually do the thing we set out to do?"
- **Deferrables** (create follow-up subtask, don't block PR):
  - Style issues, minor coverage gaps, TODOs, doc updates

### DB-Backed Runner Config (Phase B)
- `runner_configs` table: persists aggressiveness profiles, pod env overrides (Tailscale auth key, server tailnet IP, model config), defaults per runner type
- Admin CRUD API for runner configs
- `api_keys.role`: `user` vs `admin`
- Pod manager reads from DB (env fallback)
- On pod creation, pod env assembled from DB config instead of raw env var
- On runner registration, server auto-sets pending config from persisted profile (runner picks it up on first poll)
- Hot-reload of `max_concurrent`/`max_builds` via custom capacity tracker replacing Tokio Semaphore

### TUI Infrastructure Tab (Phase C)
- Expand Health view: queue depth, runner list (type/capability/backend/status/saturation%)
- RunPod pod status (Stopped/Starting/Running/Draining/Drained), daily cost, cost/run
- Per-runner utilization bars: `active_count/max_concurrent`, `active_builds/max_builds`
- Admin: start/stop pod (with drain), change aggressiveness profile (dropdown: hyper-aggressive / standard / lackadaisical / custom)
- Edit pod env config (Tailscale auth, model, etc.) — writes to DB, applied on next pod creation

---

## Implementation Order

```
Phase 1: Pod manager + DB query + extended registration/heartbeat + pending config + drain flow
Phase 2: Docker image + Tailscale userspace + entrypoint + docs
Phase 3: Infra API routes (foundation for TUI)
```

Phase 1 establishes the pull-based runner control pattern. Phase 2 adds the Docker image with Tailscale networking. Phase 3 wires it up for TUI admin control.

## Verification

1. Pod manager decision logic tests: mock DB counts, mock RunPod calls. Spin-up, drain, drain-timeout, cost cap, hybrid lifecycle
2. DB `count_queued_runs()` tests in SQLite + Postgres
3. Extended registration: utilization metrics round-trip, pending config returned and cleared after ack
4. RuntimeConfig: poll_interval hot-reload, drain flag stops claiming
5. Drain flow: pod manager sets pending drain on runner → runner picks up on poll → stops claiming → reports drained → pod manager stops pod
6. `#[ignore]` integration test: real RunPod API (needs API key)
7. Docker image build + local test with mock vLLM endpoint + Tailscale connectivity
8. End-to-end: queue run → pod starts → tailscale connects → runner claims → executes → drain → pod stops
9. Clippy: `nix develop -c cargo clippy -- -D warnings`
10. Full suite: `nix develop -c cargo test --workspace --all-features`

## File Summary

| New | Purpose |
|-----|---------|
| `crates/flowstate-server/src/pod_manager.rs` | Pod lifecycle background task (spin-up/drain/stop) |
| `crates/flowstate-server/src/routes/infra.rs` | GPU status/control, runner list, pending config API |
| `docker/runpod/Dockerfile` | Tailscale + vLLM + runner image |
| `docker/runpod/entrypoint.sh` | Tailscale setup + pod startup script |
| `docs/runpod-setup.md` | Setup guide + Tailscale networking + economics |

| Modified | Change |
|----------|--------|
| `Cargo.toml` | Add `runpod` to workspace deps |
| `crates/flowstate-server/Cargo.toml` | Add `runpod` dependency |
| `crates/flowstate-server/src/lib.rs` | Add `pod_manager` module, spawn in `serve()` |
| `crates/flowstate-server/src/routes/mod.rs` | Add `infra` routes, `PodManagerState` to `InnerAppState`, extend `RunnerInfo` with utilization + pending_config fields |
| `crates/flowstate-server/src/routes/claude_runs.rs` | Extend `register_runner` to accept utilization, return pending config; extend claim response |
| `crates/flowstate-db/src/lib.rs` | Add `count_queued_runs()` to `Database` trait |
| `crates/flowstate-db/src/sqlite/queries/claude_runs.rs` | Implement count query |
| `crates/flowstate-db/src/postgres/queries/claude_runs.rs` | Implement count query |
| `crates/flowstate-runner/src/config.rs` | Add `RuntimeConfig { poll_interval, drain }` behind `Arc<RwLock<>>` |
| `crates/flowstate-runner/src/main.rs` | Report utilization on poll; parse pending config from responses; apply RuntimeConfig changes |
