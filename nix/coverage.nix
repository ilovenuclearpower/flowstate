{ pkgs, rustToolchain }:
let
  clang = pkgs.llvmPackages.clang;

  runtimePath = pkgs.lib.makeBinPath [
    rustToolchain
    pkgs.cargo-llvm-cov
    clang
    pkgs.coreutils
    pkgs.gnugrep
    pkgs.gawk
    pkgs.bash
  ];

  flowstateCoverage = pkgs.writeShellScriptBin "flowstate-coverage" ''
    set -euo pipefail
    export PATH="${runtimePath}:$PATH"

    THRESHOLD="''${FLOWSTATE_COV_THRESHOLD:-90}"

    # Use clang as the C compiler so bundled C deps (libsqlite3-sys, ring, aws-lc-sys)
    # can handle LLVM instrumentation flags (-fprofile-instr-generate, -fcoverage-mapping)
    export CC="clang"

    echo "=== Flowstate Coverage ==="
    echo "Threshold: $THRESHOLD% line coverage"
    echo ""

    # ── Track what we started so cleanup is correct ──
    STARTED_PG=0
    STARTED_GARAGE=0
    CARGO_EXIT=0

    cleanup() {
      echo ""
      echo "=== Cleanup ==="
      if [ "$STARTED_GARAGE" -eq 1 ]; then
        echo "Stopping ephemeral Garage..."
        garage-test-stop 2>/dev/null || true
      fi
      if [ "$STARTED_PG" -eq 1 ]; then
        echo "Stopping ephemeral Postgres..."
        pg-test-stop 2>/dev/null || true
      fi
      echo "Cleanup complete."
    }
    trap cleanup EXIT

    # ── 1. Start ephemeral Postgres ──
    echo "Starting ephemeral Postgres on port 5711..."
    pg-test-start
    STARTED_PG=1

    # Source Postgres credentials
    PG_MARKER="/tmp/flowstate-pg-test-current"
    if [ ! -f "$PG_MARKER" ]; then
      echo "ERROR: pg-test-start did not create marker file."
      exit 1
    fi
    PG_TMPDIR=$(cat "$PG_MARKER")
    PG_CRED_FILE="$PG_TMPDIR/credentials/pg.env"
    if [ ! -f "$PG_CRED_FILE" ]; then
      echo "ERROR: Postgres credential file not found at $PG_CRED_FILE"
      exit 1
    fi
    set -a
    source "$PG_CRED_FILE"
    set +a
    echo "Postgres credentials loaded."
    echo ""

    # ── 2. Start ephemeral Garage S3 ──
    echo "Starting ephemeral Garage on port 3910..."
    garage-test-start
    STARTED_GARAGE=1

    # Source Garage credentials
    GARAGE_MARKER="/tmp/flowstate-garage-test-current"
    if [ ! -f "$GARAGE_MARKER" ]; then
      echo "ERROR: garage-test-start did not create marker file."
      exit 1
    fi
    GARAGE_TMPDIR=$(cat "$GARAGE_MARKER")
    GARAGE_CRED_FILE="$GARAGE_TMPDIR/credentials/s3.env"
    if [ ! -f "$GARAGE_CRED_FILE" ]; then
      echo "ERROR: Garage credential file not found at $GARAGE_CRED_FILE"
      exit 1
    fi
    set -a
    source "$GARAGE_CRED_FILE"
    set +a
    echo "Garage S3 credentials loaded."
    echo ""

    # ── 3. Run cargo llvm-cov ──
    echo "Running cargo llvm-cov (workspace, all features, including ignored tests)..."
    echo ""

    cargo llvm-cov \
      --workspace \
      --all-features \
      --fail-under-lines "$THRESHOLD" \
      --ignore-filename-regex '(/main\.rs$|/(mock|test_helpers)\.rs$|backend/claude_cli\.rs$|backend/opencode\.rs$|repo_provider/github\.rs$)' \
      --exclude flowstate-mcp \
      -- --include-ignored \
      || CARGO_EXIT=$?

    echo ""
    if [ "$CARGO_EXIT" -eq 0 ]; then
      echo "Coverage threshold ($THRESHOLD%) met."
    else
      echo "Coverage threshold ($THRESHOLD%) NOT met (or tests failed)."
    fi

    exit "$CARGO_EXIT"
  '';

in {
  inherit flowstateCoverage;
  all = [ flowstateCoverage ];
}
