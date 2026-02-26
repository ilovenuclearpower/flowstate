{
  description = "flowstate - Task tracker for vibe coding";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, flake-utils, fenix, crane }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        rustToolchain = fenix.packages.${system}.stable.withComponents [
          "cargo" "clippy" "rustc" "rustfmt" "rust-src" "rust-analyzer" "llvm-tools"
        ];

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        flowstate-tui = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          cargoExtraArgs = "-p flowstate-tui";
          meta.mainProgram = "flowstate";
        });

        flowstate-server = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          cargoExtraArgs = "-p flowstate-server";
          meta.mainProgram = "flowstate-server";
        });

        flowstate-mcp = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          cargoExtraArgs = "-p flowstate-mcp";
          meta.mainProgram = "flowstate-mcp";
        });

        flowstate-runner = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          cargoExtraArgs = "-p flowstate-runner";
          meta.mainProgram = "flowstate-runner";
        });

        flowstate-server-postgres = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          cargoExtraArgs = "-p flowstate-server --no-default-features --features postgres";
          meta.mainProgram = "flowstate-server";
        });

        garageScripts = import ./nix/garage.nix { inherit pkgs; };
        postgresScripts = import ./nix/postgres.nix { inherit pkgs; };
        giteaScripts = import ./nix/gitea.nix { inherit pkgs; };
        coverageScripts = import ./nix/coverage.nix { inherit pkgs rustToolchain; };
        runnerGeminiScripts = import ./nix/runner-gemini.nix { inherit pkgs; };
        runnerClaudeScripts = import ./nix/runner-claude.nix { inherit pkgs; };
        runnerOpencodeScripts = import ./nix/runner-opencode.nix { inherit pkgs; };

      in {
        packages = {
          default = flowstate-tui;
          tui = flowstate-tui;
          server = flowstate-server;
          server-postgres = flowstate-server-postgres;
          mcp = flowstate-mcp;
          runner = flowstate-runner;
        };

        checks = {
          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--workspace -- -D warnings";
          });
          test = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
          });
          fmt = craneLib.cargoFmt { inherit src; };
        };

        apps.coverage = {
          type = "app";
          program = "${coverageScripts.flowstateCoverage}/bin/flowstate-coverage";
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};
          packages = [
            pkgs.sqlite
            pkgs.git
            pkgs.openssl
            pkgs.cargo-llvm-cov
            pkgs.gemini-cli
            pkgs.jq
          ] ++ garageScripts.all
            ++ postgresScripts.all
            ++ giteaScripts.all
            ++ coverageScripts.all
            ++ runnerGeminiScripts.all
            ++ runnerClaudeScripts.all
            ++ runnerOpencodeScripts.all;
          RUST_MIN_STACK = "67108864";
          shellHook = ''
            # Install project git hooks
            if [ -d ".githooks" ]; then
              git config core.hooksPath .githooks
            fi

            echo "flowstate dev shell"
            echo "  cargo: $(cargo --version)"
            echo "  rustc: $(rustc --version)"

            # Auto-load Garage dev credentials if instance is running
            GARAGE_CRED_FILE="''${XDG_DATA_HOME:-$HOME/.local/share}/flowstate/garage/dev/credentials/s3.env"
            if [ -f "$GARAGE_CRED_FILE" ]; then
              set -a
              source "$GARAGE_CRED_FILE"
              set +a
              echo "  garage: loaded S3 credentials (endpoint=$AWS_ENDPOINT_URL)"
            fi

            # Auto-load Postgres dev credentials if instance is running
            PG_CRED_FILE="''${XDG_DATA_HOME:-$HOME/.local/share}/flowstate/postgres/dev/credentials/pg.env"
            if [ -f "$PG_CRED_FILE" ]; then
              set -a
              source "$PG_CRED_FILE"
              set +a
              echo "  postgres: loaded credentials (backend=$FLOWSTATE_DB_BACKEND)"
            fi

            # Auto-load runner credentials (API keys for agent backends)
            RUNNER_CRED_FILE="''${XDG_DATA_HOME:-$HOME/.local/share}/flowstate/runner/credentials/runner.env"
            if [ -f "$RUNNER_CRED_FILE" ]; then
              set -a
              source "$RUNNER_CRED_FILE"
              set +a
              echo "  runner: loaded credentials"
            fi

            echo ""
            echo "Garage commands:"
            echo "  garage-dev-start   - Start persistent Garage (S3 on :3900)"
            echo "  garage-dev-stop    - Stop persistent Garage"
            echo "  garage-dev-status  - Check persistent Garage status"
            echo "  garage-dev-info    - Show S3 credentials"
            echo "  garage-test-start  - Start ephemeral Garage (S3 on :3910)"
            echo "  garage-test-stop   - Stop ephemeral Garage and wipe data"
            echo "  garage-test-status - Check ephemeral Garage status"
            echo "  garage-test-info   - Show test S3 credentials"
            echo ""
            echo "Gitea commands:"
            echo "  gitea-test-start  - Start ephemeral Gitea (HTTP on :3920)"
            echo "  gitea-test-stop   - Stop ephemeral Gitea and wipe data"
            echo "  gitea-test-status - Check ephemeral Gitea status"
            echo "  gitea-test-info   - Show test Gitea credentials"
            echo ""
            echo "Runner:"
            echo "  runner-claude       - Start runner with Claude CLI (port 3714)"
            echo "  runner-gemini-pro   - Start runner with Gemini 3.1 Pro (port 3712)"
            echo "  runner-gemini-flash - Start runner with Gemini 3 Flash (port 3713)"
            echo "  runner-opencode     - Start runner with OpenCode CLI (port 3715)"
            echo ""
            echo "Coverage:"
            echo "  flowstate-coverage - Run full coverage suite (starts Postgres + Garage)"
            echo ""
            echo "Postgres commands:"
            echo "  pg-dev-start       - Start persistent Postgres (port 5710)"
            echo "  pg-dev-stop        - Stop persistent Postgres"
            echo "  pg-dev-status      - Check persistent Postgres status"
            echo "  pg-dev-info        - Show Postgres credentials"
            echo "  pg-test-start      - Start ephemeral Postgres (port 5711)"
            echo "  pg-test-stop       - Stop ephemeral Postgres and wipe data"
            echo "  pg-test-status     - Check ephemeral Postgres status"
            echo "  pg-test-info       - Show test Postgres credentials"
          '';
        };
      }
    );
}
