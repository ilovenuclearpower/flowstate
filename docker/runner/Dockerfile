# Flowstate Runner — Nix-based Docker Image (no GPU)
#
# Single-stage nix build: the binary runs against the same glibc/openssl
# it was compiled with. No library mismatch issues.
#
# Build:
#   docker build -t flowstate-runner -f docker/runner/Dockerfile .

FROM nixos/nix:latest

RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

WORKDIR /src
COPY . .

# Build the runner binary via the flake
# RUST_MIN_STACK: prevent rustc ICE on large dependency trees (matches devShell)
# git-minimal is already in the nixos/nix base image (supports clone/push/fetch)
RUN RUST_MIN_STACK=67108864 nix build .#runner \
    && nix-env -i ./result \
    && rm -rf /src

# Create entrypoint that sets LD_LIBRARY_PATH for nix's openssl
# (crane doesn't embed RPATH for openssl in the binary)
RUN OPENSSL_LIB=$(find /nix/store -path '*/lib/libssl.so.3' -not -path '*-dev/*' 2>/dev/null | head -1 | xargs dirname) \
    && printf '#!/bin/sh\nexport LD_LIBRARY_PATH="%s"\nexec flowstate-runner \\\n    --poll-interval "${FLOWSTATE_POLL_INTERVAL:-5}" \\\n    --max-concurrent "${FLOWSTATE_MAX_CONCURRENT:-2}" \\\n    --max-builds "${FLOWSTATE_MAX_BUILDS:-1}" \\\n    "$@"\n' "$OPENSSL_LIB" \
       > /entrypoint.sh \
    && chmod +x /entrypoint.sh \
    && /entrypoint.sh --help >/dev/null 2>&1

# Environment defaults
ENV FLOWSTATE_SERVER_URL=""
ENV FLOWSTATE_API_KEY=""
ENV FLOWSTATE_RUNNER_CAPABILITY="standard"
ENV FLOWSTATE_AGENT_BACKEND="claude-cli"
ENV FLOWSTATE_MAX_CONCURRENT="2"
ENV FLOWSTATE_MAX_BUILDS="1"
ENV FLOWSTATE_POLL_INTERVAL="5"

ENTRYPOINT ["/entrypoint.sh"]
