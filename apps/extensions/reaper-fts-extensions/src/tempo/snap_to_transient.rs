//! Snap Grid to Transient Action
//!
//! Moves the nearest measure grid line to align with the next transient.
//! Uses REAPER's "Move cursor to next transient in items" action (40375).

use super::envelope::{MIN_TEMPO_DIST, TempoEnvelope, move_tempo};
use super::grid::{
    get_closest_measure_grid_line, get_next_measure_position, get_previous_measure_position,
    position_at_mouse_cursor,
};
use reaper_high::Reaper;
use tracing::{debug, info, warn};

/// REAPER action ID for "Move cursor to next transient in items"
const ACTION_NEXT_TRANSIENT: i32 = 40375;

/// Snap grid to transient action variant
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapToTransientVariant {
    /// Unconstrained - affects all prior tempo
    Unconstrained,
    /// Constrained - adds anchor marker one measure before
    Constrained,
    /// Fully constrained - adds anchor markers before AND after
    FullyConstrained,
}

/// Saved state for restoring after action
struct SavedState {
    edit_cursor_pos: f64,
    arrange_start: f64,
    arrange_end: f64,
}

impl SavedState {
    /// Save current cursor and view state
    fn save() -> Self {
        let reaper = Reaper::get();
        let low = reaper.medium_reaper().low();

        let edit_cursor_pos = unsafe { low.GetCursorPosition() };

        let mut arrange_start = 0.0;
        let mut arrange_end = 0.0;
        unsafe {
            low.GetSet_ArrangeView2(
                std::ptr::null_mut(),
                false, // get
                0,
                0,
                &mut arrange_start,
                &mut arrange_end,
            );
        }

        Self {
            edit_cursor_pos,
            arrange_start,
            arrange_end,
        }
    }

    /// Restore cursor and view state
    fn restore(&self) {
        let reaper = Reaper::get();
        let low = reaper.medium_reaper().low();

        // Restore edit cursor
        unsafe {
            low.SetEditCurPos(self.edit_cursor_pos, false, false);
        }

        // Restore arrange view
        unsafe {
            low.GetSet_ArrangeView2(
                std::ptr::null_mut(),
                true, // set
                0,
                0,
                &mut self.arrange_start.clone(),
                &mut self.arrange_end.clone(),
            );
        }
    }
}

