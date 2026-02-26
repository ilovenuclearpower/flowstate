# Flowstate Server — Nix-based Docker Image
#
# Single-stage nix build: the binary runs against the same glibc/openssl
# it was compiled with. No library mismatch issues.
#
# Build:
#   docker build -t flowstate-server -f docker/server/Dockerfile .

FROM nixos/nix:latest

RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

WORKDIR /src
COPY . .

# Build the server binary via the flake
# RUST_MIN_STACK: prevent rustc ICE on large dependency trees (matches devShell)
RUN RUST_MIN_STACK=67108864 nix build .#server \
    && nix-env -i ./result \
    && rm -rf /src

# Create entrypoint that sets LD_LIBRARY_PATH for nix's openssl
# (crane doesn't embed RPATH for openssl in the binary)
RUN OPENSSL_LIB=$(find /nix/store -path '*/lib/libssl.so.3' -not -path '*-dev/*' 2>/dev/null | head -1 | xargs dirname) \
    && printf '#!/bin/sh\nexport LD_LIBRARY_PATH="%s"\nexec flowstate-server "$@"\n' "$OPENSSL_LIB" \
       > /entrypoint.sh \
    && chmod +x /entrypoint.sh \
    && /entrypoint.sh --help >/dev/null 2>&1

# Environment defaults (SQLite backend)
ENV FLOWSTATE_DB_BACKEND="sqlite"
ENV FLOWSTATE_HOST="0.0.0.0"
ENV FLOWSTATE_PORT="3710"

EXPOSE 3710

ENTRYPOINT ["/entrypoint.sh"]
