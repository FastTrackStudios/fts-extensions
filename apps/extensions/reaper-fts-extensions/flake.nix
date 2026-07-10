{
  description = "FTS Extensions — unified REAPER extension (launcher, templates, session, sync, input, keyflow)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    devenv.url = "github:cachix/devenv";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    fts-flake.url = "github:FastTrackStudios/fts-flake/600ffd1c779864b896ad2ba44e7107d7aa59e832";
    fts-flake.inputs.nixpkgs.follows = "nixpkgs";
  };

  nixConfig = {
    extra-trusted-public-keys = [
      "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw="
      "fasttrackstudio.cachix.org-1:r7v7WXBeSZ7m5meL6w0wttnvsOltRvTpXeVNItcy9f4="
    ];
    extra-substituters = [
      "https://devenv.cachix.org"
      "https://fasttrackstudio.cachix.org"
    ];
  };

  outputs =
    {
      self,
      nixpkgs,
      devenv,
      flake-utils,
      rust-overlay,
      fts-flake,
    } @ inputs:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfreePredicate =
            pkg:
            builtins.elem (pkgs.lib.getName pkg) [
              "reaper"
              "reaper-headless"
            ];
        };

        ftsReaperConfig = "$HOME/.config/FastTrackStudio/Reaper";

        ftsDev = fts-flake.lib.mkFtsPackages {
          inherit pkgs;
          cfg = fts-flake.presets.dev // {
            reaper.configDir = ftsReaperConfig;
          };
        };
      in
      {
        devShells = {
          default = devenv.lib.mkShell {
            inherit inputs pkgs;
            modules = [
              (
                { pkgs, config, ... }:
                {
                  cachix.pull = [ "fasttrackstudio" ];

                  packages = with pkgs; [
                    # FTS REAPER environment
                    ftsDev.fts-test
                    ftsDev.fts-gui
                    ftsDev.reaper-fhs

                    # Build essentials
                    pkg-config
                    openssl
                    cmake

                    # ── Dioxus / Blitz / wgpu GUI dependencies ──

                    # X11 / windowing
                    libx11
                    libxi
                    libxext
                    libxrandr
                    libxcursor
                    libxinerama
                    libxcomposite
                    libxdamage
                    libxfixes
                    libxrender
                    libxtst
                    libxcb
                    libxscrnsaver
                    xdotool

                    # XKB (keyboard handling)
                    libxkbcommon

                    # GPU / Vulkan (wgpu backend)
                    vulkan-loader
                    vulkan-headers
                    vulkan-tools
                    libGL
                    mesa

                    # GTK / GLib (for file dialogs, clipboard)
                    gtk3
                    webkitgtk_4_1
                    libsoup_3
                    glib
                    gdk-pixbuf
                    pango
                    cairo
                    atk

                    # Wayland (optional secondary backend)
                    wayland
                    wayland-protocols

                    # Font rendering
                    fontconfig
                    freetype

                    # Audio libs
                    alsa-lib
                    pipewire.jack

                    # mDNS / network discovery (sync crate / avahi-sys)
                    avahi
                    avahi.dev

                    # C/C++ bindgen
                    llvmPackages.libclang

                    # Misc system libs
                    dbus
                    zlib
                    stdenv.cc.cc.lib
                  ];

                  languages.rust = {
                    enable = true;
                    channel = "stable";
                  };

                  env = {
                    FTS_REAPER_EXECUTABLE = "${ftsDev.reaper}/bin/reaper";
                    FTS_REAPER_RESOURCES = "${ftsDev.reaper}/opt/REAPER";
                    FTS_REAPER_CONFIG = ftsReaperConfig;
                    LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
                    BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.avahi.dev}/include -isystem ${pkgs.glibc.dev}/include";
                    LIBRARY_PATH = pkgs.lib.makeLibraryPath [
                      pkgs.avahi
                      pkgs.xdotool
                    ];
                    LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
                      pkgs.vulkan-loader
                      pkgs.libGL
                      pkgs.wayland
                      pkgs.libxkbcommon
                      pkgs.webkitgtk_4_1
                      pkgs.libsoup_3
                      pkgs.xdotool
                    ];
                  };

                  scripts = {
                    fts-build.exec = "cargo build --workspace";
                    fts-build.description = "Build the entire fts-extensions workspace";

                    fts-unit-test.exec = "cargo test --workspace";
                    fts-unit-test.description = "Run all unit tests";

                    fts-check.exec = "cargo check --workspace";
                    fts-check.description = "Type-check the workspace (faster than build)";

                    fts-clippy.exec = "cargo clippy --workspace -- -D warnings";
                    fts-clippy.description = "Run clippy lints";
                  };

                  enterShell = ''
                    echo ""
                    echo "  fts-extensions dev shell (devenv + fts-flake)"
                    echo "  ──────────────────────────────────────────────"
                    echo "  fts-build         — cargo build --workspace"
                    echo "  fts-unit-test     — cargo test --workspace"
                    echo "  fts-check         — cargo check --workspace"
                    echo "  fts-clippy        — run clippy lints"
                    echo ""
                    echo "  just install      — build + symlink to REAPER"
                    echo "  just integration-test — run live REAPER tests"
                    echo ""
                    echo "  REAPER: ${ftsDev.reaper}/bin/reaper"
                    echo ""
                  '';
                }
              )
            ];
          };
        };
      }
    );
}
