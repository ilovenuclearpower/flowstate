# Flowstate Runner — RunPod GPU Pod Image
#
# Uses nix to build the runner binary with the exact same toolchain as
# development (fenix stable via crane), then layers it into a vLLM
# image with Tailscale for outbound-only networking.
#
# Build:
#   docker build -t flowstate-runner-gpu-tailscale -f docker/runpod/Dockerfile .
#
# The nix builder stage handles Rust compilation — no system Rust needed.

# ---- Builder stage: nix builds the runner with the correct toolchain ----
FROM nixos/nix:latest AS builder

RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

WORKDIR /src
COPY . .

# Build the runner binary via the flake (uses fenix stable + crane)
# RUST_MIN_STACK: prevent rustc ICE on large dependency trees (matches devShell)
RUN RUST_MIN_STACK=67108864 nix build .#runner

# Collect the runtime closure AND the openssl libraries (crane doesn't
# embed RPATH for openssl, so it's missing from nix-store -qR).
RUN readlink -f result/bin/flowstate-runner > /runner-path \
    && mkdir -p /closure/nix/store \
    && for p in $(nix-store -qR result); do cp -a "$p" /closure/nix/store/; done \
    && OPENSSL_STORE=$(find /nix/store -path '*/lib/libssl.so.3' -not -path '*-dev/*' -not -path '/closure/*' 2>/dev/null | head -1 | xargs dirname | xargs dirname) \
    && cp -a "$OPENSSL_STORE" /closure/nix/store/ \
    && basename "$OPENSSL_STORE" > /openssl-store-name

# ---- Runtime stage: vLLM (includes CUDA + Python + PyTorch) + Tailscale ----
FROM vllm/vllm-openai:latest

USER root

# Install Tailscale (userspace networking — no /dev/net/tun in RunPod)
RUN apt-get update && apt-get install -y --no-install-recommends \
    iptables iproute2 \
    && curl -fsSL https://pkgs.tailscale.com/stable/ubuntu/jammy.noarmor.gpg \
       | tee /usr/share/keyrings/tailscale-archive-keyring.gpg >/dev/null \
    && curl -fsSL https://pkgs.tailscale.com/stable/ubuntu/jammy.tailscale-keyring.list \
       | tee /etc/apt/sources.list.d/tailscale.list \
    && apt-get update && apt-get install -y tailscale \
    && rm -rf /var/lib/apt/lists/*

# Copy the nix store closure (runner binary + glibc + openssl + deps)
COPY --from=builder /closure/nix/store /nix/store
COPY --from=builder /runner-path /tmp/runner-path
COPY --from=builder /openssl-store-name /tmp/openssl-store-name
RUN ln -sf "$(cat /tmp/runner-path)" /usr/local/bin/flowstate-runner \
    && rm /tmp/runner-path

# Copy the entrypoint and bake in the openssl LD_LIBRARY_PATH
COPY docker/runpod/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh \
    && OPENSSL_LIB="/nix/store/$(cat /tmp/openssl-store-name)/lib" \
    && sed -i "2i export LD_LIBRARY_PATH=\"$OPENSSL_LIB\"" /entrypoint.sh \
    && rm /tmp/openssl-store-name \
    && LD_LIBRARY_PATH="$OPENSSL_LIB" flowstate-runner --help >/dev/null 2>&1

# Environment defaults
ENV FLOWSTATE_SERVER_URL=""
ENV FLOWSTATE_API_KEY=""
ENV FLOWSTATE_RUNNER_CAPABILITY="heavy"
ENV FLOWSTATE_AGENT_BACKEND="claude-cli"
ENV FLOWSTATE_MAX_CONCURRENT="2"
ENV FLOWSTATE_MAX_BUILDS="1"
ENV FLOWSTATE_POLL_INTERVAL="5"
ENV TS_AUTHKEY=""
ENV TS_SERVER_IP=""
ENV VLLM_MODEL="MiniMaxAI/MiniMax-M1-80k"
ENV VLLM_PORT="8000"
ENV VLLM_MAX_MODEL_LEN="80000"
ENV HF_TOKEN=""

ENTRYPOINT ["/entrypoint.sh"]
