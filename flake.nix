{
  description = "fts-ui — FastTrackStudio UI design system";
  inputs = {
    dioxus-flake.url = "github:FastTrackStudios/Dioxus-Flake";
    nixpkgs.follows = "dioxus-flake/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.follows = "dioxus-flake/rust-overlay";
    # Shared FTS hygiene hub — pinned capn/tracey + cargo xtask CI battery.
    fts-repo.url = "git+https://git.starcommand.live/FastTrackStudios/fts-repo";
    fts-repo.inputs.nixpkgs.follows = "nixpkgs";
  };
  nixConfig = {
    extra-trusted-public-keys = [ "fasttrackstudio.cachix.org-1:r7v7WXBeSZ7m5meL6w0wttnvsOltRvTpXeVNItcy9f4=" ];
    extra-substituters = [ "https://fasttrackstudio.cachix.org" ];
  };
  outputs = { self, dioxus-flake, nixpkgs, flake-utils, rust-overlay, fts-repo, }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default;
        cargoLock = {
          lockFile = ./Cargo.lock;
          outputHashes = {
            "lucide-dioxus-2.26.0" = "sha256-baNzMCjfJ1k9dNhTval9OrUby1cq7073eRlofKn2LI4=";
          };
        };

        ftsUiSrc = pkgs.runCommand "fts-ui-src" { } ''
          set -euo pipefail

          mkdir -p "$out/crates"
          cp -r ${./Cargo.lock} "$out/Cargo.lock"
          cp -r ${./crates/fts-ui} "$out/crates/fts-ui"

          cat > "$out/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/fts-ui"]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
authors = ["FastTrack Studio"]
license = "MIT OR Apache-2.0"

[workspace.dependencies]
dioxus = { version = "0.7.6", default-features = false, features = ["lib"] }
lucide-dioxus = { git = "https://github.com/Leaf-Computer/lucide.git", rev = "e3e509d5855324e893aef838747cccb2d115dc15", features = ["all-icons"] }
tracing = { version = "0.1", features = ["std"] }

[workspace.lints.rust]
unused = "warn"
EOF

          chmod -R u+w "$out"
        '';

        commonRustArgs = {
          version = "0.1.0";
          src = ftsUiSrc;
          inherit cargoLock;
          nativeBuildInputs = with pkgs; [ pkg-config ];
          buildInputs = with pkgs; [ openssl ];
        };
      in {
        packages = {
          fts-ui = pkgs.rustPlatform.buildRustPackage (commonRustArgs // {
            pname = "fts-ui";
            cargoBuildFlags = [ "--package" "fts-ui" ];
            cargoTestFlags = [ "--package" "fts-ui" ];
          });

          default = self.packages.${system}.fts-ui;
        };

        checks = {
          fts-ui = self.packages.${system}.fts-ui;
          default = self.packages.${system}.fts-ui;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ dioxus-flake.devShells.${system}.default ];
          shellHook = ''
            alias fts-build='cargo build --workspace'
            alias fts-test='cargo test --workspace'
            alias fts-check='cargo check --workspace'
            alias fts-clippy='cargo clippy --workspace -- -D warnings'
            alias fts-showcase-web='cd apps/web && dx serve --platform web'
            alias fts-showcase-desktop='cd apps/desktop && dx serve --platform desktop'
            alias fts-showcase-native='cd apps/native && dx serve --platform native'

            echo ""
            echo "  fts-ui dev shell"
            echo "  ----------------"
            echo "  fts-build     cargo build --workspace"
            echo "  fts-test      cargo test --workspace"
            echo "  fts-check     cargo check --workspace"
            echo "  fts-clippy    cargo clippy --workspace -- -D warnings"
            echo "  fts-showcase-web       cd apps/web && dx serve --platform web"
            echo "  fts-showcase-desktop   cd apps/desktop && dx serve --platform desktop"
            echo "  fts-showcase-native    cd apps/native && dx serve --platform native"
            echo ""
          '';
        };

        # CI shell — `cargo xtask ci` gate. dioxus-flake supplies rust + the
        # blitz/native build deps; layer on the shared hygiene tools.
        devShells.ci = pkgs.mkShell {
          inputsFrom = [ dioxus-flake.devShells.${system}.default ];
          buildInputs = [
            pkgs.cargo-nextest
            pkgs.cargo-shear
            pkgs.git-cliff
            pkgs.just
            fts-repo.packages.${system}.capn
            fts-repo.packages.${system}.tracey
          ];
        };

        devShells.mobile = pkgs.mkShell {
          inputsFrom = [ dioxus-flake.devShells.${system}.mobile ];
          shellHook = ''
            alias fts-build-mobile='cd apps/mobile && dx build --platform android'
            alias fts-showcase-mobile='cd apps/mobile && dx serve --platform android'

            echo ""
            echo "  fts-ui mobile shell"
            echo "  -------------------"
            echo "  fts-build-mobile      cd apps/mobile && dx build --platform android"
            echo "  fts-showcase-mobile   cd apps/mobile && dx serve --platform android"
            echo ""
          '';
        };
      }
    );
}
