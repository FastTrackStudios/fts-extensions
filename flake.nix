{
  description = "Keyflow development shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    # Shared FTS repo-hygiene hub: pinned capn/tracey + cargo xtask CI battery.
    fts-repo.url = "git+https://git.starcommand.live/FastTrackStudios/fts-repo";
    fts-repo.inputs.nixpkgs.follows = "nixpkgs";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Shared Dioxus flake — bundles `dx`, the rust toolchain + wasm32
    # target, and the wasm-bindgen that matches our dioxus fork
    # (codywright/dioxus, pinned in Cargo.toml's `[patch.crates-io]`), plus
    # the GTK/WebView base deps. Same input Task + Editor use, so the whole
    # FTS stack shares one `dx` + wasm-bindgen. This is what gives the web
    # editor "the right wasm-bindgen". Pointed at the local checkout; switch
    # to `github:FastTrackStudios/Dioxus-Flake` for a clean clone.
    dioxus-flake = {
      url = "path:/home/cody/Development/Dioxus/dioxus-flake";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-overlay.follows = "rust-overlay";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      fts-repo,
      dioxus-flake,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };

        # Toolchain (rust + wasm32 target) and `dx` come from here.
        dioxusShell = dioxus-flake.devShells.${system}.default;

        # wasm-bindgen-cli pinned to EXACTLY the version our dioxus fork uses
        # (`wasm-bindgen 0.2.122` in Cargo.lock). The Dioxus shell ships a
        # different version, and dx refuses to run a mismatched wasm-bindgen
        # (and downloading one fails to link on NixOS), so we put the matching
        # binary on PATH. Bump `version` when Cargo.lock's wasm-bindgen moves
        # (nix prints the new `hash`/`cargoHash` on the first build).
        wasm-bindgen-cli = pkgs.rustPlatform.buildRustPackage rec {
          pname = "wasm-bindgen-cli";
          version = "0.2.122";
          src = pkgs.fetchCrate {
            inherit pname version;
            hash = "sha256-vO4RSxi/sMWxmsEs3GuljdMfIRSu75A+Q+c5wgYToRU=";
          };
          cargoHash = "sha256-Inup6vvJSG5ghNyeDPyZbfZo4d0LsMG2OJfStoaeDBs=";
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.openssl ];
          doCheck = false;
        };

        linuxGuiPackages =
          with pkgs;
          lib.optionals stdenv.isLinux [
            atk
            cairo
            gdk-pixbuf
            glib
            gtk3
            libsoup_3
            pango
            webkitgtk_4_1
            xdotool
            libx11
            libxcb
            libxcursor
            libxi
            libxkbcommon
            libxrandr
            wayland
            libGL
            vulkan-loader
          ];
      in
      {
        packages.wasm-bindgen-cli = wasm-bindgen-cli;

        devShells.default = pkgs.mkShell {
          # The rust toolchain (with the wasm32 target), `dx`, and the
          # wasm-bindgen matching our dioxus fork come from the shared
          # Dioxus shell — so `dx serve --package web-editor --platform web`
          # builds the editor for the browser. keyflow layers on its native
          # GUI/GPU deps (keyflow-ui's dioxus-native/Blitz + wgpu/vello) and
          # the wasm C toolchain below.
          inputsFrom = [ dioxusShell ];
          packages =
            with pkgs;
            [
              # Must come before the Dioxus shell's wasm-bindgen on PATH.
              wasm-bindgen-cli
              cmake
              fontconfig
              freetype
              nodejs_22
              openssl
              pkg-config
              pnpm
              # clang + llvm give a wasm-capable C cross-compiler so
              # `cc::Build` crates — notably `arborium-sysroot`, which ships
              # the wasm sysroot for the editor's tree-sitter grammars —
              # actually emit wasm objects. Without these, `cc` falls back to
              # host gcc, produces x86-64 ELF, the wasm linker drops them, and
              # the wasm has unresolved `env` imports at load time. The
              # *unwrapped* variants skip nix's cc-wrapper, whose host-only
              # hardening flags (e.g. `-fzero-call-used-regs`) clang rejects
              # for the wasm target.
              llvmPackages.clang-unwrapped
              llvmPackages.bintools-unwrapped
              # `wasm-opt` — dx runs it on release web builds.
              binaryen
            ]
            ++ linuxGuiPackages;

          shellHook = ''
            export RUST_BACKTRACE=1
            export OPENSSL_DIR=${pkgs.openssl.dev}
            export OPENSSL_LIB_DIR=${pkgs.openssl.out}/lib
            # Pin the `cc` crate's wasm32 C compiler/archiver to the unwrapped
            # clang/llvm-ar (arborium-sysroot's build.rs). Native builds still
            # go through the wrapped gcc/clang.
            export CC_wasm32_unknown_unknown=${pkgs.llvmPackages.clang-unwrapped}/bin/clang
            export AR_wasm32_unknown_unknown=${pkgs.llvmPackages.bintools-unwrapped}/bin/llvm-ar
          '';
        };

        # CI shell — `cargo xtask ci` gate. Reuses the default shell and
        # layers on the shared hygiene tools. keyflow pulls blitz/fts-ui
        # transitively, so include the fts-ui GUI/GPU build inputs too.
        devShells.ci = pkgs.mkShell {
          inputsFrom = [ self.devShells.${system}.default ];
          buildInputs = [
            pkgs.cargo-nextest
            pkgs.cargo-shear
            pkgs.git-cliff
            pkgs.just
            fts-repo.packages.${system}.capn
            fts-repo.packages.${system}.tracey
          ] ++ fts-repo.lib.ftsUiBuildInputs pkgs;
        };
      }
    );
}
