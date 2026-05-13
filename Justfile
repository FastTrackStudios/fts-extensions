# Task workspace recipes
# Run commands: just <recipe-name>

# Default: check the core workspace
default: check

# ── CLI ──────────────────────────────────────────────────────────────────

# Run the task CLI
task *args:
    cargo run -p task-cli -- {{args}}

# ── Web (dev) ────────────────────────────────────────────────────────────

# ── Run the app ──────────────────────────────────────────────────────────
#
# Three recipes for three terminals (or `just dev` to run them all):
#   1. `just server` → task-server on :9090, sync relay
#   2. `just web`    → Dioxus dev server on :8765
#   3. `just db`     → run migrations + seed fake data
#
# Defaults to in-memory sqlite — `just server` and `just db` populate
# their own process's database. For persistent data across runs, set
# `SYNC_DEMO_DATABASE_URL=sqlite://./data.db?mode=rwc` first.

# Dioxus dev server for apps/web on port 8765. Binds 0.0.0.0 so the
# starcommand nginx reverse proxy reaches it via the 10G LAN.
# `--wasm-split` enables route-level lazy chunks.
web:
    nix develop .#ui --command bash -c 'cd apps/web && dx serve --web --addr 0.0.0.0 --port 8765'

# Canonical server. Defaults: bind 0.0.0.0:9090, in-memory sqlite,
# seed-on-startup. Override via TASK_SERVER_{BIND,SEED} env vars.
server:
    TASK_SERVER_SEED=1 TASK_SERVER_BIND="0.0.0.0:9090" \
        cargo run --release -p task-server

# Run migrations + seed the workspace doc with fake data. Standalone
# CLI — useful for inspecting what `task-db` does without binding a
# port. Since the default sqlite is in-memory the snapshot dies when
# the process exits; set SYNC_DEMO_DATABASE_URL to a file URL for
# state that survives.
db:
    cargo run --release -p task-db -- all

# Launch server + web side-by-side; Ctrl+C kills both. Server lines
# prefixed [srv], web lines [web].
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'kill 0' EXIT
    TASK_SERVER_SEED=1 TASK_SERVER_BIND="0.0.0.0:9090" \
        cargo run --release -p task-server 2>&1 | sed 's/^/[srv] /' &
    just web 2>&1 | sed 's/^/[web] /' &
    wait

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
