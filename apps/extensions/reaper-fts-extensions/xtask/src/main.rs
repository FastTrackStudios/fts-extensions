//! xtask runner for fts-extensions integration tests.
//!
//! Usage:
//!   cargo xtask                    # headless (DISPLAY=""), fastest
//!   cargo xtask --gui              # visible REAPER window, watch tests run
//!   cargo xtask --virtual          # virtual display (Xvfb), full GUI but invisible
//!   cargo xtask <filter>           # run tests matching filter
//!   FTS_KEEP_OPEN=1 cargo xtask   # keep REAPER open after tests

use reaper_test::runner::{ExtensionPackage, TestPackage, TestRunner};
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
        env::var("FTS_REAPER_RESOURCES").unwrap_or_else(|_| format!("{home}/.fts-dev"));

    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()?;
    let daw_root = project_root.join("../daw").canonicalize()?;

    println!("=== FTS-Extensions Integration Tests ===");
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

    let runner = TestRunner::new(&resources_dir)
        .with_extension_log(format!(
            "{home}/.local/state/fasttrackstudio/reaper-fts-extensions.log"
        ))
        .with_headless(runner_headless);

    let packages = vec![TestPackage {
        package: "fts-extensions".into(),
        features: vec![],
        test_threads: 1,
        default_skips: vec![],
        test_binary: Some("extension_loads".into()),
    }];

    runner.install_test_extensions(
        &daw_root,
        &project_root,
        &[ExtensionPackage {
            package: "fts-extensions".into(),
            lib_stem: "reaper_fts_extensions".into(),
            plugin_name: "reaper_fts_extensions.so".into(),
            release: true,
        }],
    )?;

    // Install config symlinks (modules contribute their own configs)
    install_configs(&project_root, &resources_dir)?;

    runner.build_test_packages(&project_root, &packages)?;
    let tests_passed = runner.run_reaper_tests(&packages, filter.as_deref())?;

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
/// sibling repo source directories so config files are live-editable.
fn install_configs(
    project_root: &std::path::Path,
    resources_dir: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let fts_dir = PathBuf::from(resources_dir).join("fasttrackstudio");

    // ── input: keybind profiles + workflows ──
    let input_src = project_root.join("../input/config").canonicalize()?;
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
    let launcher_src = project_root.join("../fts-launcher/packs").canonicalize()?;
    let launcher_packs = fts_dir.join("launcher/packs");
    std::fs::create_dir_all(&launcher_packs)?;
    for name in &["reaper-core", "reaper-visibility"] {
        symlink_force(&launcher_src.join(name), &launcher_packs.join(name))?;
    }
    println!("  launcher: packs");

    println!("  Config installed -> {}", fts_dir.display());
    Ok(())
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
