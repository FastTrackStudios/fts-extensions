//! xtask runner for fts-extensions integration tests.
//!
//! Usage:
//!   cargo xtask                    # headless (DISPLAY=""), fastest
//!   cargo xtask --gui              # visible REAPER window, watch tests run
//!   cargo xtask --virtual          # virtual display (Xvfb), full GUI but invisible
//!   cargo xtask `<filter>`         # run tests matching filter
//!   FTS_KEEP_OPEN=1 cargo xtask   # keep REAPER open after tests

use daw::test::runner::{ExtensionPackage, TestPackage, TestRunner};
use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    // Parse display mode flags
    let gui_mode = args.iter().any(|a| a == "--gui");
    let virtual_mode = args.iter().any(|a| a == "--virtual");
    let headless = !gui_mode && !virtual_mode;

    // Filter is the first non-flag argument
    let filter = args.iter().skip(1).find(|a| !a.starts_with("--")).cloned();

    let home = env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let resources_dir =
        env::var("FTS_REAPER_RESOURCES").unwrap_or_else(|_| format!("{home}/fts-dev"));

    // xtask lives at apps/extensions/reaper-fts-extensions/xtask — the
    // monorepo root (the ONE cargo workspace) is three levels up.
    let repo_root = canonicalize_ctx(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.."),
        "monorepo root",
    )?;
    let project_root = repo_root.join("apps/extensions/reaper-fts-extensions");

    println!("=== FTS-Extensions Integration Tests ===");
    println!("  Workspace: {}", repo_root.display());
    println!("  Project:   {}", project_root.display());

    let mode_label = if gui_mode {
        "GUI (visible)"
    } else if virtual_mode {
        "Virtual display (Xvfb)"
    } else {
        "Headless (DISPLAY=\"\")"
    };
    println!("  Mode:      {mode_label}");

    // Virtual mode uses headless=true so the runner uses FTS_REAPER_EXECUTABLE
    // (our FHS wrapper). The DISPLAY is then restored from the Xvfb environment
    // by the wrapper script inheriting it.
    let runner_headless = if virtual_mode { true } else { headless };

    let mut runner = TestRunner::new(&resources_dir)
        .with_extension_log(format!(
            "{home}/.local/state/fasttrackstudio/reaper-fts-extensions.log"
        ))
        .with_headless(runner_headless);

    // `--virtual` brings up our own Xvfb + window manager. It used to
    // depend on an `fts-test` launcher that is not installed
    // everywhere, and silently fell back to headless when missing —
    // which meant GUI tests appeared to run and could never pass.
    if virtual_mode {
        runner = runner.with_virtual_display()?;
    }

    let packages = vec![
        TestPackage {
            package: "fts-extensions".into(),
            features: vec![],
            test_threads: 1,
            default_skips: vec![],
            test_binary: Some("extension_loads".into()),
        },
        // The expression editor's REAPER-only coverage: panel docking,
        // finding REAPER's selection, and writes reaching a real take.
        // Its logic is tested against the standalone backend instead.
        TestPackage {
            package: "fts-extensions".into(),
            features: vec![],
            test_threads: 1,
            default_skips: vec![],
            test_binary: Some("expression_editor".into()),
        },
        // Take envelopes in REAPER: creation by action, the
        // enumeration fallback, and the selection surviving both.
        // Nothing here has a standalone equivalent — the behaviour under
        // test is REAPER's own.
        TestPackage {
            package: "fts-extensions".into(),
            features: vec![],
            test_threads: 1,
            default_skips: vec![],
            test_binary: Some("take_envelope".into()),
        },
        // The midi-tools panels driven through DockHost — the only tests
        // that exercise the real Blitz renderer and the real event path.
        // Separate binary, so it needs its own entry: `test_binary` is a
        // single name, not a filter.
        TestPackage {
            package: "fts-extensions".into(),
            features: vec![],
            test_threads: 1,
            // Skipped by default because this rig shares its REAPER
            // profile (~/fts-dev) with the interactive dev instance: if
            // one is open, the spawned REAPER contends with it and these
            // fail on socket discovery rather than on anything they
            // assert. The DockHost bug that used to block them is fixed
            // (the layer is mounted now) — run them against a clean rig:
            //
            //     just reaper integration-test panel_
            default_skips: vec![
                "panel_toggles_via_its_action".into(),
                "panel_actually_renders_in_reaper".into(),
                "panel_click_shapes_the_take".into(),
            ],
            test_binary: Some("midi_tools_panel".into()),
        },
    ];

    // fts-extensions is built with `host-hooks` (default feature): it embeds
    // the daw socket host itself. Do NOT install daw-bridge alongside it —
    // two extensions would fight over the same fts-daw-<pid>.sock and every
    // RPC would die with ConnectionClosed.
    let stale_bridge = PathBuf::from(&resources_dir).join("UserPlugins/reaper_daw_bridge.so");
    if stale_bridge.symlink_metadata().is_ok() {
        std::fs::remove_file(&stale_bridge)?;
        println!("  Removed stale daw-bridge: {}", stale_bridge.display());
    }
    runner.install_extension_package(
        &repo_root,
        &ExtensionPackage {
            package: "fts-extensions".into(),
            lib_stem: "reaper_fts_extensions".into(),
            plugin_name: "reaper_fts_extensions.so".into(),
            release: true,
        },
    )?;

    // Install config symlinks (modules contribute their own configs)
    install_configs(&repo_root, &resources_dir)?;

    runner.build_test_packages(&repo_root, &packages)?;
    // REAPER restores whatever dialogs were open when its config was
    // last saved — usually the Actions list — and they cover the
    // arrange view and any panel under test. Close them before the run.
    let shots_dir = repo_root.join("target/reaper-shots");
    // The recorder sweeps stray dialogs itself for the first few
    // seconds — REAPER restores them after it starts, so closing once
    // before the run is too early to catch anything.
    let recording = runner
        .virtual_display()
        .map(|vd| vd.record(&shots_dir, std::time::Duration::from_millis(500)));

    let tests_passed = runner.run_reaper_tests(&packages, filter.as_deref())?;

    if let Some(rec) = recording {
        let kept = rec.finish(&shots_dir);
        println!("  Screenshots: {kept} frames in {}", shots_dir.display());
    }

    if tests_passed {
        println!("\n  All tests passed!");
        Ok(())
    } else {
        Err("Some tests failed".into())
    }
}

