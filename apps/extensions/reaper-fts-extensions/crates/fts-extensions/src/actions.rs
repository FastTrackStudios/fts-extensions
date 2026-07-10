//! FTS action definitions.
//!
//! All actions are registered under the `FTS_` namespace.  The display name
//! shown in REAPER's action list is prefixed with "FTS: " in the registration
//! call inside `lib.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::continuous_action::start_continuous_action;
#[allow(unused_imports)]
use crate::item_actions;
use crate::tempo::{
    MoveGridVariant, register_move_grid_actions, set_move_grid_variant,
    snap_grid_to_transient_constrained_handler, snap_grid_to_transient_fully_constrained_handler,
    snap_grid_to_transient_handler,
};
use daw::service::ActionRegistration;
use reaper_high::Reaper;

pub type ActionDefs = Vec<(String, String, Arc<dyn Fn() + Send + Sync>, bool, bool)>;

pub(crate) fn show(msg: impl Into<String>) {
    Reaper::get().show_console_msg(msg.into());
}

static TEST_TOGGLE_STATE: AtomicBool = AtomicBool::new(false);

pub(crate) fn sync_toggle_state(command_id: &str, is_on: bool) {
    daw_reaper::Reaper.set_toggle_state(command_id, is_on);
}

pub(crate) fn toggle_test_toggle_handler() {
    let new_state = !TEST_TOGGLE_STATE.load(Ordering::Relaxed);
    TEST_TOGGLE_STATE.store(new_state, Ordering::Relaxed);

    show(format!(
        "FTS: Test Toggle {}\n",
        if new_state { "on" } else { "off" }
    ));
    sync_toggle_state("FTS_TEST_TOGGLE", new_state);
}

pub(crate) fn move_cursor_creating_time_selection_by_measure(action_id: i32) {
    let low = reaper_low::Reaper::get();

    let previous_cursor = low.GetCursorPosition();
    let mut selection_start = 0.0;
    let mut selection_end = 0.0;
    unsafe {
        low.GetSet_LoopTimeRange(
            false,
            false,
            &mut selection_start,
            &mut selection_end,
            false,
        );
    }

    low.Main_OnCommand(action_id, 0);
    let new_cursor = low.GetCursorPosition();

    let has_selection = selection_end > selection_start;
    let epsilon = 0.000_001;
    let (start, end) = if has_selection && (previous_cursor - selection_start).abs() < epsilon {
        (new_cursor.min(selection_end), new_cursor.max(selection_end))
    } else if has_selection && (previous_cursor - selection_end).abs() < epsilon {
        (
            selection_start.min(new_cursor),
            selection_start.max(new_cursor),
        )
    } else {
        (
            previous_cursor.min(new_cursor),
            previous_cursor.max(new_cursor),
        )
    };

    let mut start = start;
    let mut end = end;
    unsafe {
        low.GetSet_LoopTimeRange(true, false, &mut start, &mut end, false);
    }
}

