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
          # One shell, everything in it. Layers on top of dioxus-flake's
          # default (full Linux GTK + WebView deps for `apps/desktop` +
          # capn pre-push) plus the wasm C toolchain required by
          # arborium-sysroot when building the editor for the browser.
          #
          # Why the unwrapped clang/llvm-bintools:
          # - `arborium-sysroot`'s build.rs compiles a tiny wasm
          #   sysroot through `cc::Build`. It needs a clang that
          #   targets `wasm32-unknown-unknown` and an `llvm-ar` that
          #   produces wasm archives.
          # - nix's `cc-wrapper` injects host-only hardening flags
          #   (notably `-fzero-call-used-regs=used-gpr`) that clang
          #   rejects for wasm. The *-unwrapped variants skip that
          #   wrapper. Same fix the editor's standalone flake used —
          #   ported verbatim per upstream's note.
          # - The CC_/AR_ env vars pin `cc::Build` to these binaries
          #   for the wasm target only; native builds still go through
          #   the wrapped gcc/clang.
          # The mobile shell stays separate because the Android NDK
          # toolchain it pulls in is heavy and only matters for that
          # surface.
          devShells.default = pkgs.mkShell {
            inputsFrom = [ inputs.dioxus-flake.devShells.${system}.default ];
            packages = with pkgs; [
              llvmPackages.clang-unwrapped
              llvmPackages.bintools-unwrapped
              # `lance-encoding` / `lance-file` (the LanceDB
              # crate stack that backs `wiki-search`'s
              # `vector` feature) invokes `prost-build` to
              # generate Rust from `.proto` schemas. Without
              # `protoc` on PATH the build fails with
              # `NotFound { kind: NotFound, error: "Could not
              # find protoc" }`.
              protobuf
            ];
            shellHook = ''
              export CC_wasm32_unknown_unknown=${pkgs.llvmPackages.clang-unwrapped}/bin/clang
              export AR_wasm32_unknown_unknown=${pkgs.llvmPackages.bintools-unwrapped}/bin/llvm-ar
              export PROTOC=${pkgs.protobuf}/bin/protoc
            '';
          };
          devShells.mobile = inputs.dioxus-flake.devShells.${system}.mobile;

          # ── Playwright shell ───────────────────────────────────────
          #
          # Browser-testing surface used by `features/editor/tests/`
          # (the editor playground spec) and any future
          # `tests/playwright/` suites. Layers nodejs + pnpm + just +
          # the Nix-managed Chromium on top of the default shell so
          # we still get cargo + dx + the wasm C toolchain for
          # `dx serve --package playground --platform web`, which is
          # what `playwright.config.js`'s `webServer` boots.
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
            inputsFrom = [ self'.devShells.default ];
            packages = with pkgs; [
              nodejs_22
              pnpm
              just
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
