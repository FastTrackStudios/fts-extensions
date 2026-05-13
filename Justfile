# Task workspace recipes
# Run commands: just <recipe-name>

# Default: check the core workspace
default: check

# ── CLI ──────────────────────────────────────────────────────────────────

# Run the task CLI
task *args:
    cargo run -p task-cli -- {{args}}

# ── Web (dev) ────────────────────────────────────────────────────────────

# Run the Dioxus dev server for apps/web on a fixed port + host so the
# starcommand task-dev.starcommand.live tunnel reaches it.
#
# - Enters the `.#ui` dev shell (re-exports dioxus-flake's default shell)
#   so dx, rustup with the wasm32-unknown-unknown target, and the rest
#   of the Dioxus toolchain are on PATH.
# - Binds 0.0.0.0 so the starcommand nginx reverse proxy reaches it via
#   the 10G LAN (10.10.10.10); port 8765 matches the upstream the
#   task-dev nginx vhost forwards to.
# - `--wasm-split` + `--features wasm-split` enable route-level bundle
#   splitting so the initial wasm payload only contains the visited
#   route's code. Each Route variant becomes its own lazy-loaded chunk.
# - When this is running you can visit task-dev.starcommand.live from
#   any device. When it's not, nginx serves a friendly offline page.
task-web-dev:
    nix develop .#ui --command bash -c 'cd apps/web && dx serve --web --addr 0.0.0.0 --port 8765 --wasm-split --features wasm-split'

# ── Build & Test ─────────────────────────────────────────────────────────

# Every workspace recipe runs inside `nix develop .#ui` so the
# dioxus-desktop pango/gtk system libs + the wasm32 target are
# available. Drops back to plain `cargo` for hosts that already
# have the toolchain on PATH (CI runners, direnv users).
check:
    nix develop .#ui --command cargo check --workspace

build:
    nix develop .#ui --command cargo build --workspace

test:
    nix develop .#ui --command cargo test --workspace

# ── Loro sync demo ───────────────────────────────────────────────────────
#
# Two recipes; run them in two terminals:
#   1. `just sync-demo-server` → relay on :9090, in-memory sqlite,
#      pre-seeded with ~1700 fake rows across every feature so the
#      UI has realistic content on first load.
#   2. `just task-web-dev` (existing) → Dioxus dev server on :8765.
#      Open http://localhost:8765/<feature-route> in two browsers.
sync-demo-server:
    SYNC_DEMO_SEED=1 SYNC_DEMO_BIND="0.0.0.0:9090" \
        cargo run --release -p sync-demo

# Pre-seed only (without starting the server). Useful when you want
# to inspect what `task-db` does without keeping a process bound to
# the port. Defaults to in-memory sqlite so this is mostly a
# debugging aid; the snapshot is gone when the process exits.
seed:
    cargo run --release -p task-db -- seed

# ── Lint / format / CI ───────────────────────────────────────────────────

fmt:
    nix develop .#ui --command cargo fmt --all

clippy:
    nix develop .#ui --command cargo clippy --workspace --all-targets -- -D warnings

ci:
    nix develop .#ui --command bash -c '\
        cargo fmt --all -- --check && \
        cargo clippy --workspace --all-targets -- -D warnings && \
        cargo nextest run --workspace --profile ci'

# ── Git hooks (capn) ─────────────────────────────────────────────────────

# Install the capn pre-commit + pre-push hooks. Run once per clone.
install-hooks:
    ./hooks/install.sh

# Run capn pre-commit checks manually (without committing).
capn-precommit:
    capn

# Run capn pre-push checks manually (without pushing).
capn-prepush:
    capn pre-push

# ── Releases / changelog ─────────────────────────────────────────────────

# Regenerate CHANGELOG.md from conventional commits.
changelog:
    git cliff -o CHANGELOG.md

# Preview release notes for the next bump (no file write).
changelog-preview:
    git cliff --unreleased

# ── Aliases ──────────────────────────────────────────────────────────────

alias c := check
alias b := build
alias t := test

# ── Deploy ───────────────────────────────────────────────────────────────

# Build task-cli (release) and ship it to starcommand for the
# task-email-watcher systemd service. The binary is placed at
# /var/lib/task-watcher/bin/task and the watcher is restarted.
#
# Called automatically from ~/.starcommand/justfile `deploy`, so
# `just deploy` in starcommand does the whole pipeline.
deploy-task-watcher host="root@192.168.0.106" remote="/var/lib/task-watcher/bin/task":
    cargo build --release -p task-cli
    scp target/release/task {{host}}:{{remote}}.new
    ssh {{host}} 'install -o task-watcher -g task-watcher -m 0755 {{remote}}.new {{remote}} && rm -f {{remote}}.new && systemctl restart task-email-watcher.service && sleep 2 && systemctl status task-email-watcher.service --no-pager | head -8'
