//! Persisted on/off toggles for the audio/clock-sync subsystem.
//!
//! Both values live in REAPER's global ExtState (`reaper-extstate.ini`)
//! so the choice survives REAPER restarts. Each is exposed as a REAPER
//! action so the user can toggle from the action list / a binding.
//!
//! Both default ON — the app layer assumes sync + drift correction
//! are running and exposes UI for turning them off when an operator
//! deliberately wants to opt out (e.g. running standalone with no
//! peers, or debugging timing without the corrector intervening).
//! No env vars required for the common case.
//!
//! Toggles take effect on the NEXT plugin load — the underlying
//! `ClockSync::bind` runs once at startup and the current
//! `daw_audio_sync` API doesn't expose a hot stop/start. Action
//! handlers show a console message so the user knows to restart.

use daw_reaper::safe_wrappers::ext_state;
use reaper_high::Reaper;
use std::ffi::CString;
use tracing::info;

const EXT_SECTION: &str = "FTS_SESSION";
const KEY_CLOCK_SYNC: &str = "clock_sync_enabled";
const KEY_DRIFT: &str = "drift_correction_enabled";

fn get_bool(key: &str, default: bool) -> bool {
    let low = Reaper::get().medium_reaper().low();
    let Ok(section) = CString::new(EXT_SECTION) else {
        return default;
    };
    let Ok(key) = CString::new(key) else {
        return default;
    };
    match ext_state::get_ext_state(low, &section, &key) {
        Some(s) if s == "0" => false,
        Some(s) if s == "1" => true,
        _ => default,
    }
}

fn set_bool(key: &str, value: bool) {
    let low = Reaper::get().medium_reaper().low();
    let Ok(section) = CString::new(EXT_SECTION) else {
        return;
    };
    let Ok(key_c) = CString::new(key) else {
        return;
    };
    let Ok(val) = CString::new(if value { "1" } else { "0" }) else {
        return;
    };
    ext_state::set_ext_state(low, &section, &key_c, &val, true);
}

/// Whether clock-sync (multicast peer discovery + position broadcast)
/// should be active at startup. Defaults to true.
pub fn clock_sync_enabled() -> bool {
    get_bool(KEY_CLOCK_SYNC, true)
}

/// Whether drift correction (auto-rate-changes when off the elected
/// leader's projected position) should be active at startup. Defaults
/// to true — sync is the default operating mode for FTS sessions, so
/// the corrector that keeps peers locked in time runs unless the user
/// explicitly toggles it off.
pub fn drift_enabled() -> bool {
    get_bool(KEY_DRIFT, true)
}

/// Toggle the persisted clock-sync on/off. Returns the new state.
/// Effect lands on next plugin load (no hot stop yet).
pub fn toggle_clock_sync() -> bool {
    let new = !clock_sync_enabled();
    set_bool(KEY_CLOCK_SYNC, new);
    info!(
        enabled = new,
        "[sync] Clock-sync toggled — takes effect on next plugin reload"
    );
    Reaper::get().show_console_msg(format!(
        "FTS: Clock-sync {} (takes effect on next REAPER restart)\n",
        if new { "enabled" } else { "disabled" }
    ));
    new
}

/// Toggle the persisted drift-correction on/off. Returns the new state.
/// Effect lands on next plugin load. Implies clock-sync (drift is a no-op
/// without an active ClockSync session).
pub fn toggle_drift_correction() -> bool {
    let new = !drift_enabled();
    set_bool(KEY_DRIFT, new);
    info!(
        enabled = new,
        "[sync] Drift correction toggled — takes effect on next plugin reload"
    );
    Reaper::get().show_console_msg(format!(
        "FTS: Drift correction {} (takes effect on next REAPER restart)\n",
        if new { "enabled" } else { "disabled" }
    ));
    new
}
