{ pkgs }:
let
  postgresql = pkgs.postgresql_16;

  runtimePath = pkgs.lib.makeBinPath [
    postgresql
    pkgs.coreutils
    pkgs.util-linux # flock
    pkgs.iproute2   # ss
    pkgs.gnugrep
    pkgs.gawk
    pkgs.bash
  ];

  pgDevStart = pkgs.writeShellScriptBin "pg-dev-start" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    DATA_DIR="''${XDG_DATA_HOME:-$HOME/.local/share}/flowstate/postgres/dev"
    PG_DATA="$DATA_DIR/data"
    PID_FILE="$DATA_DIR/postmaster.pid"
    LOG_FILE="$DATA_DIR/postgres.log"
    LOCK_FILE="$DATA_DIR/start.lock"
    CRED_FILE="$DATA_DIR/credentials/pg.env"

    PG_PORT=5710

    # Ensure base directory exists for lock file
    mkdir -p "$DATA_DIR"

    # Quick check: is it already running?
    if [ -f "$PG_DATA/postmaster.pid" ]; then
      OLD_PID=$(head -1 "$PG_DATA/postmaster.pid" 2>/dev/null || true)
      if [ -n "$OLD_PID" ] && kill -0 "$OLD_PID" 2>/dev/null; then
        echo "PostgreSQL dev instance is already running (PID $OLD_PID)."
        if [ -f "$CRED_FILE" ]; then
          echo ""
          echo "Connection: postgresql://flowstate:flowstate@127.0.0.1:$PG_PORT/flowstate"
          echo "Credentials: $CRED_FILE"
          echo ""
          echo "To load credentials: eval \$(pg-dev-info --env)"
        fi
        exit 0
      fi
    fi

    # Acquire exclusive lock for startup
    exec 9>"$LOCK_FILE"
    if ! flock -n 9; then
      echo "Another pg-dev-start is already running. Waiting..."
      flock 9
    fi

    # Re-check after acquiring lock
    if [ -f "$PG_DATA/postmaster.pid" ]; then
      OLD_PID=$(head -1 "$PG_DATA/postmaster.pid" 2>/dev/null || true)
      if [ -n "$OLD_PID" ] && kill -0 "$OLD_PID" 2>/dev/null; then
        echo "PostgreSQL dev instance is already running (PID $OLD_PID)."
        exec 9>&-
        exit 0
      fi
    fi

    # Check for port conflicts
    if ss -tln 2>/dev/null | grep -q ":$PG_PORT "; then
      echo "ERROR: Port $PG_PORT is already in use."
      ss -tlnp 2>/dev/null | grep ":$PG_PORT " || true
      exec 9>&-
      exit 1
    fi

    # First run: initdb
    if [ ! -f "$PG_DATA/PG_VERSION" ]; then
      echo "First run: initializing database cluster..."
      initdb -D "$PG_DATA" --auth=trust --no-locale --encoding=UTF8 >/dev/null
      echo "Database cluster initialized."
    fi

    # Release startup lock before launching server
    exec 9>&-

    # Start postgres in background
    echo "Starting PostgreSQL dev instance on port $PG_PORT..."
    pg_ctl -D "$PG_DATA" -l "$LOG_FILE" -o "-p $PG_PORT -k /tmp -h 127.0.0.1" start

    # Wait for ready
    echo "Waiting for PostgreSQL to become ready..."
    ATTEMPTS=0
    MAX_ATTEMPTS=30
    while [ $ATTEMPTS -lt $MAX_ATTEMPTS ]; do
      if pg_isready -h 127.0.0.1 -p $PG_PORT -q 2>/dev/null; then
        echo "PostgreSQL is ready."
        break
      fi
      ATTEMPTS=$((ATTEMPTS + 1))
      sleep 1
    done

    if [ $ATTEMPTS -ge $MAX_ATTEMPTS ]; then
      echo "ERROR: PostgreSQL failed to become ready within $MAX_ATTEMPTS seconds."
      echo "Last 20 lines of log:"
      tail -20 "$LOG_FILE" 2>/dev/null || true
      pg_ctl -D "$PG_DATA" stop 2>/dev/null || true
      exit 1
    fi

    # First run: create role and database
    if [ ! -f "$CRED_FILE" ]; then
      echo "First run: creating role and database..."
      mkdir -p "$DATA_DIR/credentials"

      # Create role (ignore error if exists)
      psql -h 127.0.0.1 -p $PG_PORT -d postgres -c \
        "CREATE ROLE flowstate WITH LOGIN PASSWORD 'flowstate';" 2>/dev/null || true

      # Create database (ignore error if exists)
      psql -h 127.0.0.1 -p $PG_PORT -d postgres -c \
        "CREATE DATABASE flowstate OWNER flowstate;" 2>/dev/null || true

      # Write credentials file
      cat > "$CRED_FILE" <<EOF
    FLOWSTATE_DB_BACKEND=postgres
    FLOWSTATE_DATABASE_URL=postgresql://flowstate:flowstate@127.0.0.1:$PG_PORT/flowstate
    DATABASE_URL=postgresql://flowstate:flowstate@127.0.0.1:$PG_PORT/flowstate
    EOF
      chmod 600 "$CRED_FILE"

      echo "Bootstrap complete. Credentials written to $CRED_FILE"
    fi

    echo ""
    echo "=== PostgreSQL Dev Instance ==="
    echo "Host:         127.0.0.1"
    echo "Port:         $PG_PORT"
    echo "Database:     flowstate"
    echo "User:         flowstate"
    echo "Password:     flowstate"
    echo "URL:          postgresql://flowstate:flowstate@127.0.0.1:$PG_PORT/flowstate"
    echo "Credentials:  $CRED_FILE"
    echo ""
    echo "To load credentials: eval \$(pg-dev-info --env)"
  '';

  pgDevStop = pkgs.writeShellScriptBin "pg-dev-stop" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    DATA_DIR="''${XDG_DATA_HOME:-$HOME/.local/share}/flowstate/postgres/dev"
    PG_DATA="$DATA_DIR/data"

    if [ ! -f "$PG_DATA/postmaster.pid" ]; then
      echo "PostgreSQL dev instance is not running."
      exit 0
    fi

    echo "Stopping PostgreSQL dev instance..."
    pg_ctl -D "$PG_DATA" stop -m fast 2>/dev/null || true
    echo "PostgreSQL dev instance stopped."
  '';

  pgDevStatus = pkgs.writeShellScriptBin "pg-dev-status" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    PG_PORT=5710

    if pg_isready -h 127.0.0.1 -p $PG_PORT -q 2>/dev/null; then
      echo "PostgreSQL dev instance: running (port $PG_PORT)"
      exit 0
    fi

    echo "PostgreSQL dev instance: stopped"
    exit 1
  '';

  pgDevInfo = pkgs.writeShellScriptBin "pg-dev-info" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    DATA_DIR="''${XDG_DATA_HOME:-$HOME/.local/share}/flowstate/postgres/dev"
    CRED_FILE="$DATA_DIR/credentials/pg.env"

    if [ ! -f "$CRED_FILE" ]; then
      echo "ERROR: No credentials found. Run pg-dev-start first."
      exit 1
    fi

    if [ "''${1:-}" = "--env" ]; then
      while IFS= read -r line; do
        [ -n "$line" ] && echo "export $line"
      done < "$CRED_FILE"
    else
      echo "=== PostgreSQL Dev Instance ==="
      echo "URL:          postgresql://flowstate:flowstate@127.0.0.1:5710/flowstate"
      echo "Database:     flowstate"
      echo "User:         flowstate"
      echo "Password:     flowstate"
      echo "Credentials:  $CRED_FILE"
      echo ""
      echo "To load into shell: eval \$(pg-dev-info --env)"
    fi
  '';

  pgTestStart = pkgs.writeShellScriptBin "pg-test-start" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    MARKER="/tmp/flowstate-pg-test-current"

    PG_PORT=5711

    # Check if a test instance is already running
    if [ -f "$MARKER" ]; then
      EXISTING_DIR=$(cat "$MARKER")
      if pg_isready -h 127.0.0.1 -p $PG_PORT -q 2>/dev/null; then
        echo "A PostgreSQL test instance is already running on port $PG_PORT."
        echo "Run pg-test-stop first."
        exit 1
      fi
      # Stale marker, clean up
      rm -f "$MARKER"
    fi

    # Check for port conflicts
    if ss -tln 2>/dev/null | grep -q ":$PG_PORT "; then
      echo "ERROR: Port $PG_PORT is already in use."
      ss -tlnp 2>/dev/null | grep ":$PG_PORT " || true
      exit 1
    fi

    # Create temp directory
    TMPDIR=$(mktemp -d /tmp/flowstate-pg-test-XXXXXXXX)
    echo "$TMPDIR" > "$MARKER"

    PG_DATA="$TMPDIR/data"

    # Initialize cluster
    echo "Initializing test database cluster..."
    initdb -D "$PG_DATA" --auth=trust --no-locale --encoding=UTF8 >/dev/null

    # Start postgres
    echo "Starting PostgreSQL test instance on port $PG_PORT..."
    pg_ctl -D "$PG_DATA" -l "$TMPDIR/postgres.log" -o "-p $PG_PORT -k /tmp -h 127.0.0.1" start

    # Wait for ready
    echo "Waiting for PostgreSQL to become ready..."
    ATTEMPTS=0
    MAX_ATTEMPTS=30
    while [ $ATTEMPTS -lt $MAX_ATTEMPTS ]; do
      if pg_isready -h 127.0.0.1 -p $PG_PORT -q 2>/dev/null; then
        echo "PostgreSQL is ready."
        break
      fi
      ATTEMPTS=$((ATTEMPTS + 1))
      sleep 1
    done

    if [ $ATTEMPTS -ge $MAX_ATTEMPTS ]; then
      echo "ERROR: PostgreSQL failed to become ready within $MAX_ATTEMPTS seconds."
      tail -20 "$TMPDIR/postgres.log" 2>/dev/null || true
      pg_ctl -D "$PG_DATA" stop 2>/dev/null || true
      rm -rf "$TMPDIR"
      rm -f "$MARKER"
      exit 1
    fi

    # Create role and database
    psql -h 127.0.0.1 -p $PG_PORT -d postgres -c \
      "CREATE ROLE flowstate WITH LOGIN PASSWORD 'flowstate';" 2>/dev/null || true
    psql -h 127.0.0.1 -p $PG_PORT -d postgres -c \
      "CREATE DATABASE flowstate_test OWNER flowstate;" 2>/dev/null || true

    # Write credentials
    CRED_FILE="$TMPDIR/credentials/pg.env"
    mkdir -p "$TMPDIR/credentials"
    cat > "$CRED_FILE" <<EOF
    FLOWSTATE_DB_BACKEND=postgres
    FLOWSTATE_DATABASE_URL=postgresql://flowstate:flowstate@127.0.0.1:$PG_PORT/flowstate_test
    DATABASE_URL=postgresql://flowstate:flowstate@127.0.0.1:$PG_PORT/flowstate_test
    EOF

    echo ""
    echo "=== PostgreSQL Test Instance ==="
    echo "Port:         $PG_PORT"
    echo "Database:     flowstate_test"
    echo "URL:          postgresql://flowstate:flowstate@127.0.0.1:$PG_PORT/flowstate_test"
    echo "Test dir:     $TMPDIR"
    echo "Credentials:  $CRED_FILE"
    echo ""
    echo "To load credentials: eval \$(pg-test-info --env)"
    echo ""
    echo "To run postgres parity tests:"
    echo "  DATABASE_URL=\"postgresql://flowstate:flowstate@127.0.0.1:$PG_PORT/flowstate_test\" \\"
    echo "    cargo test -p flowstate-db --features postgres -- --ignored"
  '';

  pgTestStop = pkgs.writeShellScriptBin "pg-test-stop" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    MARKER="/tmp/flowstate-pg-test-current"

    if [ ! -f "$MARKER" ]; then
      echo "No PostgreSQL test instance is running."
      exit 0
    fi

    TMPDIR=$(cat "$MARKER")

    if [ ! -d "$TMPDIR" ]; then
      echo "Test directory $TMPDIR does not exist. Cleaning up marker."
      rm -f "$MARKER"
      exit 0
    fi

    PG_DATA="$TMPDIR/data"
    if [ -f "$PG_DATA/postmaster.pid" ]; then
      echo "Stopping PostgreSQL test instance..."
      pg_ctl -D "$PG_DATA" stop -m immediate 2>/dev/null || true
    fi

    echo "Removing test directory $TMPDIR..."
    rm -rf "$TMPDIR"
    rm -f "$MARKER"
    echo "PostgreSQL test instance stopped and data wiped."
  '';

  pgTestStatus = pkgs.writeShellScriptBin "pg-test-status" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    PG_PORT=5711

    if pg_isready -h 127.0.0.1 -p $PG_PORT -q 2>/dev/null; then
      echo "PostgreSQL test instance: running (port $PG_PORT)"
      exit 0
    fi

    echo "PostgreSQL test instance: stopped"
    exit 1
  '';

  pgTestInfo = pkgs.writeShellScriptBin "pg-test-info" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    MARKER="/tmp/flowstate-pg-test-current"

    if [ ! -f "$MARKER" ]; then
      echo "ERROR: No test instance is running. Run pg-test-start first."
      exit 1
    fi

    TMPDIR=$(cat "$MARKER")
    CRED_FILE="$TMPDIR/credentials/pg.env"

    if [ ! -f "$CRED_FILE" ]; then
      echo "ERROR: No credentials found."
      exit 1
    fi

    if [ "''${1:-}" = "--env" ]; then
      while IFS= read -r line; do
        [ -n "$line" ] && echo "export $line"
      done < "$CRED_FILE"
    else
      echo "=== PostgreSQL Test Instance ==="
      echo "URL:          postgresql://flowstate:flowstate@127.0.0.1:5711/flowstate_test"
      echo "Database:     flowstate_test"
      echo "Test dir:     $TMPDIR"
      echo "Credentials:  $CRED_FILE"
      echo ""
      echo "To load into shell: eval \$(pg-test-info --env)"
    fi
  '';

in {
  inherit pgDevStart pgDevStop pgDevStatus pgDevInfo;
  inherit pgTestStart pgTestStop pgTestStatus pgTestInfo;

  all = [
    pgDevStart pgDevStop pgDevStatus pgDevInfo
    pgTestStart pgTestStop pgTestStatus pgTestInfo
    postgresql
  ];
}
