#!/usr/bin/env bash
# Clone the FTS sibling-repo layout that fts-extensions and FastTrackStudio
# expect when their workspace Cargo.toml local-dev patches are active.
#
# Usage:
#   ./scripts/bootstrap-siblings.sh [target-parent-dir]
#
# If no target dir is given, defaults to one level above this repo:
#   ~/Development/FastTrackStudio/
#
# Idempotent: skips repos that already exist; pulls latest on the default
# branch otherwise. Missing-on-starcommand fallbacks (moire) clone from
# their canonical upstream.

set -euo pipefail

PARENT="${1:-$(cd "$(dirname "$0")/../.." && pwd)}"
mkdir -p "$PARENT"
cd "$PARENT"

STAR="ssh://forgejo@git.starcommand.live/FastTrackStudios"

# Sibling -> "url|branch". Most live on starcommand; a couple use upstream.
# Branch hints exist because some workspaces expect crates that only live on
# feature branches (daw's daw-module is only on pt-rpp-track-order-midi, etc).
declare -A REPOS=(
  [blitz]="$STAR/blitz.git|main"
  [daw]="$STAR/daw.git|pt-rpp-track-order-midi"
  [dioxus-launcher]="$STAR/dioxus-launcher.git|main"
  [dock-dioxus]="$STAR/dock-dioxus.git|main"
  [dynamic-template]="$STAR/dynamic-template.git|main"
  [fts-launcher]="$STAR/fts-launcher.git|main"
  [fts-story]="$STAR/fts-story.git|main"
  [fts-ui]="$STAR/fts-ui.git|main"
  [input]="$STAR/input.git|main"
  [input_actions]="$STAR/input_actions.git|main"
  [keyflow]="$STAR/keyflow.git|forgejo-main"
  [monarchy]="$STAR/monarchy.git|main"
  [reaper-file]="$STAR/reaper-file.git|main"
  [reaper-lib]="$STAR/reaper-lib.git|main"
  # session: starcommand main is branch-protected, so the matching state
  # for fts-extensions lives on a PR branch until that's merged.
  [session]="$STAR/session.git|update/initial-import-main"
  [moire]="https://github.com/bearcove/moire|main"
)

# Top-level workspaces the user will actually build.
declare -A TOPLEVELS=(
  [fts-extensions]="$STAR/fts-extensions.git|main"
  [FastTrackStudio]="$STAR/FastTrackStudio.git|migrate/new-patterns"
)

clone_or_update() {
  local name="$1" spec="$2"
  local url="${spec%|*}" branch="${spec#*|}"
  if [ -d "$name/.git" ]; then
    echo "==> $name (existing, fetching, checkout $branch)"
    # Pre-existing checkouts often point at github origin. Point origin at
    # the expected URL so subsequent fetches see all the branches we need.
    local current_origin
    current_origin="$(git -C "$name" remote get-url origin 2>/dev/null || true)"
    if [ "$current_origin" != "$url" ]; then
      if [ -n "$current_origin" ]; then
        git -C "$name" remote set-url origin "$url"
      else
        git -C "$name" remote add origin "$url"
      fi
    fi
    git -C "$name" fetch --quiet origin "$branch" || true
    git -C "$name" checkout --quiet "$branch" 2>/dev/null || \
      git -C "$name" checkout --quiet -b "$branch" "origin/$branch"
    # Fast-forward to remote tip if it has moved.
    git -C "$name" merge --quiet --ff-only "origin/$branch" 2>/dev/null || true
  else
    echo "==> $name (cloning $url, branch $branch)"
    git clone --quiet --branch "$branch" "$url" "$name"
  fi
}

for name in "${!REPOS[@]}"; do
  clone_or_update "$name" "${REPOS[$name]}"
done

for name in "${!TOPLEVELS[@]}"; do
  clone_or_update "$name" "${TOPLEVELS[$name]}"
done

# Repos that live one level above FastTrackStudio (under $PARENT/..) because
# the workspaces reference them via `../../<name>` relative paths.
DEV_PARENT="$(dirname "$PARENT")"
mkdir -p "$DEV_PARENT"
declare -A DEV_LEVEL=(
  [architect]="https://git.starcommand.live/codywright/architect.git|main"
  [Dioxus]=""  # placeholder; see Dioxus note below
)

(
  cd "$DEV_PARENT"
  for name in architect; do
    clone_or_update "$name" "${DEV_LEVEL[$name]}"
  done
)

# Dioxus: starcommand's codywright/dioxus exists but its initial pack
# exceeds the Cloudflare body limit, so we clone github upstream and
# apply two custom commits as patches. Patches must already be on disk
# at /tmp/0001-*.patch and /tmp/0002-*.patch (scp them from a machine
# that has the source tree).
if [ ! -d "$DEV_PARENT/Dioxus/dioxus/.git" ]; then
  echo "==> Dioxus (github upstream + custom patches)"
  mkdir -p "$DEV_PARENT/Dioxus"
  (
    cd "$DEV_PARENT/Dioxus"
    git clone --no-checkout https://github.com/DioxusLabs/dioxus.git
    cd dioxus
    git fetch --depth 1 origin a626d609c1995601215f780431d315cf7c063d1e
    git checkout a626d609c1995601215f780431d315cf7c063d1e
    if [ -f /tmp/0001-Expose-embedded-desktop-webview-host.patch ]; then
      git am --keep-non-patch /tmp/0001-Expose-embedded-desktop-webview-host.patch \
                              /tmp/0002-feat-desktop-plumb-linux_offscreen-through-EmbeddedD.patch
    else
      echo "    WARNING: Dioxus patches not found at /tmp/0001-* and /tmp/0002-*"
      echo "    Copy them from a machine that has the source tree first."
    fi
  )
else
  echo "==> Dioxus (already present, skipping)"
fi

echo
echo "Done. Workspace ready under: $PARENT"
echo "Build fts-extensions:    (cd $PARENT/fts-extensions && cargo build)"
echo "Build FastTrackStudio:   (cd $PARENT/FastTrackStudio && cargo build)"
