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

    # Shared Dioxus toolchain (web/desktop/mobile/native). Its default
    # dev shell is reused below as our `devShells.default` so capn
    # pre-push and any other workspace-wide build sees the full Linux
    # GTK + WebView dep list out of the box. Pointed at the local
    # checkout temporarily so dx CLI bumps land without a fork push
    # round-trip. Switch back to `github:FastTrackStudios/Dioxus-Flake`
    # once those changes are upstreamed.
    dioxus-flake = {
      url = "path:/home/cody/Development/Dioxus/dioxus-flake";
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
          #
          # One shell, everything in it. Reuses dioxus-flake's default
          # shell so we get the full Linux GTK + WebView dep list
          # (pango, webkitgtk_4_1, gtk3, libsoup_3, gst plugins, X11 /
          # Wayland, libGL, …) needed by `apps/desktop` and capn
          # pre-push — without duplicating the package list here.
          # The mobile shell stays separate because the Android NDK
          # toolchain it pulls in is heavy and only matters for that
          # surface.
          devShells.default = inputs.dioxus-flake.devShells.${system}.default;
          devShells.mobile = inputs.dioxus-flake.devShells.${system}.mobile;

          # ── Playwright shell ───────────────────────────────────────
          #
          # Browser-testing surface for `tests/playwright/`. Layers
          # nodejs + a Nix-managed Chromium on top of the default
          # shell so we still get cargo + dx for the
          # `cargo run --release -p task-server` and `dx serve`
          # webServer entries that `playwright.config.js` boots.
          #
          # NixOS-specific bit: the binary `npx playwright install`
          # downloads is not patchelf'd for our libs, so we point
          # Playwright at `playwright-driver.browsers` instead and
          # disable its own download path. The JS-side
          # `@playwright/test` version must roughly match the
          # `playwright-driver` rev in nixpkgs — if they drift you'll
          # see "Executable doesn't exist" / "browser version
          # mismatch" errors; bump one to match the other.
          devShells.playwright = pkgs.mkShell {
            inputsFrom = [ inputs.dioxus-flake.devShells.${system}.default ];
            packages = with pkgs; [
              nodejs_22
              playwright-driver.browsers
            ];
            shellHook = ''
              export PLAYWRIGHT_BROWSERS_PATH=${pkgs.playwright-driver.browsers}
              export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=true
              export PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=true
            '';
          };
        };
    };
}