/// Build the list of all FTS utility actions.
///
/// Call this **once** at startup (after REAPER high-level API is available).
pub fn build_action_defs() -> ActionDefs {
    // Register continuous actions with the continuous-action timer system.
    register_move_grid_actions();

    let defs = vec![
        // ── Launcher ─────────────────────────────────────────────────────
        // ── Tempo grid — move ────────────────────────────────────────────────
        action(
            "FTS_TEMPO_MOVE_MEASURE_GRID_TO_MOUSE",
            "Move closest measure grid line to mouse cursor (perform until shortcut released)",
            || {
                set_move_grid_variant(MoveGridVariant::ClosestMeasure);
                if !start_continuous_action("FTS_TEMPO_MOVE_MEASURE_GRID_TO_MOUSE") {
                    show("Failed to start Move Measure Grid action\n");
                }
            },
        ),
        action(
            "FTS_TEMPO_MOVE_MEASURE_GRID_TO_MOUSE_CONSTRAINED",
            "Move closest measure grid line to mouse cursor — constrained (perform until shortcut released)",
            || {
                set_move_grid_variant(MoveGridVariant::ClosestMeasureConstrained);
                if !start_continuous_action("FTS_TEMPO_MOVE_MEASURE_GRID_TO_MOUSE_CONSTRAINED") {
                    show("Failed to start Move Measure Grid (Constrained) action\n");
                }
            },
        ),
        action(
            "FTS_TEMPO_MOVE_MEASURE_GRID_TO_MOUSE_FULLY_CONSTRAINED",
            "Move closest measure grid line to mouse cursor — fully constrained (perform until shortcut released)",
            || {
                set_move_grid_variant(MoveGridVariant::ClosestMeasureFullyConstrained);
                if !start_continuous_action(
                    "FTS_TEMPO_MOVE_MEASURE_GRID_TO_MOUSE_FULLY_CONSTRAINED",
                ) {
                    show("Failed to start Move Measure Grid (Fully Constrained) action\n");
                }
            },
        ),
        action(
            "FTS_TEMPO_MOVE_GRID_TO_MOUSE",
            "Move closest grid line to mouse cursor (perform until shortcut released)",
            || {
                set_move_grid_variant(MoveGridVariant::ClosestGrid);
                if !start_continuous_action("FTS_TEMPO_MOVE_GRID_TO_MOUSE") {
                    show("Failed to start Move Grid action\n");
                }
            },
        ),
        action(
            "FTS_TEMPO_MOVE_MARKER_TO_MOUSE",
            "Move closest tempo marker to mouse cursor (perform until shortcut released)",
            || {
                set_move_grid_variant(MoveGridVariant::ClosestTempo);
                if !start_continuous_action("FTS_TEMPO_MOVE_MARKER_TO_MOUSE") {
                    show("Failed to start Move Tempo action\n");
                }
            },
        ),
        // ── Tempo grid — snap ────────────────────────────────────────────────
        action(
            "FTS_TEMPO_SNAP_GRID_TO_TRANSIENT",
            "Snap closest measure grid line to next transient",
            snap_grid_to_transient_handler,
        ),
        action(
            "FTS_TEMPO_SNAP_GRID_TO_TRANSIENT_CONSTRAINED",
            "Snap closest measure grid line to next transient — constrained",
            snap_grid_to_transient_constrained_handler,
        ),
        action(
            "FTS_TEMPO_SNAP_GRID_TO_TRANSIENT_FULLY_CONSTRAINED",
            "Snap closest measure grid line to next transient — fully constrained",
            snap_grid_to_transient_fully_constrained_handler,
        ),
        // ── Navigation ───────────────────────────────────────────────────────
        action(
            "FTS_MOVE_CURSOR_LEFT_CREATING_TIME_SELECTION_BY_MEASURE",
            "Move cursor left creating time selection by measure",
            || move_cursor_creating_time_selection_by_measure(40838),
        ),
        action(
            "FTS_MOVE_CURSOR_RIGHT_CREATING_TIME_SELECTION_BY_MEASURE",
            "Move cursor right creating time selection by measure",
            || move_cursor_creating_time_selection_by_measure(40837),
        ),
        // ── Item editing ─────────────────────────────────────────────────────
        action(
            "FTS_SPLIT_ITEMS_CROSSFADE_LEFT",
            "Split selected items at cursor with crossfade on left",
            item_actions::split_items_with_crossfade_left,
        ),
        toggle_menu_action("FTS_TEST_TOGGLE", "Test Toggle", toggle_test_toggle_handler),
        // ── Modes ────────────────────────────────────────────────────────────
        #[cfg(feature = "mod-session")]
        menu_action(
            "FTS_MODE_SELECTOR",
            "Mode Selector",
            crate::mode_selector::show_mode_menu,
        ),
        #[cfg(feature = "mod-session")]
        menu_action(
            "FTS_MODE_DEBUG_WINDOW_TITLES",
            "Mode: Debug Top-Level Window Titles",
            daw_reaper::window_manager::debug_dump_top_level_windows,
        ),
        #[cfg(feature = "mod-session")]
        menu_action(
            "FTS_MODE_DEBUG_TOOLBAR_STATES",
            "Mode: Debug Toolbar States",
            daw_reaper::window_manager::debug_dump_toolbar_states,
        ),
        #[cfg(feature = "mod-session")]
        menu_action(
            "FTS_MODE_DEBUG_TOOLBAR_COMMAND_IDS",
            "Mode: Debug Toolbar Command IDs",
            daw_reaper::window_manager::debug_log_toolbar_command_names,
        ),
        #[cfg(feature = "mod-session")]
        menu_action(
            "FTS_MODE_DEBUG_DOCKER_POSITIONS",
            "Mode: Debug Docker Positions",
            daw_reaper::window_manager::debug_dump_docker_positions,
        ),
        #[cfg(feature = "mod-session")]
        menu_action(
            "FTS_MODE_DEBUG_TOOLBAR_ATTACHMENTS",
            "Mode: Debug Toolbar Attachments",
            daw_reaper::window_manager::debug_dump_mode_toolbar_attachments,
        ),
        #[cfg(feature = "mod-session")]
        menu_action(
            "FTS_MODE_OPEN_ALL_TOOLBARS",
            "Mode: Open All Toolbars",
            daw_reaper::window_manager::open_all_mode_toolbars,
        ),
        // ── Sync toggles ────────────────────────────────────────────────────
        // Both are persisted in REAPER ExtState (FTS_SESSION namespace);
        // changes take effect on next plugin reload (no hot stop yet).
        menu_action(
            "FTS_CLOCK_SYNC_TOGGLE",
            "Sync: Toggle clock-sync (multicast peer discovery)",
            || {
                crate::sync_settings::toggle_clock_sync();
            },
        ),
        menu_action(
            "FTS_DRIFT_CORRECTION_TOGGLE",
            "Sync: Toggle drift correction (auto-rate-change)",
            || {
                crate::sync_settings::toggle_drift_correction();
            },
        ),
        // ── MIDI editor modes ─────────────────────────────────────────────
        menu_action(
            "FTS_MIDI_MODE_DRUMS",
            "MIDI mode: Drums (drum-map view + drum keybinds)",
            || crate::midi_mode::set_midi_mode(crate::midi_mode::MidiMode::Drums),
        ),
        menu_action(
            "FTS_MIDI_MODE_CYCLE",
            "MIDI mode: Cycle to next",
            crate::midi_mode::cycle_midi_mode,
        ),
        action(
            "FTS_MIDI_INSERT_FLAM",
            "MIDI: Insert flam at mouse cursor",
            crate::midi_flam::insert_flam_at_mouse,
        ),
        // ── Info ─────────────────────────────────────────────────────────────
        menu_action("FTS_INFO", "FastTrackStudio Info", || {
            let version = env!("CARGO_PKG_VERSION");
            Reaper::get().show_console_msg(format!(
                "FastTrackStudio Extensions v{version}\n\
                     https://github.com/FastTrackStudios\n"
            ));
        }),
    ];
    let mut defs = defs;

    // ── Tempo — time signatures ─────────────────────────────────────────────
    // Two insert actions per signature, both targeting the start of the
    // measure under the edit cursor (flooring — 99% through measure 4 is
    // still measure 4):
    //  - plain: sets the signature (hold Shift while firing for single-measure)
    //  - _SINGLE: always a single measure, restoring the previous signature on
    //    the next downbeat — for key sequences (`T 2 4`) where Shift can't be
    //    held through the whole chord sequence.
    for &(num, denom) in crate::tempo::TIME_SIGNATURES {
        defs.push(action(
            &format!("FTS_TEMPO_INSERT_TIMESIG_{num}_{denom}"),
            &format!("Insert {num}/{denom} time signature at measure (hold Shift: single measure)"),
            move || crate::tempo::insert_time_signature_at_cursor(num, denom),
        ));
        defs.push(action(
            &format!("FTS_TEMPO_INSERT_TIMESIG_{num}_{denom}_SINGLE"),
            &format!("Insert single measure of {num}/{denom} (restores previous signature)"),
            move || crate::tempo::insert_single_measure_time_signature(num, denom),
        ));
    }

    // ── Volume balancer — constant-sum fader linking ────────────────────────
    defs.push(action(
        "FTS_VOLBAL_TOGGLE",
        "Volume Balancer: toggle constant-sum fader linking",
        || {
            let on = !crate::volume_balancer::is_enabled();
            crate::volume_balancer::set_enabled(on);
            sync_toggle_state("FTS_VOLBAL_TOGGLE", on);
            show(format!(
                "FTS Volume Balancer: {}\n",
                if on { "enabled" } else { "disabled" }
            ));
        },
    ));
    defs.push(action(
        "FTS_VOLBAL_LINK_SELECTED",
        "Volume Balancer: link selected tracks (constant total volume)",
        crate::volume_balancer::link_selected_tracks,
    ));
    defs.push(action(
        "FTS_VOLBAL_UNLINK_SELECTED",
        "Volume Balancer: unlink groups containing selected tracks",
        crate::volume_balancer::unlink_selected_tracks,
    ));

    // ── MIDI mirror — source tracks with live performance twins ────────────
    #[cfg(feature = "mod-mirror")]
    {
        defs.push(action(
            "FTS_MIRROR_LINK_SELECTED",
            "Mirror: make selected tracks mirror sources (creates hidden performance twins)",
            crate::mirror::link_selected_tracks,
        ));
        defs.push(action(
            "FTS_MIRROR_UNLINK_SELECTED",
            "Mirror: unlink selected source tracks",
            crate::mirror::unlink_selected_tracks,
        ));
        defs.push(action(
            "FTS_MIRROR_REGENERATE",
            "Mirror: force regeneration of all mirror items",
            crate::mirror::regenerate_all,
        ));
        defs.push(action(
            "FTS_MIRROR_TOGGLE",
            "Mirror: toggle live mirroring",
            || {
                let on = !crate::mirror::is_enabled();
                crate::mirror::set_enabled(on);
                sync_toggle_state("FTS_MIRROR_TOGGLE", on);
                show(format!(
                    "FTS Mirror: {}\n",
                    if on { "enabled" } else { "disabled" }
                ));
            },
        ));
    }

    // Module actions (launcher, session-owned template/keyflow, sync, input)
    // are collected via daw::module::collect_actions() in lib.rs — not here.

    defs
}

/// Convenience constructor for a single action entry (not shown in menu).
fn action(
    id: &str,
    display_name: &str,
    handler: impl Fn() + Send + Sync + 'static,
) -> (String, String, Arc<dyn Fn() + Send + Sync>, bool, bool) {
    (
        id.to_string(),
        format!("FTS: {display_name}"),
        Arc::new(handler),
        false,
        false,
    )
}

/// Convenience constructor for a single action entry shown in the Extensions menu.
fn menu_action(
    id: &str,
    display_name: &str,
    handler: impl Fn() + Send + Sync + 'static,
) -> (String, String, Arc<dyn Fn() + Send + Sync>, bool, bool) {
    (
        id.to_string(),
        format!("FTS: {display_name}"),
        Arc::new(handler),
        true,
        false,
    )
}

/// Convenience constructor for a toggleable action shown in the Extensions menu.
fn toggle_menu_action(
    id: &str,
    display_name: &str,
    handler: impl Fn() + Send + Sync + 'static,
) -> (String, String, Arc<dyn Fn() + Send + Sync>, bool, bool) {
    (
        id.to_string(),
        format!("FTS: {display_name}"),
        Arc::new(handler),
        true,
        true,
    )
}
