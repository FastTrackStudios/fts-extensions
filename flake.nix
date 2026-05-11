{
  description = "Task — spec-driven task management";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    crane.url = "github:ipetkov/crane";

    # Shared Dioxus toolchain (web/desktop/mobile/native) — re-exposed
    # below as `devShells.ui` and `devShells.mobile`.
    # Use the pushed fork as a real git input so flake eval/builds don't
    # depend on a local checkout path.
    dioxus-flake = {
      url = "github:FastTrackStudios/Dioxus-Flake";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-overlay.follows = "rust-overlay";
      inputs.crane.follows = "crane";
    };

  };

  outputs = inputs@{ flake-parts, nixpkgs, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      # ── NixOS module (for deploying task-server) ──────────────────
      flake = {
        nixosModules.default = ./nix/module.nix;
        nixosModules.task-server = ./nix/module.nix;
        nixosModules.task-email-watcher = ./nix/task-email-watcher.nix;
      };

      perSystem = { system, lib, self', ... }:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ inputs.rust-overlay.overlays.default ];
          };

          # ── Rust toolchain ──────────────────────────────────────────────
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

          craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;

          # ── Source layout ───────────────────────────────────────────────
          # All fork dependencies are pinned by git rev in Cargo.toml, so
          # cargo+crane handle vendoring with no in-flake rewrites.
          taskSrc = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              let name = builtins.baseNameOf (toString path); in
              !(builtins.elem name [ "target" "node_modules" ".git" ".beads" "result" ]);
          };

          cargoVendorDir = craneLib.vendorCargoDeps { src = taskSrc; };
          taskWebCargoVendorDir = craneLib.vendorCargoDeps {
            src = taskSrc;
            cargoLock = ./apps/web/Cargo.lock;
          };

          commonArgs = {
            src = taskSrc;
            inherit cargoVendorDir;
            strictDeps = true;
            nativeBuildInputs = with pkgs; [ pkg-config ];
            buildInputs = with pkgs; [ openssl ];
          };

          # ── wasm-bindgen-cli pinned to match Cargo.lock ────────────────
          wasm-bindgen-cli = pkgs.rustPlatform.buildRustPackage rec {
            pname = "wasm-bindgen-cli";
            version = "0.2.116";
            src = pkgs.fetchCrate {
              inherit pname version;
              hash = "sha256-Pcb5+dEaWK+SQJz73/3Si0kxEy8y8n21t+DETMdvKHc=";
            };
            cargoHash = "sha256-PztHuoWBh+wqOuSFTQxnnddKSSu0S1MBOgKy0szHQpI=";
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = lib.optionals pkgs.stdenv.isLinux [ pkgs.openssl ]
              ++ lib.optionals pkgs.stdenv.isDarwin
                (with pkgs.darwin.apple_sdk.frameworks; [ Security CoreFoundation ]);
            doCheck = false;
          };

          wasm-bindgen-cli-web = pkgs.rustPlatform.buildRustPackage rec {
            pname = "wasm-bindgen-cli";
            version = "0.2.121";
            src = pkgs.fetchCrate {
              inherit pname version;
              hash = "sha256-ZOMgFNOcGkO66Jz/Z83eoIu+DIzo3Z/vq6Z5g6BDY/w=";
            };
            cargoHash = "sha256-DPdCDPTAPBrbqLUqnCwQu1dePs9lGg85JCJOCIr9qjU=";
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = lib.optionals pkgs.stdenv.isLinux [ pkgs.openssl ]
              ++ lib.optionals pkgs.stdenv.isDarwin
                (with pkgs.darwin.apple_sdk.frameworks; [ Security CoreFoundation ]);
            doCheck = false;
          };

          # ── Packages ────────────────────────────────────────────────────

          task-server = craneLib.buildPackage (commonArgs // {
            pname = "task-server";
            version = "0.1.0";
            cargoExtraArgs = "--package task-server";
            doCheck = false;
          });

          task-cli = craneLib.buildPackage (commonArgs // {
            pname = "task-cli";
            version = "0.1.0";
            cargoExtraArgs = "--package task-cli";
            doCheck = false;
          });

          xtask = craneLib.buildPackage (commonArgs // {
            pname = "xtask";
            cargoExtraArgs = "--package xtask";
            doCheck = false;
          });

          vault-core = craneLib.buildPackage (commonArgs // {
            pname = "vault-core";
            cargoExtraArgs = "--package vault-core";
          });

          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
            pname = "task-deps";
            version = "0.1.0";
          });

          obsidian-wasm = craneLib.mkCargoDerivation (commonArgs // {
            pname = "obsidian-plugin-wasm";
            inherit cargoArtifacts;
            CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
            buildPhaseCargoCommand = ''
              cargo build --release \
                --package obsidian-plugin \
                --target wasm32-unknown-unknown
            '';
            installPhase = ''
              mkdir -p $out/lib
              cp target/wasm32-unknown-unknown/release/obsidian_plugin.wasm $out/lib/
            '';
            doCheck = false;
          });

          task-webapp = craneLib.buildPackage (commonArgs // {
            pname = "task-webapp";
            version = "0.1.0";
            cargoVendorDir = taskWebCargoVendorDir;
            cargoArtifacts = null;
            cargoExtraArgs = "--manifest-path apps/web/Cargo.toml";
            nativeBuildInputs = commonArgs.nativeBuildInputs ++ (with pkgs; [
              dioxus-cli
              tailwindcss_4
              wasm-bindgen-cli-web
              binaryen
            ]);
            doNotPostBuildInstallCargoBinaries = true;
            buildPhaseCargoCommand = ''
              tailwindcss -i apps/web/tailwind.css -o apps/web/assets/tailwind.css
              cd apps/web
              dx build --release --platform web
            '';
            installPhaseCommand = ''
              mkdir -p $out/www
              cp -R target/dx/task-app-web/release/web/public/* $out/www/
            '';
            doCheck = false;
          });

        in
        {
          packages = {
            default = task-cli;
            inherit task-server task-cli task-webapp xtask vault-core obsidian-wasm wasm-bindgen-cli;
          };

          # ── Checks ──────────────────────────────────────────────────────
          checks = {
            vault-core-tests = craneLib.cargoTest (commonArgs // {
              inherit cargoArtifacts;
              pname = "vault-core-tests";
              version = "0.1.0";
              cargoExtraArgs = "--package vault-core";
            });
            fmt = craneLib.cargoFmt { src = taskSrc; pname = "task-fmt"; version = "0.1.0"; };
          };

          # ── Dev shell ───────────────────────────────────────────────────
          devShells.default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              rustToolchain
              cargo-watch
              cargo-expand
              wasm-pack
              wasm-bindgen-cli
              binaryen
              nodejs_22
              nodePackages.npm
              pkg-config
              openssl.dev
            ];

            RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

            shellHook = ''
              echo ""
              echo "  task dev shell"
              echo "  ──────────────────────────────────────────────"
              echo "  nix build .#task-server   Vox/CRDT service"
              echo "  nix build .#task-cli      CLI tool"
              echo "  nix build .#obsidian-wasm  Obsidian plugin WASM"
              echo "  nix build .#task-webapp    Dioxus/Tailwind web bundle"
              echo "  nix flake check           tests, clippy, fmt"
              echo "  nix develop .#ui          Dioxus web/desktop dev shell"
              echo "  nix develop .#mobile      Dioxus mobile (Android) dev shell"
              echo ""
            '';
          };

          # Re-export the shared Dioxus dev shells so app work uses the
          # same toolchain as fts-ui / dioxus-flake without duplicating
          # config here.
          devShells.ui = inputs.dioxus-flake.devShells.${system}.default;
          devShells.mobile = inputs.dioxus-flake.devShells.${system}.mobile;
        };
    };
}
