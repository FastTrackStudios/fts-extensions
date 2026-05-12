{
  description = "architect — example-proto pattern (vox + Dioxus + SeaORM)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    # Dioxus dev shell — supplies rustc/cargo with wasm32 target,
    # wasm-bindgen-cli, binaryen, tailwindcss, dx, nodejs.
    dioxus-flake = {
      url = "github:FastTrackStudios/Dioxus-Flake";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      dioxus-flake,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Resolve a github-release platform tag for the current system.
        # Tracks the targets bearcove ships (linux-gnu + apple-darwin).
        bearcoveTarget =
          {
            "x86_64-linux" = "x86_64-unknown-linux-gnu";
            "aarch64-linux" = "aarch64-unknown-linux-gnu";
            "x86_64-darwin" = "x86_64-apple-darwin";
            "aarch64-darwin" = "aarch64-apple-darwin";
          }
          .${system};

        # ── Pre-built tools from bearcove releases ────────────────────
        #
        # ddc (dodeca) and tracey aren't in nixpkgs. We pull the
        # release tarballs and patch their interpreters via
        # autoPatchelfHook so they run inside the nix store. Update
        # `version` + `hash` when bumping.

        ddc = pkgs.stdenv.mkDerivation rec {
          pname = "dodeca";
          version = "0.13.0";
          src = pkgs.fetchurl {
            url = "https://github.com/bearcove/dodeca/releases/download/v${version}/dodeca-${bearcoveTarget}.tar.xz";
            hash = "sha256-l6vXPKLr/qQ4dOAGWZCZafq/LsvAjL74W5GnxiBO8xY=";
          };
          sourceRoot = ".";
          nativeBuildInputs = [
            pkgs.autoPatchelfHook
          ];
          buildInputs = [
            pkgs.stdenv.cc.cc.lib
            pkgs.openssl
          ];
          # The tarball is flat — `ddc`, every `ddc-cell-*`, plus a
          # `devtools/` dir. ddc looks for cells in its own directory,
          # so everything has to live next to each other under bin/.
          installPhase = ''
            mkdir -p $out/bin
            cp -r ./* $out/bin/
            chmod +x $out/bin/ddc $out/bin/ddc-cell-*
          '';
          meta = {
            description = "Fully incremental static site generator (bearcove)";
            homepage = "https://github.com/bearcove/dodeca";
            license = pkgs.lib.licenses.mit;
            platforms = pkgs.lib.platforms.unix;
          };
        };

        tracey = pkgs.stdenv.mkDerivation rec {
          pname = "tracey";
          version = "1.3.0";
          src = pkgs.fetchurl {
            url = "https://github.com/bearcove/tracey/releases/download/v${version}/tracey-${bearcoveTarget}.tar.xz";
            hash = "sha256-+8DCXEQyjMsJcLJQkJX/KUEvpyy7xrADbEzujBKCH0c=";
          };
          nativeBuildInputs = [ pkgs.autoPatchelfHook ];
          buildInputs = [
            pkgs.stdenv.cc.cc.lib
            pkgs.openssl
          ];
          # Tarball contains a `tracey-<target>/` dir with the binary
          # at its root. Drop the wrapper, install just `tracey`.
          installPhase = ''
            mkdir -p $out/bin
            install -m755 tracey $out/bin/tracey
          '';
          meta = {
            description = "Spec ↔ implementation ↔ test traceability (bearcove)";
            homepage = "https://github.com/bearcove/tracey";
            license = pkgs.lib.licenses.mit;
            platforms = pkgs.lib.platforms.unix;
          };
        };

        # Browser + driver for `wasm-bindgen-test-runner`. Pinned via
        # nixpkgs so a fresh checkout doesn't need a global install.
        wasmTestDeps = [
          pkgs.geckodriver
          pkgs.firefox
          pkgs.chromedriver
          pkgs.chromium
        ];

        # Everything else the workspace needs day-to-day. The Dioxus
        # shell already covers rustc/cargo/wasm-bindgen-cli; this list
        # is purely the architect-specific additions.
        extraDeps = with pkgs; [
          just
          sqlite # introspect the dev sqlite DB
          sea-orm-cli # migrations CLI
          wasm-pack # alternative wasm test runner
          git-cliff # CHANGELOG.md from conventional commits
          pkg-config
          openssl
          curl
          ddc
          tracey
        ];

        # Base shell from dioxus-flake — gives us dx + cargo + wasm32 +
        # tailwindcss + wasm-bindgen-cli. We layer on the wasm test
        # harness deps and a shellHook that auto-wires the runner.
        baseShell = dioxus-flake.devShells.${system}.default;
      in
      {
        # Expose the tool packages directly so consumers can pin them.
        packages = {
          inherit ddc tracey;
        };

        devShells = {
          default = pkgs.mkShell {
            inputsFrom = [ baseShell ];
            buildInputs = wasmTestDeps ++ extraDeps;

            shellHook = ''
              # Prepend the pinned tool versions so any pre-existing
              # `~/.cargo/bin/{ddc,tracey}` install from before this
              # shell existed doesn't shadow the version the flake
              # pins.
              export PATH="${ddc}/bin:${tracey}/bin:$PATH"

              # Auto-wire `cargo test --target wasm32-unknown-unknown`:
              # cargo invokes wasm-bindgen-test-runner (from the dioxus
              # shell), which then drives geckodriver/firefox.
              export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner
              export GECKODRIVER=${pkgs.geckodriver}/bin/geckodriver
              export CHROMEDRIVER=${pkgs.chromedriver}/bin/chromedriver

              # Default DB path for `cargo run -p app-server` and
              # `cargo run -p app-db -- up`.
              : "''${DATABASE_URL:=sqlite://./example.db?mode=rwc}"
              export DATABASE_URL

              echo "architect dev shell"
              echo "  cargo, dx, wasm-bindgen-cli, ddc, tracey, geckodriver, firefox on PATH"
              echo "  CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=$CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER"
              echo "  DATABASE_URL=$DATABASE_URL"
              echo
              echo "  cargo xtask <cmd>     check / build / test / e2e / docs / wiki / ci"
              echo "  just server           run app-server"
              echo "  just test-e2e         server + browser tests (sqlite backend)"
              echo "  just test-e2e-memory  server + browser tests (in-memory backend)"
              echo "  just docs             dodeca serve"
              echo "  just tracey-validate  spec ↔ impl ↔ verify check"
            '';
          };

          # Compatibility alias — older docs/scripts still reference `.#ui`.
          ui = self.devShells.${system}.default;
        };
      }
    );
}
