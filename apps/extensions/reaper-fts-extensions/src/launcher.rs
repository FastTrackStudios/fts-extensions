//! FTS Launcher integration.
//!
//! Initializes the launcher engine at extension startup.
//! The actual UI panel will be managed via reaper-dioxus when available.
//! For now, the launcher registers its actions and providers.

use std::sync::OnceLock;

use fts_launcher::LauncherEngine;
use tracing::info;

static LAUNCHER: OnceLock<LauncherEngine> = OnceLock::new();

/// Initialize the launcher engine.
/// Call once after DAW is initialized.
pub fn init() {
    match LAUNCHER.set(LauncherEngine::new()) {
        Ok(_) => info!("FTS Launcher engine initialized"),
        Err(_) => info!("FTS Launcher already initialized"),
    }
}

/// Get the launcher engine reference.
pub fn get() -> Option<&'static LauncherEngine> {
    LAUNCHER.get()
}
