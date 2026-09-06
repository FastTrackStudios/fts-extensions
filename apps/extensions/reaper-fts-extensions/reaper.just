# REAPER extension (apps/extensions/reaper-fts-extensions) — build, install
# into a real REAPER resource dir, and run the in-REAPER integration suite.
#
# `nix run .#fts-reaper` (nix/modules/packages/reaper.nix) is the fully
# reproducible path — a whole pre-wired REAPER + SWS + ReaPack + this
# extension. These recipes are the fast local loop: build the cdylib with
# plain cargo and symlink it into whatever REAPER config dir you already
# use day to day, so `just reaper install && reaper` is enough to pick up
# a change.

# Where your day-to-day REAPER keeps its resource dir. FTS_REAPER_HOME
# overrides it; default is ~/fts-dev — this machine's actual dev rig
# (launched with `-cfgfile ~/fts-dev/reaper.ini` through the FHS sandbox),
# NOT the system default ~/.config/REAPER. The integration-test harness
# below also defaults to ~/fts-dev (FTS_REAPER_RESOURCES) but runs headless
# against a real REAPER process of its own — the two never run at once.
reaper_home := env('FTS_REAPER_HOME', env('HOME') + "/fts-dev")

# One cargo workspace at the repo root — `just reaper ...` runs with this
# module file's own directory as cwd, not the workspace root, so the built
# .so has to be found there rather than assumed relative to `pwd`.
repo_root := `git rev-parse --show-toplevel`

# Build the release cdylib.
build:
    cargo build --release -p fts-extensions

# Build + symlink the .so into $FTS_REAPER_HOME/UserPlugins. Symlinked
# (not copied) so a rebuild is picked up on REAPER's next (re)start with
# no reinstall step.
install: build install-configs
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "{{reaper_home}}/UserPlugins"
    ln -sf "{{repo_root}}/target/release/libreaper_fts_extensions.so" \
        "{{reaper_home}}/UserPlugins/reaper_fts_extensions.so"
    echo "installed -> {{reaper_home}}/UserPlugins/reaper_fts_extensions.so"
    echo "restart REAPER to load it"

# Symlink reaper-input's keybind/workflow config and fts-launcher's action
# packs into $FTS_REAPER_HOME/fasttrackstudio/ — where reaper-input reads
# them from at runtime (see reaper-input's `source.rs`: "<REAPER resource
# path>/fasttrackstudio/input"). Both crates moved into the `daw` git
# dependency in the split, so this resolves their config directory through
# `cargo metadata` rather than a path in this repo — same logic
# fts-extensions-xtask's `install_configs` runs for the isolated
# integration-test profile, but safe to run against your real rig too
# without spinning up a REAPER instance.
install-configs:
    #!/usr/bin/env bash
    set -euo pipefail
    crate_dir() {
        manifest="$(cargo metadata --format-version=1 2>/dev/null | jq -r --arg n "$1" '.packages[] | select(.name == $n) | .manifest_path')"
        dirname "$manifest"
    }
    input_src="$(crate_dir reaper-input)/config/config"
    launcher_src="$(crate_dir fts-launcher)/packs"
    fts_dir="{{reaper_home}}/fasttrackstudio"

    mkdir -p "$fts_dir/input/keybinds" "$fts_dir/launcher/packs"
    # `ln -sf TARGET LINKNAME` does NOT overwrite LINKNAME when it's an
    # existing symlink to a directory — it follows it and creates the
    # link *inside* that directory instead. Since this recipe re-runs
    # every `install` (install-configs is a dependency), the second run
    # planted a self-referential symlink one level down inside the
    # SOURCE directory itself (a shared cargo git checkout, not
    # per-run scratch space) — rm the destination first so this is
    # actually idempotent.
    link() { rm -rf "$2"; ln -s "$1" "$2"; }
    for name in fasttrackstudio logic reaper pro-tools ableton overlays; do
        link "$input_src/$name" "$fts_dir/input/keybinds/$name"
    done
    link "$input_src/workflows" "$fts_dir/input/workflows"
    for name in reaper-core reaper-visibility; do
        link "$launcher_src/$name" "$fts_dir/launcher/packs/$name"
    done
    echo "configs -> $fts_dir"

# Remove the symlinks `install` created (extension + configs). Leaves the
# build artifact alone.
uninstall:
    rm -f "{{reaper_home}}/UserPlugins/reaper_fts_extensions.so"
    rm -rf "{{reaper_home}}/fasttrackstudio/input" "{{reaper_home}}/fasttrackstudio/launcher"

# Tail the extension's own log (tracing-appender, rolling daily).
log:
    tail -f "$HOME/.local/state/fasttrackstudio/reaper-fts-extensions.log"

# In-REAPER integration suite (13 tests): builds, installs into the
# isolated FTS_REAPER_RESOURCES profile (default ~/fts-dev, NOT the
# real REAPER config `install` above uses), and drives a real headless
# REAPER. Needs a licensed REAPER on PATH or FTS_REAPER_EXECUTABLE.
integration-test *ARGS:
    cargo run -p fts-extensions-xtask -- {{ARGS}}

# Same, with a visible REAPER window — watch the tests run.
integration-test-gui *ARGS:
    cargo run -p fts-extensions-xtask -- --gui {{ARGS}}

# Same, through a virtual display (Xvfb) — full GUI rendering, no window
# shown. What CI runs.
integration-test-virtual *ARGS:
    cargo run -p fts-extensions-xtask -- --virtual {{ARGS}}
