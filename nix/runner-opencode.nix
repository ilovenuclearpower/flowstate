{ pkgs }:
let
  runtimePath = pkgs.lib.makeBinPath [
    pkgs.coreutils
    pkgs.bash
    pkgs.curl
  ];

  # Shared launcher script parameterised by model and health port.
  # OpenCode-specific config is forwarded via env vars that the runner reads:
  #   FLOWSTATE_OPENCODE_API_KEY, FLOWSTATE_OPENCODE_MODEL,
  #   FLOWSTATE_OPENCODE_PROVIDER, FLOWSTATE_OPENCODE_BASE_URL
  mkRunner = { name, defaultModel, defaultProvider, defaultHealthPort }: pkgs.writeShellScriptBin name ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    # ── Load runner credentials if available ──
    RUNNER_CRED_FILE="''${XDG_DATA_HOME:-$HOME/.local/share}/flowstate/runner/credentials/runner.env"
    if [ -f "$RUNNER_CRED_FILE" ]; then
      set -a
      source "$RUNNER_CRED_FILE"
      set +a
    fi

    # ── Defaults (env vars from cred file take precedence, CLI overrides both) ──
    SERVER_URL="''${FLOWSTATE_SERVER_URL:-http://127.0.0.1:3710}"
    API_KEY="''${FLOWSTATE_API_KEY:-}"
    OPENCODE_API_KEY="''${FLOWSTATE_OPENCODE_API_KEY:-}"
    export FLOWSTATE_OPENCODE_PROVIDER="''${FLOWSTATE_OPENCODE_PROVIDER:-${defaultProvider}}"
    export FLOWSTATE_OPENCODE_MODEL="''${FLOWSTATE_OPENCODE_MODEL:-${defaultModel}}"
    OPENCODE_BASE_URL="''${FLOWSTATE_OPENCODE_BASE_URL:-}"
    HEALTH_PORT="''${FLOWSTATE_HEALTH_PORT:-${toString defaultHealthPort}}"
    RUNNER_CAPABILITY="''${FLOWSTATE_RUNNER_CAPABILITY:-heavy}"

    echo "=== Flowstate OpenCode Runner (${name}) ==="
    echo ""

    # ── Check opencode CLI is installed ──
    if ! command -v opencode &>/dev/null; then
      echo "ERROR: opencode CLI not found."
      echo ""
      echo "Install it: https://opencode.ai/docs/"
      exit 1
    fi
    echo "opencode: $(opencode version 2>&1 || echo 'unknown version')"

    # ── Check authentication ──
    if [ -n "$OPENCODE_API_KEY" ]; then
      echo "auth:   API key for $FLOWSTATE_OPENCODE_PROVIDER"
    else
      echo "auth:   using opencode default config"
      echo ""
      echo "Hint: set FLOWSTATE_OPENCODE_API_KEY in $RUNNER_CRED_FILE"
      echo "      for headless authentication."
    fi

    echo "provider: $FLOWSTATE_OPENCODE_PROVIDER"
    echo "model:    $FLOWSTATE_OPENCODE_MODEL"
    if [ -n "$OPENCODE_BASE_URL" ]; then
      echo "baseURL:  $OPENCODE_BASE_URL"
    fi
    echo "server:   $SERVER_URL"
    echo "health:   http://127.0.0.1:$HEALTH_PORT/health"
    echo "cap:      $RUNNER_CAPABILITY"
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
      --agent-backend opencode
      --health-port "$HEALTH_PORT"
      --runner-capability "$RUNNER_CAPABILITY"
    )

    if [ -n "$API_KEY" ]; then
      ARGS+=(--api-key "$API_KEY")
    fi

    # Pass through any extra CLI args
    ARGS+=("$@")

    echo "Starting flowstate-runner (opencode / $FLOWSTATE_OPENCODE_MODEL)..."
    echo ""
    exec cargo run -p flowstate-runner -- "''${ARGS[@]}"
  '';

  runnerOpencode = mkRunner {
    name = "runner-opencode";
    defaultProvider = "anthropic";
    defaultModel = "anthropic/claude-sonnet-4-5";
    defaultHealthPort = 3715;
  };

in {
  inherit runnerOpencode;
  all = [ runnerOpencode ];
}