/// Execute the snap grid to transient action
///
/// This action:
/// 1. Saves cursor and zoom state
/// 2. Moves edit cursor to mouse position
/// 3. Runs "next transient" action to find the transient
/// 4. Moves the nearest measure grid line to that transient position
/// 5. Restores cursor and zoom state
pub fn snap_grid_to_transient(variant: SnapToTransientVariant) {
    let reaper = Reaper::get();
    let low = reaper.medium_reaper().low();

    // Get mouse position first
    let Some(mouse_pos) = position_at_mouse_cursor(true) else {
        warn!("Snap to transient: mouse not in valid position");
        return;
    };

    // Begin undo block
    unsafe {
        low.Undo_BeginBlock2(std::ptr::null_mut());
    }

    // Save current state
    let saved_state = SavedState::save();

    // Move edit cursor to mouse position
    unsafe {
        low.SetEditCurPos(mouse_pos, false, false);
    }

    // Run "Move cursor to next transient in items" action
    unsafe {
        low.Main_OnCommand(ACTION_NEXT_TRANSIENT, 0);
    }

    // Get the new cursor position (at the transient)
    let transient_pos = unsafe { low.GetCursorPosition() };

    // Check if cursor actually moved (found a transient)
    if (transient_pos - mouse_pos).abs() < 0.0001 {
        warn!("Snap to transient: no transient found");
        saved_state.restore();
        unsafe {
            low.Undo_EndBlock2(
                std::ptr::null_mut(),
                c"Snap grid to transient (no transient found)".as_ptr(),
                0,
            );
        }
        return;
    }

    debug!(
        "Found transient at {} (mouse was at {})",
        transient_pos, mouse_pos
    );

    // Find the closest measure grid line to the mouse position
    let grid_pos = get_closest_measure_grid_line(mouse_pos);

    // Now we need to move the grid line at grid_pos to transient_pos
    // This requires manipulating the tempo map

    // Initialize tempo map if needed
    let count = unsafe { low.CountTempoTimeSigMarkers(std::ptr::null_mut()) };
    if count == 0 {
        // Create initial tempo marker
        let tempo = unsafe { low.Master_GetTempo() };
        unsafe {
            low.SetTempoTimeSigMarker(std::ptr::null_mut(), -1, 0.0, -1, 0.0, tempo, 0, 0, false);
        }
    }

    // Get tempo envelope
    let Some(mut tempo_map) = TempoEnvelope::new() else {
        warn!("Snap to transient: failed to get tempo envelope");
        saved_state.restore();
        unsafe {
            low.Undo_EndBlock2(
                std::ptr::null_mut(),
                c"Snap grid to transient (tempo error)".as_ptr(),
                0,
            );
        }
        return;
    };

    if tempo_map.count_points() == 0 || tempo_map.is_locked() {
        warn!("Snap to transient: tempo envelope is empty or locked");
        saved_state.restore();
        unsafe {
            low.Undo_EndBlock2(
                std::ptr::null_mut(),
                c"Snap grid to transient (tempo locked)".as_ptr(),
                0,
            );
        }
        return;
    }

    // Create anchor markers based on variant
    let mut anchor_offset = 0usize;

    if variant == SnapToTransientVariant::Constrained
        || variant == SnapToTransientVariant::FullyConstrained
    {
        // Create anchor BEFORE the grid line
        let anchor_before_pos = get_previous_measure_position(grid_pos);

        if anchor_before_pos > 0.0 && tempo_map.find(anchor_before_pos, MIN_TEMPO_DIST).is_none() {
            if let Some(prev_id) = tempo_map.find_previous(anchor_before_pos) {
                let shape = tempo_map.get_point(prev_id).map(|p| p.shape).unwrap_or(0);
                let bpm = tempo_map.value_at_position(anchor_before_pos);

                if tempo_map.create_point(prev_id, anchor_before_pos, bpm, shape) {
                    anchor_offset = 1;
                    debug!(
                        "Created anchor marker BEFORE at {} with BPM {}",
                        anchor_before_pos, bpm
                    );
                }
            }
        }

        // For fully constrained, also create anchor AFTER
        if variant == SnapToTransientVariant::FullyConstrained {
            let anchor_after_pos = get_next_measure_position(grid_pos);

            if tempo_map.find(anchor_after_pos, MIN_TEMPO_DIST).is_none() {
                if let Some(prev_id) = tempo_map.find_previous(anchor_after_pos) {
                    let shape = tempo_map.get_point(prev_id).map(|p| p.shape).unwrap_or(0);
                    let bpm = tempo_map.value_at_position(anchor_after_pos);

                    if tempo_map.create_point(prev_id, anchor_after_pos, bpm, shape) {
                        debug!(
                            "Created anchor marker AFTER at {} with BPM {}",
                            anchor_after_pos, bpm
                        );
                    }
                }
            }
        }
    }

    // Find or create tempo marker at the grid position
    let target_id = if let Some(id) = tempo_map.find(grid_pos, MIN_TEMPO_DIST) {
        Some(id + anchor_offset)
    } else {
        // Create tempo marker at grid position
        if let Some(prev_id) = tempo_map.find_previous(grid_pos) {
            let shape = tempo_map.get_point(prev_id).map(|p| p.shape).unwrap_or(0);
            let bpm = tempo_map.value_at_position(grid_pos);

            if tempo_map.create_point(prev_id, grid_pos, bpm, shape) {
                Some(prev_id + 1)
            } else {
                None
            }
        } else {
            None
        }
    };

    // Move the tempo marker to the transient position
    let mut success = false;
    if let Some(id) = target_id {
        if id != 0 {
            let time_diff = transient_pos - grid_pos;

            if move_tempo(&mut tempo_map, id, time_diff, true) {
                tempo_map.commit();
                success = true;
                info!(
                    "Snapped grid from {} to transient at {}",
                    grid_pos, transient_pos
                );
            } else {
                warn!("Snap to transient: move_tempo failed (BPM out of range)");
            }
        }
    }

    // Restore state
    saved_state.restore();

    // End undo block
    let undo_desc = match variant {
        SnapToTransientVariant::Unconstrained => c"Snap grid to transient",
        SnapToTransientVariant::Constrained => c"Snap grid to transient (constrained)",
        SnapToTransientVariant::FullyConstrained => c"Snap grid to transient (fully constrained)",
    };

    let undo_flags = if success { 1 } else { 0 };
    unsafe {
        low.Undo_EndBlock2(std::ptr::null_mut(), undo_desc.as_ptr(), undo_flags);
    }
}

/// Handler for snap grid to transient (unconstrained)
pub fn snap_grid_to_transient_handler() {
    snap_grid_to_transient(SnapToTransientVariant::Unconstrained);
}

/// Handler for snap grid to transient (constrained)
pub fn snap_grid_to_transient_constrained_handler() {
    snap_grid_to_transient(SnapToTransientVariant::Constrained);
}

/// Handler for snap grid to transient (fully constrained)
pub fn snap_grid_to_transient_fully_constrained_handler() {
    snap_grid_to_transient(SnapToTransientVariant::FullyConstrained);
}
