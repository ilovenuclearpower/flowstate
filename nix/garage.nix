{ pkgs }:
let
  garage = pkgs.garage;

  runtimePath = pkgs.lib.makeBinPath [
    garage
    pkgs.coreutils
    pkgs.openssl
    pkgs.curl
    pkgs.util-linux # flock
    pkgs.iproute2   # ss
    pkgs.gnugrep
    pkgs.gawk
    pkgs.bash
  ];

  garageDevStart = pkgs.writeShellScriptBin "garage-dev-start" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    DATA_DIR="''${XDG_DATA_HOME:-$HOME/.local/share}/flowstate/garage/dev"
    CONFIG="$DATA_DIR/garage.toml"
    PID_FILE="$DATA_DIR/garage.pid"
    LOCK_FILE="$DATA_DIR/start.lock"
    CRED_FILE="$DATA_DIR/credentials/s3.env"

    S3_PORT=3900
    RPC_PORT=3901
    WEB_PORT=3902
    ADMIN_PORT=3903

    # Ensure base directory exists for lock file
    mkdir -p "$DATA_DIR"

    # Quick check before locking: is it already running?
    if [ -f "$PID_FILE" ]; then
      OLD_PID=$(cat "$PID_FILE")
      if kill -0 "$OLD_PID" 2>/dev/null; then
        echo "Garage dev instance is already running (PID $OLD_PID)."
        if [ -f "$CRED_FILE" ]; then
          echo ""
          echo "S3 endpoint: http://127.0.0.1:$S3_PORT"
          echo "Credentials: $CRED_FILE"
          echo ""
          echo "To load credentials: eval \$(garage-dev-info --env)"
        fi
        exit 0
      fi
    fi

    # Acquire exclusive lock for startup
    exec 9>"$LOCK_FILE"
    if ! flock -n 9; then
      echo "Another garage-dev-start is already running. Waiting..."
      flock 9
    fi

    # Re-check after acquiring lock (another process may have started it)
    if [ -f "$PID_FILE" ]; then
      OLD_PID=$(cat "$PID_FILE")
      if kill -0 "$OLD_PID" 2>/dev/null; then
        echo "Garage dev instance is already running (PID $OLD_PID)."
        exec 9>&-
        if [ -f "$CRED_FILE" ]; then
          echo ""
          echo "S3 endpoint: http://127.0.0.1:$S3_PORT"
          echo "Credentials: $CRED_FILE"
          echo ""
          echo "To load credentials: eval \$(garage-dev-info --env)"
        fi
        exit 0
      else
        echo "Removing stale PID file (PID $OLD_PID is not running)."
        rm -f "$PID_FILE"
      fi
    fi

    # Check for port conflicts
    if ss -tln 2>/dev/null | grep -q ":$S3_PORT "; then
      echo "ERROR: Port $S3_PORT is already in use."
      echo "Another process is binding to the S3 API port."
      ss -tlnp 2>/dev/null | grep ":$S3_PORT " || true
      exec 9>&-
      exit 1
    fi

    # Create directory structure
    mkdir -p "$DATA_DIR/meta"
    mkdir -p "$DATA_DIR/data"
    mkdir -p "$DATA_DIR/secrets"
    mkdir -p "$DATA_DIR/credentials"

    # First run: generate secrets and config
    if [ ! -f "$CONFIG" ]; then
      echo "First run: generating secrets and configuration..."

      # Generate secrets
      openssl rand -hex 32 > "$DATA_DIR/secrets/rpc-secret"
      chmod 600 "$DATA_DIR/secrets/rpc-secret"

      openssl rand -hex 32 > "$DATA_DIR/secrets/admin-token"
      chmod 600 "$DATA_DIR/secrets/admin-token"

      # Write garage.toml
      cat > "$CONFIG" <<EOF
    metadata_dir = "$DATA_DIR/meta"
    db_engine = "lmdb"
    replication_factor = 1

    rpc_bind_addr = "127.0.0.1:$RPC_PORT"
    rpc_public_addr = "127.0.0.1:$RPC_PORT"
    rpc_secret_file = "$DATA_DIR/secrets/rpc-secret"

    [[data_dir]]
    path = "$DATA_DIR/data"
    capacity = "1G"

    [s3_api]
    s3_region = "garage"
    api_bind_addr = "127.0.0.1:$S3_PORT"
    root_domain = ".s3.garage.localhost"

    [s3_web]
    bind_addr = "127.0.0.1:$WEB_PORT"
    root_domain = ".web.garage.localhost"

    [admin]
    api_bind_addr = "127.0.0.1:$ADMIN_PORT"
    admin_token_file = "$DATA_DIR/secrets/admin-token"
    EOF

      echo "Configuration written to $CONFIG"
    fi

    # Release the startup lock before launching the server
    exec 9>&-

    # Start garage server in background
    echo "Starting Garage dev instance..."
    garage -c "$CONFIG" server &>"$DATA_DIR/garage.log" &
    GARAGE_PID=$!
    echo "$GARAGE_PID" > "$PID_FILE"
    echo "Garage started with PID $GARAGE_PID"

    # Wait for health endpoint
    echo "Waiting for Garage to become ready..."
    ATTEMPTS=0
    MAX_ATTEMPTS=30
    while [ $ATTEMPTS -lt $MAX_ATTEMPTS ]; do
      if curl -sf "http://127.0.0.1:$ADMIN_PORT/health" >/dev/null 2>&1; then
        echo "Garage is ready."
        break
      fi
      # Check if process is still alive
      if ! kill -0 "$GARAGE_PID" 2>/dev/null; then
        echo "ERROR: Garage process exited unexpectedly."
        echo "Last 20 lines of log:"
        tail -20 "$DATA_DIR/garage.log" 2>/dev/null || true
        rm -f "$PID_FILE"
        exit 1
      fi
      ATTEMPTS=$((ATTEMPTS + 1))
      sleep 1
    done

    if [ $ATTEMPTS -ge $MAX_ATTEMPTS ]; then
      echo "ERROR: Garage failed to become ready within $MAX_ATTEMPTS seconds."
      echo "Last 20 lines of log:"
      tail -20 "$DATA_DIR/garage.log" 2>/dev/null || true
      kill "$GARAGE_PID" 2>/dev/null || true
      rm -f "$PID_FILE"
      exit 1
    fi

    # First run bootstrap: layout, bucket, key
    if [ ! -f "$CRED_FILE" ]; then
      echo "First run: bootstrapping cluster..."

      # Get node ID
      NODE_ID=$(garage -c "$CONFIG" node id 2>/dev/null | head -1 | cut -d@ -f1)
      if [ -z "$NODE_ID" ]; then
        echo "ERROR: Failed to get node ID."
        rm -f "$CRED_FILE"
        kill "$GARAGE_PID" 2>/dev/null || true
        rm -f "$PID_FILE"
        exit 1
      fi
      echo "Node ID: $NODE_ID"

      # Assign layout (skip if already applied)
      LAYOUT_STATUS=$(garage -c "$CONFIG" layout show 2>&1 || true)
      if echo "$LAYOUT_STATUS" | grep -q "No nodes"; then
        if ! garage -c "$CONFIG" layout assign -z dc1 -c 1G "$NODE_ID"; then
          echo "ERROR: Failed to assign layout."
          rm -f "$CRED_FILE"
          kill "$GARAGE_PID" 2>/dev/null || true
          rm -f "$PID_FILE"
          exit 1
        fi

        # Apply layout (only needed for fresh clusters)
        CURRENT_VERSION=$(garage -c "$CONFIG" layout show 2>&1 | grep "Current cluster layout version:" | awk '{print $NF}' || echo "0")
        NEXT_VERSION=$((CURRENT_VERSION + 1))
        if ! garage -c "$CONFIG" layout apply --version "$NEXT_VERSION"; then
          echo "ERROR: Failed to apply layout."
          rm -f "$CRED_FILE"
          kill "$GARAGE_PID" 2>/dev/null || true
          rm -f "$PID_FILE"
          exit 1
        fi
      else
        echo "Layout already configured, skipping."
      fi

      # Create bucket
      if ! garage -c "$CONFIG" bucket create flowstate; then
        echo "WARNING: Bucket creation returned an error (may already exist)."
      fi

      # Create key
      if ! garage -c "$CONFIG" key create flowstate-dev; then
        echo "WARNING: Key creation returned an error (may already exist)."
      fi

      # Allow key access to bucket
      if ! garage -c "$CONFIG" bucket allow --read --write --owner flowstate --key flowstate-dev; then
        echo "ERROR: Failed to grant bucket permissions."
        rm -f "$CRED_FILE"
        kill "$GARAGE_PID" 2>/dev/null || true
        rm -f "$PID_FILE"
        exit 1
      fi

      # Extract credentials from key info
      KEY_INFO=$(garage -c "$CONFIG" key info --show-secret flowstate-dev 2>/dev/null)
      KEY_ID=$(echo "$KEY_INFO" | grep "Key ID:" | awk '{print $3}')
      KEY_SECRET=$(echo "$KEY_INFO" | grep "Secret key:" | awk '{print $3}')

      if [ -z "$KEY_ID" ] || [ -z "$KEY_SECRET" ]; then
        echo "ERROR: Failed to extract key credentials."
        echo "Key info output:"
        echo "$KEY_INFO"
        rm -f "$CRED_FILE"
        kill "$GARAGE_PID" 2>/dev/null || true
        rm -f "$PID_FILE"
        exit 1
      fi

      # Write credentials file
      cat > "$CRED_FILE" <<EOF
    AWS_ACCESS_KEY_ID=$KEY_ID
    AWS_SECRET_ACCESS_KEY=$KEY_SECRET
    AWS_ENDPOINT_URL=http://127.0.0.1:$S3_PORT
    AWS_REGION=garage
    GARAGE_BUCKET=flowstate
    EOF
      chmod 600 "$CRED_FILE"

      echo "Bootstrap complete. Credentials written to $CRED_FILE"
    fi

    echo ""
    echo "=== Garage Dev Instance ==="
    echo "S3 endpoint:  http://127.0.0.1:$S3_PORT"
    echo "Admin API:    http://127.0.0.1:$ADMIN_PORT"
    echo "Bucket:       flowstate"
    echo "Credentials:  $CRED_FILE"
    echo ""
    echo "To load credentials: eval \$(garage-dev-info --env)"
  '';

  garageDevStop = pkgs.writeShellScriptBin "garage-dev-stop" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    DATA_DIR="''${XDG_DATA_HOME:-$HOME/.local/share}/flowstate/garage/dev"
    PID_FILE="$DATA_DIR/garage.pid"

    if [ ! -f "$PID_FILE" ]; then
      echo "Garage dev instance is not running."
      exit 0
    fi

    PID=$(cat "$PID_FILE")

    if ! kill -0 "$PID" 2>/dev/null; then
      echo "Garage dev instance is not running (stale PID file)."
      rm -f "$PID_FILE"
      exit 0
    fi

    echo "Stopping Garage dev instance (PID $PID)..."
    kill "$PID"

    # Wait for graceful shutdown
    WAIT=0
    while [ $WAIT -lt 10 ]; do
      if ! kill -0 "$PID" 2>/dev/null; then
        echo "Garage dev instance stopped."
        rm -f "$PID_FILE"
        exit 0
      fi
      WAIT=$((WAIT + 1))
      sleep 1
    done

    # Force kill
    echo "Garage did not stop gracefully, sending SIGKILL..."
    kill -9 "$PID" 2>/dev/null || true
    rm -f "$PID_FILE"
    echo "Garage dev instance stopped (forced)."
  '';

  garageDevStatus = pkgs.writeShellScriptBin "garage-dev-status" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    DATA_DIR="''${XDG_DATA_HOME:-$HOME/.local/share}/flowstate/garage/dev"
    PID_FILE="$DATA_DIR/garage.pid"
    ADMIN_PORT=3903

    if [ -f "$PID_FILE" ]; then
      PID=$(cat "$PID_FILE")
      if kill -0 "$PID" 2>/dev/null; then
        HEALTH=$(curl -sf "http://127.0.0.1:$ADMIN_PORT/health" 2>/dev/null || echo "unreachable")
        echo "Garage dev instance: running (PID $PID)"
        echo "Health: $HEALTH"
        exit 0
      else
        echo "Garage dev instance: stopped (stale PID file)"
        exit 1
      fi
    fi

    echo "Garage dev instance: stopped"
    exit 1
  '';

  garageDevInfo = pkgs.writeShellScriptBin "garage-dev-info" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    DATA_DIR="''${XDG_DATA_HOME:-$HOME/.local/share}/flowstate/garage/dev"
    CRED_FILE="$DATA_DIR/credentials/s3.env"

    if [ ! -f "$CRED_FILE" ]; then
      echo "ERROR: No credentials found. Run garage-dev-start first."
      exit 1
    fi

    # Source the credentials
    set -a
    source "$CRED_FILE"
    set +a

    if [ "''${1:-}" = "--env" ]; then
      # Print in export format for eval
      while IFS= read -r line; do
        [ -n "$line" ] && echo "export $line"
      done < "$CRED_FILE"
    else
      echo "=== Garage Dev Instance ==="
      echo "S3 endpoint:      $AWS_ENDPOINT_URL"
      echo "Region:           $AWS_REGION"
      echo "Bucket:           $GARAGE_BUCKET"
      echo "Access Key ID:    $AWS_ACCESS_KEY_ID"
      echo "Secret Key:       $AWS_SECRET_ACCESS_KEY"
      echo ""
      echo "To load into shell: eval \$(garage-dev-info --env)"
      echo ""
      echo "Example usage:"
      echo "  aws s3 ls s3://flowstate/ --endpoint-url $AWS_ENDPOINT_URL"
    fi
  '';

  garageTestStart = pkgs.writeShellScriptBin "garage-test-start" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    MARKER="/tmp/flowstate-garage-test-current"

    S3_PORT=3910
    RPC_PORT=3911
    WEB_PORT=3912
    ADMIN_PORT=3913

    # Check if a test instance is already running
    if [ -f "$MARKER" ]; then
      EXISTING_DIR=$(cat "$MARKER")
      if [ -f "$EXISTING_DIR/pid" ]; then
        EXISTING_PID=$(cat "$EXISTING_DIR/pid")
        if kill -0 "$EXISTING_PID" 2>/dev/null; then
          echo "A Garage test instance is already running (PID $EXISTING_PID)."
          echo "Run garage-test-stop first."
          exit 1
        fi
      fi
      # Stale marker, clean up
      rm -f "$MARKER"
    fi

    # Check for port conflicts
    if ss -tln 2>/dev/null | grep -q ":$S3_PORT "; then
      echo "ERROR: Port $S3_PORT is already in use."
      ss -tlnp 2>/dev/null | grep ":$S3_PORT " || true
      exit 1
    fi

    # Create temp directory
    TMPDIR=$(mktemp -d /tmp/flowstate-garage-test-XXXXXXXX)
    echo "$TMPDIR" > "$MARKER"

    mkdir -p "$TMPDIR/meta"
    mkdir -p "$TMPDIR/data"
    mkdir -p "$TMPDIR/credentials"

    CONFIG="$TMPDIR/garage.toml"

    # Write config with deterministic secrets
    cat > "$CONFIG" <<EOF
    metadata_dir = "$TMPDIR/meta"
    db_engine = "sqlite"
    replication_factor = 1

    rpc_bind_addr = "127.0.0.1:$RPC_PORT"
    rpc_public_addr = "127.0.0.1:$RPC_PORT"
    rpc_secret = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

    [[data_dir]]
    path = "$TMPDIR/data"
    capacity = "1G"

    [s3_api]
    s3_region = "garage"
    api_bind_addr = "127.0.0.1:$S3_PORT"
    root_domain = ".s3.garage.localhost"

    [s3_web]
    bind_addr = "127.0.0.1:$WEB_PORT"
    root_domain = ".web.garage.localhost"

    [admin]
    api_bind_addr = "127.0.0.1:$ADMIN_PORT"
    admin_token = "test-admin-token"
    EOF

    # Start garage server
    echo "Starting Garage test instance..."
    garage -c "$CONFIG" server &>"$TMPDIR/garage.log" &
    GARAGE_PID=$!
    echo "$GARAGE_PID" > "$TMPDIR/pid"
    echo "Garage started with PID $GARAGE_PID"

    # Wait for health endpoint
    echo "Waiting for Garage to become ready..."
    ATTEMPTS=0
    MAX_ATTEMPTS=30
    while [ $ATTEMPTS -lt $MAX_ATTEMPTS ]; do
      if curl -sf "http://127.0.0.1:$ADMIN_PORT/health" >/dev/null 2>&1; then
        echo "Garage is ready."
        break
      fi
      if ! kill -0 "$GARAGE_PID" 2>/dev/null; then
        echo "ERROR: Garage process exited unexpectedly."
        echo "Last 20 lines of log:"
        tail -20 "$TMPDIR/garage.log" 2>/dev/null || true
        rm -rf "$TMPDIR"
        rm -f "$MARKER"
        exit 1
      fi
      ATTEMPTS=$((ATTEMPTS + 1))
      sleep 1
    done

    if [ $ATTEMPTS -ge $MAX_ATTEMPTS ]; then
      echo "ERROR: Garage failed to become ready within $MAX_ATTEMPTS seconds."
      echo "Last 20 lines of log:"
      tail -20 "$TMPDIR/garage.log" 2>/dev/null || true
      kill "$GARAGE_PID" 2>/dev/null || true
      rm -rf "$TMPDIR"
      rm -f "$MARKER"
      exit 1
    fi

    # Bootstrap: layout, bucket, key
    echo "Bootstrapping test cluster..."

    NODE_ID=$(garage -c "$CONFIG" node id 2>/dev/null | head -1 | cut -d@ -f1)
    if [ -z "$NODE_ID" ]; then
      echo "ERROR: Failed to get node ID."
      kill "$GARAGE_PID" 2>/dev/null || true
      rm -rf "$TMPDIR"
      rm -f "$MARKER"
      exit 1
    fi

    garage -c "$CONFIG" layout assign -z dc1 -c 1G "$NODE_ID"
    garage -c "$CONFIG" layout apply --version 1
    garage -c "$CONFIG" bucket create flowstate-test
    garage -c "$CONFIG" key create flowstate-test
    garage -c "$CONFIG" bucket allow --read --write --owner flowstate-test --key flowstate-test

    # Extract credentials
    KEY_INFO=$(garage -c "$CONFIG" key info flowstate-test 2>/dev/null)
    KEY_ID=$(echo "$KEY_INFO" | grep "Key ID:" | awk '{print $3}')
    KEY_SECRET=$(echo "$KEY_INFO" | grep "Secret key:" | awk '{print $3}')

    if [ -z "$KEY_ID" ] || [ -z "$KEY_SECRET" ]; then
      echo "ERROR: Failed to extract key credentials."
      echo "Key info output:"
      echo "$KEY_INFO"
      kill "$GARAGE_PID" 2>/dev/null || true
      rm -rf "$TMPDIR"
      rm -f "$MARKER"
      exit 1
    fi

    # Write credentials
    CRED_FILE="$TMPDIR/credentials/s3.env"
    cat > "$CRED_FILE" <<EOF
    AWS_ACCESS_KEY_ID=$KEY_ID
    AWS_SECRET_ACCESS_KEY=$KEY_SECRET
    AWS_ENDPOINT_URL=http://127.0.0.1:$S3_PORT
    AWS_REGION=garage
    GARAGE_BUCKET=flowstate-test
    EOF

    echo ""
    echo "=== Garage Test Instance ==="
    echo "S3 endpoint:      http://127.0.0.1:$S3_PORT"
    echo "Admin API:        http://127.0.0.1:$ADMIN_PORT"
    echo "Bucket:           flowstate-test"
    echo "Credentials:      $CRED_FILE"
    echo "Test dir:         $TMPDIR"
    echo ""
    echo "To load credentials: eval \$(garage-test-info --env)"
    echo "GARAGE_TEST_DIR=$TMPDIR"
  '';

  garageTestStop = pkgs.writeShellScriptBin "garage-test-stop" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    MARKER="/tmp/flowstate-garage-test-current"

    if [ ! -f "$MARKER" ]; then
      echo "No Garage test instance is running."
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
        echo "Stopping Garage test instance (PID $PID)..."
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
          echo "Garage did not stop gracefully, sending SIGKILL..."
          kill -9 "$PID" 2>/dev/null || true
        fi
      fi
    fi

    echo "Removing test directory $TMPDIR..."
    rm -rf "$TMPDIR"
    rm -f "$MARKER"
    echo "Garage test instance stopped and data wiped."
  '';

  garageTestStatus = pkgs.writeShellScriptBin "garage-test-status" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    MARKER="/tmp/flowstate-garage-test-current"
    ADMIN_PORT=3913

    if [ -f "$MARKER" ]; then
      TMPDIR=$(cat "$MARKER")
      if [ -f "$TMPDIR/pid" ]; then
        PID=$(cat "$TMPDIR/pid")
        if kill -0 "$PID" 2>/dev/null; then
          HEALTH=$(curl -sf "http://127.0.0.1:$ADMIN_PORT/health" 2>/dev/null || echo "unreachable")
          echo "Garage test instance: running (PID $PID)"
          echo "Health: $HEALTH"
          echo "Test dir: $TMPDIR"
          exit 0
        fi
      fi
    fi

    echo "Garage test instance: stopped"
    exit 1
  '';

  garageTestInfo = pkgs.writeShellScriptBin "garage-test-info" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    MARKER="/tmp/flowstate-garage-test-current"

    if [ ! -f "$MARKER" ]; then
      echo "ERROR: No test instance is running. Run garage-test-start first."
      exit 1
    fi

    TMPDIR=$(cat "$MARKER")
    CRED_FILE="$TMPDIR/credentials/s3.env"

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
      echo "=== Garage Test Instance ==="
      echo "S3 endpoint:      $AWS_ENDPOINT_URL"
      echo "Region:           $AWS_REGION"
      echo "Bucket:           $GARAGE_BUCKET"
      echo "Access Key ID:    $AWS_ACCESS_KEY_ID"
      echo "Secret Key:       $AWS_SECRET_ACCESS_KEY"
      echo "Test dir:         $TMPDIR"
      echo ""
      echo "To load into shell: eval \$(garage-test-info --env)"
    fi
  '';

in {
  inherit garageDevStart garageDevStop garageDevStatus garageDevInfo;
  inherit garageTestStart garageTestStop garageTestStatus garageTestInfo;

  all = [
    garageDevStart garageDevStop garageDevStatus garageDevInfo
    garageTestStart garageTestStop garageTestStatus garageTestInfo
    garage
  ];
}
