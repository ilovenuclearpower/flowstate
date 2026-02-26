#!/bin/bash
set -euo pipefail

echo "=== Flowstate Runner GPU Pod ==="
echo "Starting services..."

# ---------- 1. Tailscale (userspace networking) ----------
if [ -n "${TS_AUTHKEY:-}" ]; then
    echo "Starting Tailscale (userspace networking)..."
    tailscaled --tun=userspace-networking --socks5-server=localhost:1055 --state=/tmp/tailscale &
    sleep 2

    tailscale up --authkey="$TS_AUTHKEY" --hostname="flowstate-gpu-$(hostname)"
    echo "Tailscale connected"

    # Route all runner traffic through SOCKS5 proxy to the tailnet
    export ALL_PROXY="socks5://localhost:1055"
    export HTTPS_PROXY="socks5://localhost:1055"

    # If TS_SERVER_IP is set, use it to override the server URL
    if [ -n "${TS_SERVER_IP:-}" ]; then
        export FLOWSTATE_SERVER_URL="http://${TS_SERVER_IP}:3710"
        echo "Server URL: $FLOWSTATE_SERVER_URL (via Tailscale)"
    fi
else
    echo "No TS_AUTHKEY set, skipping Tailscale"
fi

# ---------- 2. HuggingFace cache (network volume) ----------
# RunPod mounts network volumes at /runpod-volume. Use it for model
# caching so models persist across pod restarts and don't re-download.
if [ -d "/runpod-volume" ]; then
    export HF_HOME="/runpod-volume/huggingface"
    mkdir -p "$HF_HOME"
    echo "HuggingFace cache: $HF_HOME (network volume)"
else
    echo "No network volume at /runpod-volume, using default HF cache"
fi

# ---------- 3. vLLM (local model serving) ----------
if [ -n "${VLLM_MODEL:-}" ]; then
    echo "Starting vLLM with model: $VLLM_MODEL"
    python3 -m vllm.entrypoints.openai.api_server \
        --model "$VLLM_MODEL" \
        --port "${VLLM_PORT:-8000}" \
        --trust-remote-code \
        --max-model-len "${VLLM_MAX_MODEL_LEN:-80000}" \
        &
    VLLM_PID=$!

    # Wait for vLLM to be healthy
    echo "Waiting for vLLM to start..."
    for i in $(seq 1 120); do
        if curl -sf "http://localhost:${VLLM_PORT:-8000}/health" >/dev/null 2>&1; then
            echo "vLLM ready after ${i}s"
            break
        fi
        if ! kill -0 $VLLM_PID 2>/dev/null; then
            echo "ERROR: vLLM process died"
            exit 1
        fi
        sleep 1
    done

    # Point the runner at the local vLLM endpoint
    export FLOWSTATE_ANTHROPIC_BASE_URL="http://localhost:${VLLM_PORT:-8000}/v1"
else
    echo "No VLLM_MODEL set, skipping local model serving"
fi

# ---------- 4. Flowstate Runner ----------
echo "Starting flowstate-runner..."
exec flowstate-runner \
    --poll-interval "${FLOWSTATE_POLL_INTERVAL:-5}" \
    --max-concurrent "${FLOWSTATE_MAX_CONCURRENT:-2}" \
    --max-builds "${FLOWSTATE_MAX_BUILDS:-1}"
