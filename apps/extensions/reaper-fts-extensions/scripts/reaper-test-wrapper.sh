#!/usr/bin/env bash
# Wrapper that runs the current fts-dev REAPER through the NixOS FHS environment.
# Used by the test harness, which expects a single executable.
#
# For virtual display testing (Xvfb), the test harness clears DISPLAY
# (headless mode). FTS_VIRTUAL_DISPLAY carries the original Xvfb display.
set -euo pipefail

if [ -n "${FTS_VIRTUAL_DISPLAY:-}" ] && [ -z "${DISPLAY:-}" ]; then
    export DISPLAY="$FTS_VIRTUAL_DISPLAY"
fi

FTS_DEV="${FTS_DEV:-$HOME/.fts-dev}"
CONFIG="${FTS_DEV}/launch.json"

if [ ! -f "$CONFIG" ]; then
    fts-dev --setup
fi

REAPER_EXECUTABLE="$(sed -n 's/.*"reaper_executable"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$CONFIG" | head -n 1)"

if [ -z "$REAPER_EXECUTABLE" ]; then
    echo "Could not read reaper_executable from $CONFIG" >&2
    exit 1
fi

REAPER_FHS="${FTS_REAPER_FHS:-}"
if [ -z "$REAPER_FHS" ]; then
    REAPER_FHS="$(sed -n 's/.*FTS_REAPER_FHS="\([^"]*\)".*/\1/p' "$(command -v fts-dev)" | head -n 1)"
fi

if [ -n "$REAPER_FHS" ]; then
    exec "$REAPER_FHS" "$REAPER_EXECUTABLE" "$@"
fi

exec "$REAPER_EXECUTABLE" "$@"
