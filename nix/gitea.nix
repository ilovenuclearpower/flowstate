{ pkgs }:
let
  runtimePath = pkgs.lib.makeBinPath [
    pkgs.gitea
    pkgs.curl
    pkgs.jq
    pkgs.coreutils
    pkgs.iproute2
    pkgs.gnugrep
    pkgs.gawk
    pkgs.bash
    pkgs.git
  ];

  giteaTestStart = pkgs.writeShellScriptBin "gitea-test-start" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    MARKER="/tmp/flowstate-gitea-test-current"
    HTTP_PORT=3920

    # Check if a test instance is already running
    if [ -f "$MARKER" ]; then
      EXISTING_DIR=$(cat "$MARKER")
      if [ -f "$EXISTING_DIR/pid" ]; then
        EXISTING_PID=$(cat "$EXISTING_DIR/pid")
        if kill -0 "$EXISTING_PID" 2>/dev/null; then
          echo "A Gitea test instance is already running (PID $EXISTING_PID)."
          echo "Run gitea-test-stop first."
          exit 1
        fi
      fi
      # Stale marker, clean up
      rm -f "$MARKER"
    fi

    # Check for port conflicts
    if ss -tln 2>/dev/null | grep -q ":$HTTP_PORT "; then
      echo "ERROR: Port $HTTP_PORT is already in use."
      ss -tlnp 2>/dev/null | grep ":$HTTP_PORT " || true
      exit 1
    fi

    # Create temp directory
    TMPDIR=$(mktemp -d /tmp/flowstate-gitea-test-XXXXXXXX)
    echo "$TMPDIR" > "$MARKER"

    mkdir -p "$TMPDIR/data"
    mkdir -p "$TMPDIR/custom/conf"
    mkdir -p "$TMPDIR/log"
    mkdir -p "$TMPDIR/credentials"

    # Write minimal app.ini
    cat > "$TMPDIR/custom/conf/app.ini" <<EOF
    APP_NAME = Flowstate Test
    RUN_MODE = prod

    [database]
    DB_TYPE = sqlite3
    PATH = $TMPDIR/data/gitea.db

    [server]
    HTTP_PORT = $HTTP_PORT
    ROOT_URL = http://127.0.0.1:$HTTP_PORT/
    DISABLE_SSH = true
    OFFLINE_MODE = true
    LFS_START_SERVER = false

    [service]
    DISABLE_REGISTRATION = true
    REQUIRE_SIGNIN_VIEW = false
    DEFAULT_ALLOW_CREATE_ORGANIZATION = false

    [security]
    INSTALL_LOCK = true
    SECRET_KEY = test-secret-key-do-not-use-in-production
    INTERNAL_TOKEN = eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJuYmYiOjE3MDAwMDAwMDB9.abc123

    [log]
    ROOT_PATH = $TMPDIR/log
    MODE = file
    LEVEL = Warn

    [repository]
    ROOT = $TMPDIR/data/repositories

    [indexer]
    ISSUE_INDEXER_PATH = $TMPDIR/data/indexers/issues.bleve
    REPO_INDEXER_ENABLED = false
    EOF

    # Start Gitea server
    echo "Starting Gitea test instance on port $HTTP_PORT..."
    gitea web --config "$TMPDIR/custom/conf/app.ini" --work-path "$TMPDIR" &>"$TMPDIR/log/gitea-stdout.log" &
    GITEA_PID=$!
    echo "$GITEA_PID" > "$TMPDIR/pid"
    echo "Gitea started with PID $GITEA_PID"

    # Wait for health endpoint
    echo "Waiting for Gitea to become ready..."
    ATTEMPTS=0
    MAX_ATTEMPTS=60
    while [ $ATTEMPTS -lt $MAX_ATTEMPTS ]; do
      if curl -sf "http://127.0.0.1:$HTTP_PORT/api/v1/version" >/dev/null 2>&1; then
        echo "Gitea is ready."
        break
      fi
      if ! kill -0 "$GITEA_PID" 2>/dev/null; then
        echo "ERROR: Gitea process exited unexpectedly."
        echo "Last 20 lines of log:"
        tail -20 "$TMPDIR/log/gitea-stdout.log" 2>/dev/null || true
        rm -rf "$TMPDIR"
        rm -f "$MARKER"
        exit 1
      fi
      ATTEMPTS=$((ATTEMPTS + 1))
      sleep 1
    done

    if [ $ATTEMPTS -ge $MAX_ATTEMPTS ]; then
      echo "ERROR: Gitea failed to become ready within $MAX_ATTEMPTS seconds."
      echo "Last 20 lines of log:"
      tail -20 "$TMPDIR/log/gitea-stdout.log" 2>/dev/null || true
      kill "$GITEA_PID" 2>/dev/null || true
      rm -rf "$TMPDIR"
      rm -f "$MARKER"
      exit 1
    fi

    # Create admin user
    echo "Creating admin user..."
    gitea admin user create \
      --config "$TMPDIR/custom/conf/app.ini" \
      --work-path "$TMPDIR" \
      --username testuser \
      --password testpass \
      --email test@test.local \
      --admin \
      --must-change-password=false

    # Create API token via HTTP API
    echo "Creating API token..."
    TOKEN_RESPONSE=$(curl -sf \
      -X POST \
      -u "testuser:testpass" \
      -H "Content-Type: application/json" \
      -d '{"name":"test-token","scopes":["all"]}' \
      "http://127.0.0.1:$HTTP_PORT/api/v1/users/testuser/tokens")

    API_TOKEN=$(echo "$TOKEN_RESPONSE" | jq -r '.sha1 // .token // empty')
    if [ -z "$API_TOKEN" ]; then
      echo "ERROR: Failed to create API token."
      echo "Response: $TOKEN_RESPONSE"
      kill "$GITEA_PID" 2>/dev/null || true
      rm -rf "$TMPDIR"
      rm -f "$MARKER"
      exit 1
    fi
    echo "API token created."

    # Create test repo via API (auto_init to have a default branch)
    echo "Creating test repository..."
    REPO_RESPONSE=$(curl -sf \
      -X POST \
      -H "Authorization: token $API_TOKEN" \
      -H "Content-Type: application/json" \
      -d '{"name":"test-repo","auto_init":true,"default_branch":"main"}' \
      "http://127.0.0.1:$HTTP_PORT/api/v1/user/repos")

    REPO_NAME=$(echo "$REPO_RESPONSE" | jq -r '.name // empty')
    if [ -z "$REPO_NAME" ]; then
      echo "ERROR: Failed to create test repository."
      echo "Response: $REPO_RESPONSE"
      kill "$GITEA_PID" 2>/dev/null || true
      rm -rf "$TMPDIR"
      rm -f "$MARKER"
      exit 1
    fi
    echo "Test repository '$REPO_NAME' created."

    # Write credentials file
    CRED_FILE="$TMPDIR/credentials/gitea.env"
    cat > "$CRED_FILE" <<EOF
    GITEA_TEST_URL=http://127.0.0.1:$HTTP_PORT
    GITEA_TEST_TOKEN=$API_TOKEN
    GITEA_TEST_USER=testuser
    GITEA_TEST_REPO=test-repo
    EOF
    chmod 600 "$CRED_FILE"

    echo ""
    echo "=== Gitea Test Instance ==="
    echo "HTTP:         http://127.0.0.1:$HTTP_PORT"
    echo "User:         testuser"
    echo "Repo:         test-repo"
    echo "Credentials:  $CRED_FILE"
    echo "Test dir:     $TMPDIR"
    echo ""
    echo "To load credentials: eval \$(gitea-test-info --env)"
    echo "GITEA_TEST_DIR=$TMPDIR"
  '';

  giteaTestStop = pkgs.writeShellScriptBin "gitea-test-stop" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    MARKER="/tmp/flowstate-gitea-test-current"

    if [ ! -f "$MARKER" ]; then
      echo "No Gitea test instance is running."
      exit 0
    fi

    TMPDIR=$(cat "$MARKER")

    if [ ! -d "$TMPDIR" ]; then
      echo "Test directory $TMPDIR does not exist. Cleaning up marker."
      rm -f "$MARKER"
      exit 0
    fi

    if [ -f "$TMPDIR/pid" ]; then
      PID=$(cat "$TMPDIR/pid")
      if kill -0 "$PID" 2>/dev/null; then
        echo "Stopping Gitea test instance (PID $PID)..."
        kill "$PID"

        WAIT=0
        while [ $WAIT -lt 10 ]; do
          if ! kill -0 "$PID" 2>/dev/null; then
            break
          fi
          WAIT=$((WAIT + 1))
          sleep 1
        done

        if kill -0 "$PID" 2>/dev/null; then
          echo "Gitea did not stop gracefully, sending SIGKILL..."
          kill -9 "$PID" 2>/dev/null || true
        fi
      fi
    fi

    echo "Removing test directory $TMPDIR..."
    rm -rf "$TMPDIR"
    rm -f "$MARKER"
    echo "Gitea test instance stopped and data wiped."
  '';

  giteaTestStatus = pkgs.writeShellScriptBin "gitea-test-status" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    MARKER="/tmp/flowstate-gitea-test-current"
    HTTP_PORT=3920

    if [ -f "$MARKER" ]; then
      TMPDIR=$(cat "$MARKER")
      if [ -f "$TMPDIR/pid" ]; then
        PID=$(cat "$TMPDIR/pid")
        if kill -0 "$PID" 2>/dev/null; then
          HEALTH=$(curl -sf "http://127.0.0.1:$HTTP_PORT/api/v1/version" 2>/dev/null || echo "unreachable")
          echo "Gitea test instance: running (PID $PID)"
          echo "Health: $HEALTH"
          echo "Test dir: $TMPDIR"
          exit 0
        fi
      fi
    fi

    echo "Gitea test instance: stopped"
    exit 1
  '';

  giteaTestInfo = pkgs.writeShellScriptBin "gitea-test-info" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    MARKER="/tmp/flowstate-gitea-test-current"

    if [ ! -f "$MARKER" ]; then
      echo "ERROR: No test instance is running. Run gitea-test-start first."
      exit 1
    fi

    TMPDIR=$(cat "$MARKER")
    CRED_FILE="$TMPDIR/credentials/gitea.env"

    if [ ! -f "$CRED_FILE" ]; then
      echo "ERROR: No credentials found. The test instance may not be fully bootstrapped."
      exit 1
    fi

    # Source the credentials
    set -a
    source "$CRED_FILE"
    set +a

    if [ "''${1:-}" = "--env" ]; then
      while IFS= read -r line; do
        [ -n "$line" ] && echo "export $line"
      done < "$CRED_FILE"
    else
      echo "=== Gitea Test Instance ==="
      echo "URL:          $GITEA_TEST_URL"
      echo "User:         $GITEA_TEST_USER"
      echo "Repo:         $GITEA_TEST_REPO"
      echo "Token:        $GITEA_TEST_TOKEN"
      echo "Test dir:     $TMPDIR"
      echo ""
      echo "To load into shell: eval \$(gitea-test-info --env)"
    fi
  '';

in {
  inherit giteaTestStart giteaTestStop giteaTestStatus giteaTestInfo;

  all = [
    giteaTestStart giteaTestStop giteaTestStatus giteaTestInfo
  ];
}
