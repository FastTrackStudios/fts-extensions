{
  description = "fts-plug — FastTrackStudio fork of nice-plug (Rust audio plugin framework) with a Dioxus/Blitz GUI backend";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  nixConfig = {
    extra-trusted-public-keys = [
      "fasttrackstudio.cachix.org-1:r7v7WXBeSZ7m5meL6w0wttnvsOltRvTpXeVNItcy9f4="
    ];
    extra-substituters = [
      "https://fasttrackstudio.cachix.org"
    ];
  };

  outputs =
    { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachSystem
      [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ]
      (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };

          isLinux = pkgs.stdenv.hostPlatform.isLinux;
          # nice-plug's transitive graph (vello_common etc.) needs a recent
          # toolchain; the workspace declares rust-version 1.87 but some deps
          # ask for 1.88+. Stable-latest covers both.
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
          };

          # System libraries the Dioxus/Blitz/Vello/wgpu/baseview graph links or
          # dlopens at build and run time. Linux-only; Darwin uses the SDK.
          linuxNativeLibs = with pkgs; [
            # OpenSSL (-sys crate in the dev-tools / hot-reload path)
            openssl

            # GPU / text rendering for Blitz + Vello + wgpu
            fontconfig
            freetype
            libGL
            vulkan-loader
            mesa

            # X11 + XCB (baseview fork, winit X11 backend)
            xorg.libX11
            xorg.libXcursor
            xorg.libXrandr
            xorg.libXi
            xorg.libXext
            xorg.libXrender
            xorg.libxcb
            xorg.xcbutil
            xorg.xcbutilcursor
            xorg.xcbutilkeysyms
            xorg.xcbutilwm
            libxkbcommon

            # Wayland backend (winit/softbuffer)
            wayland

            # bindgen (blitz transitive)
            llvmPackages.libclang

            # Audio backends for the standalone wrapper + examples (cpal/alsa, jack, midir)
            alsa-lib
            libjack2
            dbus
            # PipeWire (libspa-sys) for FTS-EQ's live spectrum-analyzer capture.
            pipewire

            # misc system libs commonly needed by GUI/audio crates
            zlib
            stdenv.cc.cc.lib
          ];
        in
        {
          devShells.default = pkgs.mkShell {
            packages = [
              rustToolchain
              pkgs.pkg-config
              pkgs.cargo-nextest
            ]
            ++ pkgs.lib.optionals isLinux linuxNativeLibs
            ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              pkgs.apple-sdk_15
            ];

            # pkg-config needs the libs on its search path at build time.
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = pkgs.lib.optionals isLinux linuxNativeLibs;

            env = pkgs.lib.optionalAttrs isLinux {
              LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
              # Software-rasterized GL/Vulkan so wgpu can grab an adapter headless
              # (e.g. render_screenshot()). Real GPUs still work via the host ICD.
              VK_ICD_FILENAMES = "${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.x86_64.json";
            };

            shellHook = pkgs.lib.optionalString isLinux ''
              # dlopened GUI/GPU libs (libGL, vulkan, xkbcommon, wayland, xcb-util…)
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath linuxNativeLibs}:/run/opengl-driver/lib:''${LD_LIBRARY_PATH:-}"
              # bindgen (libspa-sys for PipeWire) can't find clang's builtin / libc
              # headers on Nix by default — feed it the cc wrapper's flags + the
              # clang resource-dir includes.
              export BINDGEN_EXTRA_CLANG_ARGS="$(< ${pkgs.stdenv.cc}/nix-support/libc-crt1-cflags) \
                $(< ${pkgs.stdenv.cc}/nix-support/libc-cflags) \
                $(< ${pkgs.stdenv.cc}/nix-support/cc-cflags) \
                -idirafter ${pkgs.llvmPackages.libclang.lib}/lib/clang/${pkgs.lib.versions.major pkgs.llvmPackages.libclang.version}/include \
                ''${BINDGEN_EXTRA_CLANG_ARGS:-}"
            '';
          };
        }
      );
}
