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
# - Binds 0.0.0.0 so a remote reverse-proxy (starcommand nginx via
#   Tailscale) can connect; port 8765 matches the upstream the
#   task-dev nginx vhost forwards to.
# - When this is running you can visit task-dev.starcommand.live from
#   any device. When it's not, the URL returns the friendly 502 page
#   from starcommand nginx.
task-web-dev:
    cd apps/web && dx serve --web --addr 0.0.0.0 --port 8765

# ── Build & Test ─────────────────────────────────────────────────────────

# Check all crates compile
check:
    cargo check --workspace

# Build all crates
build:
    cargo build --workspace

# Run tests
test:
    cargo test --workspace

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
