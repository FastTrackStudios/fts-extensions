# fts-extensions — root workspace recipes
# Run commands: just <recipe-name>

# List recipes by default
default:
    @just --list

# ── REAPER extension (apps/extensions/reaper-fts-extensions) ────────────
# The production REAPER extension (cdylib reaper_fts_extensions). Recipes
# live in the `reaper` module:
#   just reaper build                  build the release cdylib
#   just reaper install                build + symlink into $FTS_REAPER_HOME
#   just reaper uninstall              remove the symlink
#   just reaper log                    tail the live extension log
#   just reaper integration-test       in-REAPER integration suite (headless)
#   just reaper integration-test-gui   same, visible window
mod reaper 'apps/extensions/reaper-fts-extensions/reaper.just'

# ── Build ────────────────────────────────────────────────────────────────

# Check the whole workspace compiles
check:
    cargo check --workspace

# Run tests. nextest: parallel per-test binaries, much faster than
# `cargo test` on this many crates. It does NOT run doctests — use
# `just test-doc` for those.
test:
    cargo nextest run --workspace

# Doctests only — nextest can't run them (libtest owns doctests).
test-doc:
    cargo test --workspace --doc

# ── Disk / build-time hygiene ────────────────────────────────────────────
# Cargo never garbage-collects target/: every rebuild with a changed
# fingerprint leaves the old artifact behind forever.

# Reclaim stale artifacts in THIS worktree (keeps anything touched recently).
sweep days="7":
    cargo sweep --time {{days}}
    @du -sh target

# Sweep every worktree — the thing to run when the dev disk fills up.
# Uses `git worktree list` so new worktrees are picked up automatically.
sweep-all days="7":
    #!/usr/bin/env bash
    set -euo pipefail
    before=$(du -sc $(git worktree list --porcelain | awk '/^worktree /{print $2"/target"}') 2>/dev/null | tail -1 | cut -f1)
    for w in $(git worktree list --porcelain | awk '/^worktree /{print $2}'); do
      [ -d "$w/target" ] || continue
      echo "── sweeping $w"
      cargo sweep --time {{days}} "$w" || true
    done
    after=$(du -sc $(git worktree list --porcelain | awk '/^worktree /{print $2"/target"}') 2>/dev/null | tail -1 | cut -f1)
    echo "reclaimed $(( (before - after) / 1024 / 1024 )) GiB"

# Drop incremental-compilation caches everywhere. They are pure cache —
# safe to delete, costs one non-incremental rebuild.
sweep-incremental:
    #!/usr/bin/env bash
    set -euo pipefail
    for w in $(git worktree list --porcelain | awk '/^worktree /{print $2}'); do
      rm -rf "$w"/target/*/incremental "$w"/target/incremental 2>/dev/null || true
    done
    echo "incremental caches cleared"

# Where is the disk actually going? Per-worktree target/ sizes, largest first.
disk:
    #!/usr/bin/env bash
    du -sh $(git worktree list --porcelain | awk '/^worktree /{print $2"/target"}') 2>/dev/null | sort -rh

# Why is the build slow? Writes target/cargo-timings/cargo-timing.html —
# a per-crate Gantt chart showing the critical path and link-time tail.
timings *ARGS:
    cargo build --timings {{ARGS}}
    @echo "→ target/cargo-timings/cargo-timing.html"

# ── Knowledge graph (graphify) ───────────────────────────────────────────
# Whole-repo knowledge graph for AI assistants — parses the tree with
# tree-sitter (100% local, no API calls) into graphify-out/ (graph.json +
# GRAPH_REPORT.md + interactive graph.html). graphify is bootstrapped in the
# nix dev shell (see flake.nix shellHook). Output is gitignored + regenerable;
# rebuild after large structural changes. `graph-serve` exposes it over MCP
# (wired into .mcp.json so Claude Code queries it instead of grepping cold).

# Build/refresh the repo knowledge graph (local AST + clustering, no LLM).
# --force so the graph shrinks when .graphifyignore excludes more (vendored
# trees); without it graphify refuses a rebuild that has fewer nodes.
graph:
    graphify update . --force

# Serve the knowledge graph over MCP (stdio) — used by .mcp.json
graph-serve:
    graphify-mcp --transport stdio --graph graphify-out/graph.json

# ── Aliases ──────────────────────────────────────────────────────────────

alias c := check
alias t := test
