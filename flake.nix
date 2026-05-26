{
  description = "Keyflow development shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    # Shared FTS repo-hygiene hub: pinned capn/tracey + cargo xtask CI battery.
    fts-repo.url = "git+https://git.starcommand.live/FastTrackStudios/fts-repo";
    fts-repo.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      fts-repo,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
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
        devShells.default = pkgs.mkShell {
          packages =
            with pkgs;
            [
              cargo
              clippy
              cmake
              fontconfig
              freetype
              nodejs_22
              openssl
              pkg-config
              pnpm
              rustc
              rustfmt
            ]
            ++ linuxGuiPackages;

          shellHook = ''
            export RUST_BACKTRACE=1
            export OPENSSL_DIR=${pkgs.openssl.dev}
            export OPENSSL_LIB_DIR=${pkgs.openssl.out}/lib
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
