//! Session mode ↔ reaper-input workflow bridge.
//!
//! Each [`session::mode_actions::Mode`] maps to a workflow id of the form
//! `mode-<slug>` (e.g. `mode-organize`, `mode-write`). On mode change we
//! activate the matching workflow, which auto-deactivates the previous
//! one — `WorkflowManager` holds a single `active_workflow`. The base
//! profile keymap (fasttrackstudio) stays underneath; the mode workflow
//! is an overlay on top.
//!
//! The skeleton `.styx` files live in
//! `../input/config/workflows/mode-<slug>.styx` and are empty by default.
//! Add `keybind_overlays(...)`, `mouse_overlays(...)`, or `settings(...)`
//! blocks to those files to give a mode behavior — no Rust changes needed.

use session::mode_actions::{self, Mode};

/// Workflow id for a given mode. Must match the basename (sans `.styx`)
/// of the per-mode workflow file under `input/config/workflows/`.
pub fn workflow_id_for(mode: Mode) -> &'static str {
    match mode {
        Mode::Organize => "mode-organize",
        Mode::Write => "mode-write",
        Mode::Produce => "mode-produce",
        Mode::Record => "mode-record",
        Mode::Edit => "mode-edit",
        Mode::Mix => "mode-mix",
        Mode::Master => "mode-master",
        Mode::Live => "mode-live",
        Mode::Video => "mode-video",
        Mode::Scoring => "mode-scoring",
    }
}

fn on_mode_changed(mode: Mode) {
    // Push the mode label first so any log lines emitted by the workflow
    // activation (or by an input event racing in right after) carry the
    // new mode tag.
    reaper_input::set_mode_label(mode.display_name());

    let id = workflow_id_for(mode);
    match reaper_input::input::workflows::activate(id) {
        Ok(()) => tracing::info!(mode = %mode, workflow = id, "[mode-input] Activated workflow"),
        Err(e) => tracing::warn!(
            mode = %mode,
            workflow = id,
            error = %e,
            "[mode-input] Failed to activate workflow (skeleton file may be missing or empty)"
        ),
    }

    apply_mode_visibility(mode);
}

/// Apply the visibility manager's rule-based per-surface visibility for the
/// mode. Calls dynamic-template directly (same handler the
/// `FTS_VISIBILITY_MANAGER_MODE_<SLUG>` actions dispatch to) rather than firing
/// the action through REAPER's registry — `install()` runs before
/// `register_actions_sync`, so a named-command lookup would miss the startup
/// application. Modes without a rule set are a logged no-op on that side.
fn apply_mode_visibility(mode: Mode) {
    let command = format!("FTS_VISIBILITY_MANAGER_MODE_{}", mode.slug().to_uppercase());
    dynamic_template::daw_module::dispatch_session_command(&command);
    tracing::info!(mode = %mode, "[mode-visibility] applied mode visibility rules");
}

/// Register the mode→workflow bridge and activate the workflow for the
/// current mode so startup state matches the active mode. Call once
/// from `plugin_main` after both session and input modules are
/// initialised.
pub fn install() {
    mode_actions::add_mode_change_listener(on_mode_changed);
    on_mode_changed(mode_actions::current_mode());
}
