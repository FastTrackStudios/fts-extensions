home_dir       := env("HOME")
reaper_home    := env("REAPER_HOME", home_dir / ".fts-dev")
reaper_plugins := env("REAPER_PLUGINS", reaper_home / "UserPlugins")
fts_config     := reaper_home / "fasttrackstudio"
lib_name       := "reaper_fts_extensions"
profile        := "debug"

# Sibling repo roots (config sources)
input_dir      := justfile_directory() / "../input"
launcher_dir   := justfile_directory() / "../fts-launcher"

# Extra cargo features to enable on top of defaults. Override per-invocation:
#   just features=ui-dock install-release          # defaults + ui-dock
#   just features=mod-launcher,ui-dock install-release
# For minimal builds during bisection, use:
#   just features=mod-input release-minimal       # only mod-input, no defaults
# Available: mod-launcher, mod-session, mod-sync, mod-input, ui-dock,
#            poll-broadcast, host-hooks
features := ""

# Build the extension
build *args:
    cargo build -p fts-extensions --features "{{features}}" {{args}}

# Build in release mode (defaults + extras from `features`)
release:
    cargo build -p fts-extensions --release --features "{{features}}"

# Build in release mode with NO default features (bisection mode)
release-minimal:
    cargo build -p fts-extensions --release --no-default-features --features "{{features}}"

# Symlink the bisection build into REAPER's UserPlugins
install-release-minimal: release-minimal install-config
    mkdir -p {{reaper_plugins}}
    ln -sf "{{justfile_directory()}}/target/release/lib{{lib_name}}.so" "{{reaper_plugins}}/{{lib_name}}.so"
    @echo "Symlinked -> {{reaper_plugins}}/{{lib_name}}.so"
    @echo "Features (no defaults): {{features}}"

# Build and check
check:
    cargo check -p fts-extensions --features "{{features}}"

# Symlink the built extension into REAPER's UserPlugins
install: build install-config
    mkdir -p {{reaper_plugins}}
    ln -sf "{{justfile_directory()}}/target/{{profile}}/lib{{lib_name}}.so" "{{reaper_plugins}}/{{lib_name}}.so"
    @echo "Symlinked -> {{reaper_plugins}}/{{lib_name}}.so"
    @echo "Features: {{features}}"

# Build release and symlink
install-release: release install-config
    mkdir -p {{reaper_plugins}}
    ln -sf "{{justfile_directory()}}/target/release/lib{{lib_name}}.so" "{{reaper_plugins}}/{{lib_name}}.so"
    @echo "Symlinked -> {{reaper_plugins}}/{{lib_name}}.so"
    @echo "Features: {{features}}"

# Remove the symlink from REAPER's UserPlugins
uninstall:
    rm -f "{{reaper_plugins}}/{{lib_name}}.so"
    @echo "Removed {{reaper_plugins}}/{{lib_name}}.so"

# Install config symlinks for all modules into $REAPER_HOME/fasttrackstudio/.
# Each module gets its own subdirectory. Config files are live-editable:
# save a .styx file and the extension hot-reloads it.
install-config:
    @echo "Installing config symlinks -> {{fts_config}}/"
    # ── input: keybind profiles + workflows ──
    mkdir -p {{fts_config}}/input/keybinds
    ln -sfn "{{input_dir}}/config/fasttrackstudio" "{{fts_config}}/input/keybinds/fasttrackstudio"
    ln -sfn "{{input_dir}}/config/logic"           "{{fts_config}}/input/keybinds/logic"
    ln -sfn "{{input_dir}}/config/reaper"          "{{fts_config}}/input/keybinds/reaper"
    ln -sfn "{{input_dir}}/config/pro-tools"       "{{fts_config}}/input/keybinds/pro-tools"
    ln -sfn "{{input_dir}}/config/ableton"         "{{fts_config}}/input/keybinds/ableton"
    ln -sfn "{{input_dir}}/config/overlays"        "{{fts_config}}/input/keybinds/overlays"
    rm -rf "{{fts_config}}/input/workflows"
    ln -sfn "{{input_dir}}/config/workflows"       "{{fts_config}}/input/workflows"
    @echo "  input: keybinds + workflows"
    # ── launcher: action packs ──
    mkdir -p {{fts_config}}/launcher/packs
    ln -sfn "{{launcher_dir}}/packs/reaper-core"       "{{fts_config}}/launcher/packs/reaper-core"
    ln -sfn "{{launcher_dir}}/packs/reaper-visibility"  "{{fts_config}}/launcher/packs/reaper-visibility"
    @echo "  launcher: packs"
    # ── session, sync, keyflow, dynamic-template: no config files ──
    @echo "Done."

