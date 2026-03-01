{ pkgs, rustToolchain }:
let
  runtimePath = pkgs.lib.makeBinPath [
    rustToolchain
    pkgs.coreutils
    pkgs.curl
    pkgs.jq
    pkgs.bash
    pkgs.gnugrep
  ];

  flowstateSmokeTest = pkgs.writeShellScriptBin "flowstate-smoke-test" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    PORT="''${FLOWSTATE_SMOKE_PORT:-3799}"
    BASE="http://127.0.0.1:$PORT"

    PASS=0
    FAIL=0
    SERVER_PID=""
    STARTED_PG=0

    # ── Helpers ──────────────────────────────────────────────────

    cleanup() {
      echo ""
      echo "=== Cleanup ==="
      if [ -n "$SERVER_PID" ]; then
        echo "Stopping server (pid $SERVER_PID)..."
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
      fi
      if [ "$STARTED_PG" -eq 1 ]; then
        echo "Stopping ephemeral Postgres..."
        pg-test-stop 2>/dev/null || true
      fi
      echo ""
      echo "=== Results: $PASS passed, $FAIL failed ==="
      if [ "$FAIL" -gt 0 ]; then
        exit 1
      fi
    }
    trap cleanup EXIT

    check() {
      local label="$1"
      local expect="$2"
      local actual="$3"
      if [ "$actual" = "$expect" ]; then
        echo "  PASS  $label"
        PASS=$((PASS + 1))
      else
        echo "  FAIL  $label (expected '$expect', got '$actual')"
        FAIL=$((FAIL + 1))
      fi
    }

    check_contains() {
      local label="$1"
      local needle="$2"
      local haystack="$3"
      if echo "$haystack" | grep -q "$needle"; then
        echo "  PASS  $label"
        PASS=$((PASS + 1))
      else
        echo "  FAIL  $label (expected to contain '$needle')"
        FAIL=$((FAIL + 1))
      fi
    }

    http_status() {
      curl -s -o /dev/null -w "%{http_code}" "$@"
    }

    # ── 1. Start ephemeral Postgres ──────────────────────────────

    echo "=== Flowstate Smoke Test ==="
    echo ""
    echo "Starting ephemeral Postgres on port 5711..."
    pg-test-stop 2>/dev/null || true
    pg-test-start
    STARTED_PG=1

    PG_MARKER="/tmp/flowstate-pg-test-current"
    if [ ! -f "$PG_MARKER" ]; then
      echo "ERROR: pg-test-start did not create marker file."
      exit 1
    fi
    PG_TMPDIR=$(cat "$PG_MARKER")
    PG_CRED_FILE="$PG_TMPDIR/credentials/pg.env"
    set -a
    source "$PG_CRED_FILE"
    set +a
    echo ""

    # ── 2. Build and start server ────────────────────────────────

    echo "Building server (postgres feature)..."
    cargo build -p flowstate-server --no-default-features --features postgres 2>&1
    echo ""

    echo "Starting server on port $PORT..."
    unset FLOWSTATE_API_KEY 2>/dev/null || true
    FLOWSTATE_DB_BACKEND=postgres \
    FLOWSTATE_DATABASE_URL="$DATABASE_URL" \
    FLOWSTATE_PORT="$PORT" \
    FLOWSTATE_BIND="127.0.0.1" \
      cargo run -p flowstate-server --no-default-features --features postgres &
    SERVER_PID=$!

    # Wait for health
    echo "Waiting for server..."
    for i in $(seq 1 30); do
      if curl -sf "$BASE/api/health" >/dev/null 2>&1; then
        break
      fi
      sleep 1
    done

    HEALTH=$(curl -sf "$BASE/api/health" 2>/dev/null || echo "UNREACHABLE")
    if [ "$HEALTH" = "UNREACHABLE" ]; then
      echo "ERROR: Server did not start within 30 seconds."
      exit 1
    fi
    echo ""

    # ── 3. Smoke tests ──────────────────────────────────────────

    echo "--- Setup flow ---"

    STATUS=$(curl -s "$BASE/api/setup/status" | jq -r .setup_needed)
    check "setup_needed on fresh DB" "true" "$STATUS"

    INIT=$(curl -s -X POST "$BASE/api/setup/init" \
      -H 'Content-Type: application/json' -d '{"name":"smoke-admin"}')
    API_KEY=$(echo "$INIT" | jq -r .api_key)
    check_contains "setup_init returns fs_ key" "fs_" "$API_KEY"

    STATUS=$(curl -s "$BASE/api/setup/status" | jq -r .setup_needed)
    check "setup_needed after init" "false" "$STATUS"

    CODE=$(http_status -X POST "$BASE/api/setup/init" \
      -H 'Content-Type: application/json' -d '{"name":"intruder"}')
    check "double init rejected" "403" "$CODE"

    echo ""
    echo "--- Auth enforcement ---"

    CODE=$(http_status "$BASE/api/projects")
    check "unauthenticated → 401" "401" "$CODE"

    CODE=$(http_status -H "Authorization: Bearer wrong-key" "$BASE/api/projects")
    check "bad key → 401" "401" "$CODE"

    CODE=$(http_status -H "Authorization: Bearer $API_KEY" "$BASE/api/projects")
    check "valid key → 200" "200" "$CODE"

    CODE=$(http_status "$BASE/api/health")
    check "health is public" "200" "$CODE"

    echo ""
    echo "--- Project CRUD ---"

    PROJECT=$(curl -s -X POST -H "Authorization: Bearer $API_KEY" \
      -H 'Content-Type: application/json' "$BASE/api/projects" \
      -d '{"name":"smoke-project","slug":"smoke","repo_url":"https://github.com/test/repo","provider_type":"github"}')
    PROJECT_ID=$(echo "$PROJECT" | jq -r .id)
    check_contains "create project returns UUID" "-" "$PROJECT_ID"

    PNAME=$(curl -s -H "Authorization: Bearer $API_KEY" "$BASE/api/projects/$PROJECT_ID" | jq -r .name)
    check "read project back" "smoke-project" "$PNAME"

    echo ""
    echo "--- Task CRUD ---"

    TASK=$(curl -s -X POST -H "Authorization: Bearer $API_KEY" \
      -H 'Content-Type: application/json' "$BASE/api/tasks" \
      -d "{\"title\":\"Smoke task\",\"project_id\":\"$PROJECT_ID\",\"status\":\"todo\",\"priority\":\"medium\"}")
    TASK_ID=$(echo "$TASK" | jq -r .id)
    check_contains "create task returns UUID" "-" "$TASK_ID"

    TSTATUS=$(curl -s -H "Authorization: Bearer $API_KEY" "$BASE/api/tasks/$TASK_ID" | jq -r .status)
    check "read task status" "todo" "$TSTATUS"

    UPDATED=$(curl -s -X PUT -H "Authorization: Bearer $API_KEY" \
      -H 'Content-Type: application/json' "$BASE/api/tasks/$TASK_ID" \
      -d '{"status":"research"}')
    USTATUS=$(echo "$UPDATED" | jq -r .status)
    check "update task status" "research" "$USTATUS"

    TCOUNT=$(curl -s -H "Authorization: Bearer $API_KEY" \
      "$BASE/api/tasks?project_id=$PROJECT_ID" | jq length)
    check "list tasks count" "1" "$TCOUNT"

    echo ""
    echo "--- API key management ---"

    RUNNER_KEY=$(curl -s -X POST -H "Authorization: Bearer $API_KEY" \
      -H 'Content-Type: application/json' "$BASE/api/admin/api-keys" \
      -d '{"name":"smoke-runner"}')
    RK=$(echo "$RUNNER_KEY" | jq -r .api_key)
    RK_ID=$(echo "$RUNNER_KEY" | jq -r .id)
    check_contains "generate runner key" "fs_" "$RK"

    KEY_COUNT=$(curl -s -H "Authorization: Bearer $API_KEY" \
      "$BASE/api/admin/api-keys" | jq length)
    check "list keys count" "2" "$KEY_COUNT"

    CODE=$(http_status -X DELETE -H "Authorization: Bearer $API_KEY" \
      "$BASE/api/admin/api-keys/$RK_ID")
    check "revoke key → 204" "204" "$CODE"

    CODE=$(http_status -H "Authorization: Bearer $RK" "$BASE/api/projects")
    check "revoked key → 401" "401" "$CODE"

    echo ""
    echo "--- Runner registration ---"

    # Generate a fresh runner key (the previous one was revoked)
    RK2=$(curl -s -X POST -H "Authorization: Bearer $API_KEY" \
      -H 'Content-Type: application/json' "$BASE/api/admin/api-keys" \
      -d '{"name":"smoke-runner-2"}' | jq -r .api_key)

    REG=$(curl -s -X POST -H "Authorization: Bearer $RK2" \
      -H 'Content-Type: application/json' "$BASE/api/runners/register" \
      -d '{"runner_id":"smoke-001","capability":"standard","agent_backend":"claude-cli"}')
    REG_STATUS=$(echo "$REG" | jq -r .status)
    check "runner registration" "registered" "$REG_STATUS"

    CLAIM_CODE=$(http_status -X POST -H "Authorization: Bearer $RK2" \
      -H "X-Runner-Id: smoke-001" \
      -H 'Content-Type: application/json' "$BASE/api/claude-runs/claim")
    check "runner claim (no work → 204)" "204" "$CLAIM_CODE"

    echo ""
  '';

in {
  inherit flowstateSmokeTest;
  all = [ flowstateSmokeTest ];
}
