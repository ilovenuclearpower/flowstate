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
          "cargo" "clippy" "rustc" "rustfmt" "rust-src" "rust-analyzer"
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

        garageScripts = import ./nix/garage.nix { inherit pkgs; };

      in {
        packages = {
          default = flowstate-tui;
          tui = flowstate-tui;
          server = flowstate-server;
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

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};
          packages = [
            pkgs.sqlite
            pkgs.git
            pkgs.openssl
          ] ++ garageScripts.all;
          shellHook = ''
            echo "flowstate dev shell"
            echo "  cargo: $(cargo --version)"
            echo "  rustc: $(rustc --version)"
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
          '';
        };
      }
    );
}