# Remove all config symlinks
uninstall-config:
    # input
    rm -f "{{fts_config}}/input/keybinds/fasttrackstudio"
    rm -f "{{fts_config}}/input/keybinds/logic"
    rm -f "{{fts_config}}/input/keybinds/reaper"
    rm -f "{{fts_config}}/input/keybinds/pro-tools"
    rm -f "{{fts_config}}/input/keybinds/ableton"
    rm -f "{{fts_config}}/input/keybinds/overlays"
    rm -f "{{fts_config}}/input/workflows"
    # launcher
    rm -f "{{fts_config}}/launcher/packs/reaper-core"
    rm -f "{{fts_config}}/launcher/packs/reaper-visibility"
    @echo "Removed config symlinks from {{fts_config}}/"

# Run unit tests (no REAPER needed)
test:
    cargo test --workspace

# ── UI snapshot tests ────────────────────────────────────────────
# Headlessly render fts-ui components via Blitz and diff against
# committed reference PNGs in crates/ui-snapshot/tests/reference/.

# Render every scene and fail on pixel diff above tolerance (default 0.1).
snapshot-check:
    cargo run -p ui-snapshot --release -- check

# Render a single scene to target/ui-snapshots/<name>.png for manual inspection.
snapshot-render name:
    cargo run -p ui-snapshot --release -- render {{name}}

# Regenerate all reference PNGs — run after intentional UI changes.
snapshot-update:
    cargo run -p ui-snapshot --release -- update

daw_dir := justfile_directory() / "../daw"

# Run integration tests headless (fastest, no GUI -- default for CI)
integration-test *args:
    FTS_REAPER_EXECUTABLE="{{justfile_directory()}}/scripts/reaper-test-wrapper.sh" \
    FTS_REAPER_RESOURCES="{{reaper_home}}" \
    cargo xtask {{args}}

# Run integration tests with visible REAPER window (watch tests execute)
integration-test-gui *args:
    FTS_REAPER_EXECUTABLE="{{justfile_directory()}}/scripts/reaper-test-wrapper.sh" \
    FTS_REAPER_RESOURCES="{{reaper_home}}" \
    cargo xtask --gui {{args}}

xvfb_run := "/nix/store/sbzdw6r5mw722sm93lr86qq07vpm28xj-xvfb-run-1+g87f6705/bin/xvfb-run"

# Run integration tests on virtual display (Xvfb -- full GUI rendering but invisible)
integration-test-virtual *args:
    {{xvfb_run}} -a -s "-screen 0 1920x1080x24" \
    bash -c 'FTS_REAPER_EXECUTABLE="{{justfile_directory()}}/scripts/reaper-test-wrapper.sh" \
    FTS_REAPER_RESOURCES="{{reaper_home}}" \
    FTS_VIRTUAL_DISPLAY="$DISPLAY" \
    cargo xtask --virtual {{args}}'

# Install the daw-bridge extension (provides VOX socket for test connectivity)
install-daw-bridge:
    cd "{{daw_dir}}" && cargo build -p daw-bridge --release
    ln -sf "{{daw_dir}}/target/release/libreaper_daw_bridge.so" "{{reaper_plugins}}/reaper_daw_bridge.so"
    @echo "daw-bridge installed -> {{reaper_plugins}}/reaper_daw_bridge.so"

# Launch REAPER via fts-dev
run:
    fts-dev

# Kill any running REAPER instances
kill:
    -pkill -f 'reaper' || true
    @sleep 1
    @echo "REAPER killed"

# Build release, install, kill old REAPER, and relaunch
restart: install-release kill
    @sleep 1
    fts-dev &
    @echo "REAPER restarting..."

# Tail the extension log (live)
log:
    tail -f "$(ls -t {{home_dir}}/.local/state/fasttrackstudio/reaper-fts-extensions.log.* | head -n 1)"

# Show last N lines of the extension log (default 30)
log-last n="30":
    tail -n {{n}} "$(ls -t {{home_dir}}/.local/state/fasttrackstudio/reaper-fts-extensions.log.* | head -n 1)"
