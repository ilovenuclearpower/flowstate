{ pkgs }:
let
  gemini-cli = pkgs.gemini-cli;

  runtimePath = pkgs.lib.makeBinPath [
    gemini-cli
    pkgs.coreutils
    pkgs.bash
    pkgs.curl
  ];

  # Shared launcher script parameterised by model and health port.
  # Gemini-specific config is forwarded via env vars that the runner reads:
  #   FLOWSTATE_GEMINI_API_KEY, FLOWSTATE_GEMINI_MODEL,
  #   FLOWSTATE_GEMINI_GCP_PROJECT, FLOWSTATE_GEMINI_GCP_LOCATION
  mkRunner = { name, defaultModel, defaultHealthPort }: pkgs.writeShellScriptBin name ''
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
    GEMINI_API_KEY="''${FLOWSTATE_GEMINI_API_KEY:-}"
    export FLOWSTATE_GEMINI_MODEL="''${FLOWSTATE_GEMINI_MODEL:-${defaultModel}}"
    GEMINI_GCP_PROJECT="''${FLOWSTATE_GEMINI_GCP_PROJECT:-}"
    GEMINI_GCP_LOCATION="''${FLOWSTATE_GEMINI_GCP_LOCATION:-}"
    HEALTH_PORT="''${FLOWSTATE_HEALTH_PORT:-${toString defaultHealthPort}}"
    RUNNER_CAPABILITY="''${FLOWSTATE_RUNNER_CAPABILITY:-heavy}"

    echo "=== Flowstate Gemini Runner (${name}) ==="
    echo ""

    # ── Check gemini CLI is installed ──
    if ! command -v gemini &>/dev/null; then
      echo "ERROR: gemini CLI not found."
      echo ""
      echo "Install it with:  npm install -g @google/gemini-cli"
      echo "(requires Node.js >= 18)"
      exit 1
    fi
    echo "gemini: $(gemini --version 2>&1 || echo 'unknown version')"

    # ── Check authentication ──
    if [ -n "$GEMINI_API_KEY" ]; then
      echo "auth:   Gemini API key"
    elif [ -n "$GEMINI_GCP_PROJECT" ]; then
      echo "auth:   Vertex AI (project=$GEMINI_GCP_PROJECT)"
    else
      echo "auth:   Google Login / Application Default Credentials"
      echo ""
      echo "Hint: set FLOWSTATE_GEMINI_API_KEY or FLOWSTATE_GEMINI_GCP_PROJECT"
      echo "      in $RUNNER_CRED_FILE for headless authentication."
    fi

    echo "model:  $FLOWSTATE_GEMINI_MODEL"
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
      --agent-backend gemini-cli
      --health-port "$HEALTH_PORT"
      --runner-capability "$RUNNER_CAPABILITY"
    )

    if [ -n "$API_KEY" ]; then
      ARGS+=(--api-key "$API_KEY")
    fi

    # Pass through any extra CLI args
    ARGS+=("$@")

    echo "Starting flowstate-runner (gemini-cli / $FLOWSTATE_GEMINI_MODEL)..."
    echo ""
    exec cargo run -p flowstate-runner -- "''${ARGS[@]}"
  '';

  runnerGeminiPro = mkRunner {
    name = "runner-gemini-pro";
    defaultModel = "gemini-3.1-pro-preview";
    defaultHealthPort = 3712;
  };

  runnerGeminiFlash = mkRunner {
    name = "runner-gemini-flash";
    defaultModel = "gemini-3-flash-preview";
    defaultHealthPort = 3713;
  };

in {
  inherit runnerGeminiPro runnerGeminiFlash;
  all = [ runnerGeminiPro runnerGeminiFlash ];
}
