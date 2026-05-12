# architect dev recipes
#
# Run from the repo root inside the Nix dev shell:
#   nix develop -c just <recipe>
#
# Most recipes delegate to `cargo xtask`, which owns the actual logic in
# Rust (see xtask/src/main.rs). Anything that needs to outlive the
# server process (the wasm e2e dance) still lives here as a shell
# recipe — xtask shells out to these.

# Default: full check across workspace + target-cfg crates.
default: check

# Type-check workspace + the target-cfg-only crates.
check:
    cargo xtask check
    cd apps/app/ui && cargo check
    cd apps/app/web && cargo check --target wasm32-unknown-unknown
    cd apps/app/desktop && cargo check

# nextest with the default profile.
test:
    cargo xtask test

# Workspace + clippy + fmt --check + nextest CI profile.
ci:
    cargo xtask ci

# Build + run the migration binary.
migrate:
    cargo run -p app-db -- up

# Run the axum + vox server. Migrations auto-apply on boot.
server:
    cargo run -p app-server

# Run the wasm browser integration tests against an already-running server.
test-wasm:
    cd features/example/tests/web && cargo test --target wasm32-unknown-unknown --release

# Browser e2e against the default (sqlite) backend.
test-e2e: (_e2e "")

# Same e2e against the in-memory backend — proves the contract is
# backend-agnostic (wasm tests don't change).
test-e2e-memory: (_e2e "--no-default-features --features backend-memory")

# Internal: build + run server with given cargo features, drive wasm tests.
_e2e features:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build -p app-server {{features}}
    rm -f example.db
    ./target/debug/app-server &
    server_pid=$!
    trap "kill $server_pid 2>/dev/null || true; rm -f example.db" EXIT
    for i in {1..30}; do
        if curl -fsS http://127.0.0.1:4040/api/health >/dev/null 2>&1; then break; fi
        sleep 0.2
    done
    cd features/example/tests/web && cargo test --target wasm32-unknown-unknown --release

# `dx serve` the web app — connects to the server on 4040 by default.
web:
    cd apps/app/web && dx serve --web --addr 0.0.0.0 --port 8765

# `dx serve` the desktop app.
desktop:
    cd apps/app/desktop && dx serve --desktop

# ── CLI client ─────────────────────────────────────────────────────────

# Invoke the `app` CLI client. Pass subcommand + args after `--`.
#   just cli -- list
#   just cli -- create --name foo
cli *args:
    cargo run -p app-cli -- {{args}}

# ── Docs ──────────────────────────────────────────────────────────────

# Serve the dodeca docs site locally with live reload.
# Reads .config/dodeca.styx for paths; run from the repo root.
docs:
    ddc serve

# Build the dodeca docs site for production.
docs-build:
    ddc build

# Sync docs/content/ → the Forgejo wiki repo.
sync-wiki:
    cargo xtask wiki sync

sync-wiki-dry-run:
    cargo xtask wiki sync --dry-run

# ── Tracey ────────────────────────────────────────────────────────────

# Validate spec ↔ impl ↔ verify links. Fails on unmapped rules.
tracey-validate:
    cargo xtask tracey-validate

# Coverage overview (what's tested, what isn't).
tracey-status:
    tracey query status

# ── Releases / changelog ──────────────────────────────────────────────

# Regenerate CHANGELOG.md from conventional commits.
changelog:
    git cliff -o CHANGELOG.md

# Preview release notes for the next bump (no file write).
changelog-preview:
    git cliff --unreleased

# Install git hooks (capn pre-commit + pre-push + tracey).
install-hooks:
    ./hooks/install.sh

# Run capn pre-commit checks manually (without committing).
capn-precommit:
    capn

# Run capn pre-push checks manually (without pushing).
capn-prepush:
    capn pre-push

# Format all Rust files in the workspace + target-cfg crates.
fmt:
    cargo fmt --all
    cd apps/app/ui && cargo fmt
    cd apps/app/web && cargo fmt
    cd apps/app/desktop && cargo fmt
