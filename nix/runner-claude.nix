{ pkgs }:
let
  runtimePath = pkgs.lib.makeBinPath [
    pkgs.coreutils
    pkgs.bash
    pkgs.curl
  ];

  runnerClaude = pkgs.writeShellScriptBin "runner-claude" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    # ── Load runner credentials if available ──
    RUNNER_CRED_FILE="''${XDG_DATA_HOME:-$HOME/.local/share}/flowstate/runner/credentials/runner.env"
    if [ -f "$RUNNER_CRED_FILE" ]; then
      set -a
      source "$RUNNER_CRED_FILE"
      set +a
    fi

    # ── Defaults ──
    SERVER_URL="''${FLOWSTATE_SERVER_URL:-http://127.0.0.1:3710}"
    API_KEY="''${FLOWSTATE_API_KEY:-}"
    ANTHROPIC_AUTH_TOKEN="''${FLOWSTATE_ANTHROPIC_AUTH_TOKEN:-}"
    ANTHROPIC_BASE_URL="''${FLOWSTATE_ANTHROPIC_BASE_URL:-}"
    ANTHROPIC_MODEL="''${FLOWSTATE_ANTHROPIC_MODEL:-}"
    HEALTH_PORT="''${FLOWSTATE_HEALTH_PORT:-3714}"
    RUNNER_CAPABILITY="''${FLOWSTATE_RUNNER_CAPABILITY:-heavy}"

    echo "=== Flowstate Claude Runner ==="
    echo ""

    # ── Check claude CLI is installed ──
    if ! command -v claude &>/dev/null; then
      echo "ERROR: claude CLI not found."
      echo ""
      echo "Install it: https://docs.anthropic.com/en/docs/claude-cli"
      exit 1
    fi
    echo "claude: $(claude --version 2>&1 || echo 'unknown version')"

    # ── Check authentication ──
    if [ -n "$ANTHROPIC_AUTH_TOKEN" ]; then
      echo "auth:   token from credentials file"
    elif [ -n "$ANTHROPIC_BASE_URL" ]; then
      echo "auth:   custom endpoint ($ANTHROPIC_BASE_URL)"
    elif [ -f "$HOME/.claude/.credentials.json" ]; then
      echo "auth:   OAuth session (~/.claude/.credentials.json)"
    else
      echo "WARNING: No Claude authentication detected."
      echo ""
      echo "Options:"
      echo "  1. Run: claude login            (interactive, creates OAuth session)"
      echo "  2. Run: claude setup-token      (creates long-lived token for headless use)"
      echo "  3. Set FLOWSTATE_ANTHROPIC_AUTH_TOKEN in $RUNNER_CRED_FILE"
    fi

    if [ -n "$ANTHROPIC_MODEL" ]; then
      echo "model:  $ANTHROPIC_MODEL"
    fi
    echo "server: $SERVER_URL"
    echo "health: http://127.0.0.1:$HEALTH_PORT/health"
    echo "cap:    $RUNNER_CAPABILITY"
    echo ""

    # ── Check server is reachable ──
    if curl -sf "$SERVER_URL/health" >/dev/null 2>&1; then
      echo "Server is reachable."
    else
      echo "WARNING: Server at $SERVER_URL is not reachable."
      echo "         The runner will retry on its poll loop."
    fi
    echo ""

    # ── Build args ──
    ARGS=(
      --server-url "$SERVER_URL"
      --agent-backend claude-cli
      --health-port "$HEALTH_PORT"
      --runner-capability "$RUNNER_CAPABILITY"
    )

    if [ -n "$API_KEY" ]; then
      ARGS+=(--api-key "$API_KEY")
    fi

    # Pass through any extra CLI args
    ARGS+=("$@")

    echo "Starting flowstate-runner (claude-cli)..."
    echo ""
    exec cargo run -p flowstate-runner -- "''${ARGS[@]}"
  '';

in {
  inherit runnerClaude;
  all = [ runnerClaude ];
}
