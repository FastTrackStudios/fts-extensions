//! MIDI editor mode ↔ reaper-input workflow bridge.
//!
//! On a [`MidiMode`] change we:
//!   1. Activate the matching reaper-input workflow `midi-<slug>` (its
//!      `bindings(...)` overlay supplies the mode's MIDI keybinds, e.g.
//!      `f` → flam). The workflow lives at
//!      `../input/config/workflows/midi-<slug>.styx`.
//!   2. Apply the mode's MIDI-editor setup by running MIDI Editor *section*
//!      actions on the active editor (drum-map note names, triangle view,
//!      etc.). This can't go through the workflow `settings(...)` block — that
//!      runs `Main_OnCommand` (arrange section), which doesn't reach the MIDI
//!      editor — so the setup list lives here.

use crate::midi_mode::{self, MidiMode};
use reaper_high::Reaper;

/// Workflow id for a mode. Must match the basename (sans `.styx`) of the
/// workflow file under `input/config/workflows/`.
pub fn workflow_id_for(mode: MidiMode) -> String {
    format!("midi-{}", mode.slug())
}

/// MIDI Editor section commands run on the active editor when entering a mode.
fn setup_commands_for(mode: MidiMode) -> &'static [i32] {
    match mode {
        // Drum programming view: named-note (drum map) names, events drawn as
        // triangles, note names shown on notes.
        MidiMode::Drums => &[
            40043, // Mode: Named Notes (Drum Map)
            40448, // View: Show events as triangles (drum mode)
            40045, // View: Show note names on notes
        ],
    }
}

/// Run the mode's MIDI-editor setup on the currently active editor (if any).
fn apply_midi_setup(mode: MidiMode) {
    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();
    let Some(editor) = medium.midi_editor_get_active() else {
        return;
    };
    let hwnd = editor.as_ptr();
    let low = medium.low();
    for &cmd in setup_commands_for(mode) {
        unsafe { low.MIDIEditor_OnCommand(hwnd, cmd) };
    }
}

fn on_midi_mode_changed(mode: MidiMode) {
    let id = workflow_id_for(mode);
    match reaper_input::input::workflows::activate(&id) {
        Ok(()) => {
            tracing::info!(midi_mode = %mode, workflow = %id, "[midi-mode] Activated workflow")
        }
        Err(e) => tracing::warn!(
            midi_mode = %mode,
            workflow = %id,
            error = %e,
            "[midi-mode] Failed to activate workflow (file may be missing or empty)"
        ),
    }
    apply_midi_setup(mode);
}

/// Register the MIDI-mode → workflow bridge. Call once from `plugin_main`
/// after the input module is initialised. No mode is entered at startup;
/// modes are switched explicitly (e.g. via `FTS_MIDI_MODE_DRUMS`).
pub fn install() {
    midi_mode::add_midi_mode_change_listener(on_midi_mode_changed);
}
