#!/usr/bin/env bash
#
# Run the REAPER integration tests on a private, screenshottable display.
#
#   ./scripts/reaper-test-visual.sh [test-name-filter]
#
# Why this exists:
#
#   * Headless (`DISPLAY=""`) cannot open a Dioxus panel at all — the
#     window creation aborts REAPER inside GDK and takes the daw socket
#     with it, so every later test in the run dies on a socket timeout.
#   * `--virtual` wants an `fts-test` Xvfb launcher that is not installed
#     everywhere, and silently falls back to headless when it is missing.
#
# So: bring up our own Xvfb, run a real window manager on it, and drive
# the suite with `--gui` (which inherits DISPLAY). Nothing touches the
# developer's own desktop.
#
# The window manager is the part that is easy to skip and shouldn't be.
# A bare Xvfb has no WM, so windows are unmanaged: nothing positions
# them, nothing stacks them, and REAPER's Actions List ends up covering
# the screen with the panel buried underneath. openbox makes the root
# capture actually represent what a user would see.
#
# Screenshots land in target/reaper-shots/.

set -uo pipefail

FILTER="${1:-}"
DISP="${FTS_TEST_DISPLAY:-:99}"
GEOM="${FTS_TEST_GEOMETRY:-1920x1200x24}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
OUT="$ROOT/target/reaper-shots"
INTERVAL="${FTS_SHOT_INTERVAL:-0.5}"

mkdir -p "$OUT"
rm -f "$OUT"/*.png

cleanup() {
  [[ -n "${CAP_PID:-}" ]] && kill "$CAP_PID" 2>/dev/null
  [[ -n "${WM_PID:-}" ]] && kill "$WM_PID" 2>/dev/null
  # Only tear down the Xvfb we started ourselves.
  [[ -n "${XVFB_PID:-}" ]] && kill "$XVFB_PID" 2>/dev/null
  wait 2>/dev/null
}
trap cleanup EXIT

if [[ -e "/tmp/.X11-unix/X${DISP#:}" ]]; then
  echo "  Reusing existing display $DISP"
else
  echo "  Starting Xvfb on $DISP ($GEOM)"
  Xvfb "$DISP" -screen 0 "$GEOM" -nolisten tcp >"$OUT/xvfb.log" 2>&1 &
  XVFB_PID=$!
  sleep 2
fi

echo "  Starting openbox"
DISPLAY="$DISP" openbox >"$OUT/wm.log" 2>&1 &
WM_PID=$!
sleep 1

echo "  Capturing to $OUT every ${INTERVAL}s"
(
  i=0
  while true; do
    i=$((i + 1))
    import -display "$DISP" -window root "$OUT/$(printf %04d $i).png" 2>/dev/null
    sleep "$INTERVAL"
  done
) &
CAP_PID=$!

echo "  Running suite${FILTER:+ (filter: $FILTER)}"
DISPLAY="$DISP" cargo run -p fts-extensions-xtask -- --gui ${FILTER:+"$FILTER"}
STATUS=$?

sleep 1
kill "$CAP_PID" 2>/dev/null
CAP_PID=

# Blank frames are the majority — REAPER takes a while to map anything.
# Keep only frames with real content so the output is worth opening.
kept=0
for f in "$OUT"/*.png; do
  [[ -e "$f" ]] || continue
  mean=$(identify -format '%[mean]' "$f" 2>/dev/null || echo 0)
  if (( $(echo "$mean < 300" | bc -l 2>/dev/null || echo 0) )); then
    rm -f "$f"
  else
    kept=$((kept + 1))
  fi
done

echo "  Kept $kept non-blank frames in $OUT"
exit $STATUS