/// Install config symlinks for all modules into `$resources_dir/fasttrackstudio/`.
///
/// Each module gets its own subdirectory. Symlinks point back to the
/// in-tree source directories so config files are live-editable.
fn install_configs(
    repo_root: &std::path::Path,
    resources_dir: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let fts_dir = PathBuf::from(resources_dir).join("fasttrackstudio");

    // ── input: keybind profiles + workflows ──
    let input_src = canonicalize_ctx(
        &repo_root.join("features/reaper/reaper-input/config/config"),
        "reaper-input keybind config",
    )?;
    let input_keybinds = fts_dir.join("input/keybinds");
    std::fs::create_dir_all(&input_keybinds)?;
    for name in &[
        "fasttrackstudio",
        "logic",
        "reaper",
        "pro-tools",
        "ableton",
        "overlays",
    ] {
        symlink_force(&input_src.join(name), &input_keybinds.join(name))?;
    }
    std::fs::create_dir_all(fts_dir.join("input"))?;
    symlink_force(
        &input_src.join("workflows"),
        &fts_dir.join("input/workflows"),
    )?;
    println!("  input: keybinds + workflows");

    // ── launcher: action packs ──
    let launcher_src = canonicalize_ctx(
        &repo_root.join("features/launcher/fts-launcher/packs"),
        "fts-launcher packs",
    )?;
    let launcher_packs = fts_dir.join("launcher/packs");
    std::fs::create_dir_all(&launcher_packs)?;
    for name in &["reaper-core", "reaper-visibility"] {
        symlink_force(&launcher_src.join(name), &launcher_packs.join(name))?;
    }
    println!("  launcher: packs");

    println!("  Config installed -> {}", fts_dir.display());
    Ok(())
}

/// `canonicalize()` with an error message that says WHICH path was missing.
fn canonicalize_ctx(
    path: &std::path::Path,
    what: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    path.canonicalize()
        .map_err(|e| format!("{what} not found at {}: {e}", path.display()).into())
}

fn symlink_force(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    if dst.is_symlink() || dst.is_file() {
        let _ = std::fs::remove_file(dst);
    }
    if !dst.exists() {
        std::os::unix::fs::symlink(src, dst)?;
    }
    Ok(())
}
