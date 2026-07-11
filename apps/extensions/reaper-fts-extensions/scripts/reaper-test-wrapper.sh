#!/usr/bin/env bash
# Wrapper that launches REAPER for the test harness, which expects a
# single executable (FTS_REAPER_EXECUTABLE points here).
#
# Resolution order for the REAPER binary:
#   1. $FTS_DEV/launch.json ("reaper_executable") when fts-dev has set up
#      a test profile (~/fts-dev by default)
#   2. `command -v reaper` (nix profile / system PATH)
#
# On NixOS the binary may need an FHS environment (reaper-env/bubblewrap);
# FTS_REAPER_FHS overrides it, otherwise it is scraped from the fts-dev
# launcher script when one is installed.
#
# For virtual display testing (Xvfb), the test harness clears DISPLAY
# (headless mode). FTS_VIRTUAL_DISPLAY carries the original Xvfb display.
set -euo pipefail

if [ -n "${FTS_VIRTUAL_DISPLAY:-}" ] && [ -z "${DISPLAY:-}" ]; then
    export DISPLAY="$FTS_VIRTUAL_DISPLAY"
fi

FTS_DEV="${FTS_DEV:-$HOME/fts-dev}"
CONFIG="${FTS_DEV}/launch.json"

REAPER_EXECUTABLE=""
if [ -f "$CONFIG" ]; then
    REAPER_EXECUTABLE="$(sed -n 's/.*"reaper_executable"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$CONFIG" | head -n 1)"
fi

# launch.json may reference a garbage-collected store path — fall back to PATH.
if [ -z "$REAPER_EXECUTABLE" ] || [ ! -x "$REAPER_EXECUTABLE" ]; then
    REAPER_EXECUTABLE="$(command -v reaper || true)"
fi

if [ -z "$REAPER_EXECUTABLE" ]; then
    echo "reaper-test-wrapper: no REAPER binary found (no $CONFIG and no \`reaper\` on PATH)" >&2
    exit 127
fi

REAPER_FHS="${FTS_REAPER_FHS:-}"
if [ -z "$REAPER_FHS" ]; then
    FTS_DEV_BIN="$(command -v fts-dev || true)"
    if [ -n "$FTS_DEV_BIN" ]; then
        REAPER_FHS="$(sed -n 's/.*FTS_REAPER_FHS="\([^"]*\)".*/\1/p' "$FTS_DEV_BIN" | head -n 1)"
    fi
fi

if [ -n "$REAPER_FHS" ] && [ -x "$REAPER_FHS" ]; then
    exec "$REAPER_FHS" "$REAPER_EXECUTABLE" "$@"
fi

exec "$REAPER_EXECUTABLE" "$@"
