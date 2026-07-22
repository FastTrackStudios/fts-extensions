//! Move Grid to Mouse Action
//!
//! Implements the "Move closest measure grid line to mouse cursor" action.
//! Based on SWS BR_Tempo.cpp MoveGridToMouse().

use super::envelope::{MIN_TEMPO_DIST, TempoEnvelope, move_tempo};
use super::grid::{
    get_closest_grid_line, get_closest_measure_grid_line, get_next_measure_position,
    get_previous_measure_position, position_at_mouse_cursor,
};
use crate::continuous_action::{ContinuousAction, register_continuous_action};
use crate::error::{lock_mut, lock_read, try_lock};
use reaper_high::Reaper;
use std::sync::{Mutex, OnceLock};
use tracing::{debug, info, warn};

/// Move grid action variant
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::enum_variant_names,
    reason = "the shared `Closest` prefix names the family of \"move to nearest X\" actions; renaming would touch 40+ call sites for a cosmetic lint"
)]
pub enum MoveGridVariant {
    /// Move closest tempo marker
    ClosestTempo,
    /// Move closest grid line
    ClosestGrid,
    /// Move closest measure grid line
    ClosestMeasure,
    /// Move closest measure grid line, constrained to one measure
    /// (adds anchor marker one measure before to prevent stretching)
    ClosestMeasureConstrained,
    /// Move closest measure grid line, fully constrained
    /// (adds anchor markers both before AND after to only affect the two adjacent measures)
    ClosestMeasureFullyConstrained,
}

/// Global state for the move grid action
struct MoveGridState {
    /// The tempo map being modified
    tempo_map: Option<TempoEnvelope>,
    /// The ID of the locked tempo marker being moved
    locked_id: Option<usize>,
    /// Previous mouse position for calculating delta
    last_position: f64,
    /// Whether we've actually moved the grid at least once
    moved_grid_once: bool,
    /// Whether we initialized the tempo map (created first marker)
    did_tempo_map_init: bool,
    /// Current action variant
    variant: MoveGridVariant,
    /// Whether we created an anchor marker (for constrained variant)
    created_anchor: bool,
}

impl Default for MoveGridState {
    fn default() -> Self {
        Self {
            tempo_map: None,
            locked_id: None,
            last_position: 0.0,
            moved_grid_once: false,
            did_tempo_map_init: false,
            variant: MoveGridVariant::ClosestMeasure,
            created_anchor: false,
        }
    }
}

static MOVE_GRID_STATE: OnceLock<Mutex<MoveGridState>> = OnceLock::new();

fn get_state() -> &'static Mutex<MoveGridState> {
    MOVE_GRID_STATE.get_or_init(|| Mutex::new(MoveGridState::default()))
}

/// Current variant being executed (set before starting action)
static CURRENT_VARIANT: OnceLock<Mutex<MoveGridVariant>> = OnceLock::new();

fn get_current_variant() -> &'static Mutex<MoveGridVariant> {
    CURRENT_VARIANT.get_or_init(|| Mutex::new(MoveGridVariant::ClosestMeasure))
}

/// Set the variant for the next action execution
pub fn set_move_grid_variant(variant: MoveGridVariant) {
    lock_mut(get_current_variant(), |v| *v = variant);
}

/// Initialize/cleanup callback for move grid action
///
/// Called with `true` when action starts, `false` when it ends.
pub fn move_grid_init(init: bool) -> bool {
    let Ok(mut state) = try_lock(get_state()) else {
        tracing::error!("Failed to lock move grid state");
        return false;
    };
    let reaper = Reaper::get();
    let low = reaper.medium_reaper().low();

    if init {
        // Starting action
        state.variant =
            lock_read(get_current_variant(), |v| *v).unwrap_or(MoveGridVariant::ClosestMeasure);
        state.locked_id = None;
        state.last_position = 0.0;
        state.moved_grid_once = false;
        state.did_tempo_map_init = false;
        state.created_anchor = false;
        state.tempo_map = None;

        // Check if mouse is in valid position
        let mouse_pos = position_at_mouse_cursor(true);
        if mouse_pos.is_none() {
            warn!("Move grid init failed: mouse not in valid position");
            return false;
        }

        // Begin undo block
        unsafe {
            low.Undo_BeginBlock2(std::ptr::null_mut());
        }

        // Check for frame-based grid (can't move frame grids)
        // For now, skip this check as the config var API is complex
        // TODO: Implement proper projgridframe check

        info!("Move grid action started: {:?}", state.variant);
        true
    } else {
        // Ending action - cleanup
        state.tempo_map = None;
        state.locked_id = None;
        state.moved_grid_once = false;
        state.did_tempo_map_init = false;
        state.created_anchor = false;

        info!("Move grid action ended");
        true
    }
}

/// Undo callback for move grid action
///
/// Creates undo point and returns undo flags when action ends.
pub fn move_grid_do_undo() -> u32 {
    let Ok(state) = try_lock(get_state()) else {
        tracing::error!("Failed to lock move grid state for undo");
        return 0;
    };
    let reaper = Reaper::get();
    let low = reaper.medium_reaper().low();

    if !state.moved_grid_once && state.did_tempo_map_init {
        // We initialized but didn't move - remove the tempo map we created
        // TODO: Implement RemoveTempoMap equivalent
    }

    // End undo block with appropriate description
    // UNDO_STATE_TRACKCFG = 1 (track/master vol/pan/routing, plus master tempo/time sig)
    let undo_flags = if state.moved_grid_once { 1 } else { 0 };

    let undo_desc = match state.variant {
        MoveGridVariant::ClosestMeasure => c"Move measure grid line to mouse",
        MoveGridVariant::ClosestMeasureConstrained => {
            c"Move measure grid line to mouse (constrained)"
        }
        MoveGridVariant::ClosestMeasureFullyConstrained => {
            c"Move measure grid line to mouse (fully constrained)"
        }
        MoveGridVariant::ClosestGrid => c"Move grid line to mouse",
        MoveGridVariant::ClosestTempo => c"Move tempo marker to mouse",
    };

    unsafe {
        low.Undo_EndBlock2(std::ptr::null_mut(), undo_desc.as_ptr(), undo_flags as i32);
    }

    if state.moved_grid_once {
        info!("Created undo point: {:?}", state.variant);
    }

    undo_flags
}

/// Main action callback - called repeatedly while key is held
pub fn move_grid_to_mouse() {
    let Ok(mut state) = try_lock(get_state()) else {
        tracing::error!("Failed to lock move grid state for action");
        return;
    };

    // Initialize tempo map on first call
    if state.tempo_map.is_none() {
        state.locked_id = None;
        state.last_position = 0.0;

        // Initialize tempo map if needed (for grid variants, not tempo variant)
        if state.variant != MoveGridVariant::ClosestTempo {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();

            let count = unsafe { low.CountTempoTimeSigMarkers(std::ptr::null_mut()) };
            if count == 0 {
                // Create initial tempo marker
                init_tempo_map();
                state.did_tempo_map_init = true;
            }
        }

        // Get tempo envelope
        state.tempo_map = TempoEnvelope::new();

        if state.tempo_map.is_none() {
            warn!("Failed to get tempo envelope");
            crate::continuous_action::stop_all_continuous_actions();
            return;
        }

        let tempo_map = state.tempo_map.as_ref().unwrap();
        if tempo_map.count_points() == 0 || tempo_map.is_locked() {
            warn!("Tempo envelope is empty or locked");
            crate::continuous_action::stop_all_continuous_actions();
            return;
        }
    }

    // Get mouse position
    let Some(mouse_position) = position_at_mouse_cursor(true) else {
        // Mouse moved out of valid area
        crate::continuous_action::stop_all_continuous_actions();
        return;
    };

    let mut time_diff = 0.0;

    // Calculate time difference
    if state.moved_grid_once {
        // Subsequent call - use delta from last position
        time_diff = mouse_position - state.last_position;
    } else {
        // First move - find and lock the grid line
        let tempo_map = state.tempo_map.as_ref().unwrap();

        let (grid, target_id) = match state.variant {
            MoveGridVariant::ClosestGrid => {
                let grid = get_closest_grid_line(mouse_position);
                let target_id = tempo_map.find(grid, MIN_TEMPO_DIST);
                (grid, target_id)
            }
            MoveGridVariant::ClosestMeasure
            | MoveGridVariant::ClosestMeasureConstrained
            | MoveGridVariant::ClosestMeasureFullyConstrained => {
                let grid = get_closest_measure_grid_line(mouse_position);
                let target_id = tempo_map.find(grid, MIN_TEMPO_DIST);
                (grid, target_id)
            }
            MoveGridVariant::ClosestTempo => {
                // Find closest tempo marker directly
                let target_id = tempo_map.find_closest(mouse_position);
                // Skip first marker (id 0)
                let target_id = target_id.map(|id| if id == 0 { 1 } else { id });

                if let Some(id) = target_id {
                    if let Some(point) = tempo_map.get_point(id) {
                        (point.time, Some(id))
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
        };

        // For constrained variants, create anchor markers to limit the effect
        // - ClosestMeasureConstrained: anchor before only
        // - ClosestMeasureFullyConstrained: anchor before AND after
        let mut anchor_offset = 0usize; // Track if we added anchors before (affects target_id)
        if state.variant == MoveGridVariant::ClosestMeasureConstrained
            || state.variant == MoveGridVariant::ClosestMeasureFullyConstrained
        {
            // Create anchor BEFORE the grid line
            let anchor_before_pos = get_previous_measure_position(grid);

            // Only create anchor if it's not at position 0 and doesn't already exist
            if anchor_before_pos > 0.0 {
                let tempo_map = state.tempo_map.as_mut().unwrap();

                // Check if there's already a marker at anchor position
                if tempo_map.find(anchor_before_pos, MIN_TEMPO_DIST).is_none() {
                    // Create anchor marker with current tempo at that position
                    if let Some(prev_id) = tempo_map.find_previous(anchor_before_pos) {
                        let shape = tempo_map.get_point(prev_id).map(|p| p.shape).unwrap_or(0);
                        let bpm = tempo_map.value_at_position(anchor_before_pos);

                        if tempo_map.create_point(prev_id, anchor_before_pos, bpm, shape) {
                            state.created_anchor = true;
                            anchor_offset = 1; // We added a point, so indices shift
                            debug!(
                                "Created anchor marker BEFORE at {} with BPM {}",
                                anchor_before_pos, bpm
                            );
                        }
                    }
                }
            }

            // For fully constrained, also create anchor AFTER the grid line
            if state.variant == MoveGridVariant::ClosestMeasureFullyConstrained {
                let anchor_after_pos = get_next_measure_position(grid);
                let tempo_map = state.tempo_map.as_mut().unwrap();

                // Check if there's already a marker at anchor position
                if tempo_map.find(anchor_after_pos, MIN_TEMPO_DIST).is_none() {
                    // Create anchor marker with current tempo at that position
                    if let Some(prev_id) = tempo_map.find_previous(anchor_after_pos) {
                        let shape = tempo_map.get_point(prev_id).map(|p| p.shape).unwrap_or(0);
                        let bpm = tempo_map.value_at_position(anchor_after_pos);

                        if tempo_map.create_point(prev_id, anchor_after_pos, bpm, shape) {
                            state.created_anchor = true;
                            // Note: anchor after doesn't affect target_id since it's after the grid
                            debug!(
                                "Created anchor marker AFTER at {} with BPM {}",
                                anchor_after_pos, bpm
                            );
                        }
                    }
                }
            }
        }

        // Check if we need to create a tempo marker at this grid position
        let target_id: Option<usize> =
            if target_id.is_none() && state.variant != MoveGridVariant::ClosestTempo {
                // No tempo marker on grid, create one
                let tempo_map = state.tempo_map.as_mut().unwrap();

                if let Some(prev_id) = tempo_map.find_previous(grid) {
                    let shape = tempo_map.get_point(prev_id).map(|p| p.shape).unwrap_or(0);
                    let bpm = tempo_map.value_at_position(grid);

                    if tempo_map.create_point(prev_id, grid, bpm, shape) {
                        Some(prev_id + 1)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                // Adjust target_id if we added an anchor before it
                target_id.map(|id| id + anchor_offset)
            };

        // Can't move first tempo marker
        if let Some(id) = target_id {
            if id != 0 {
                state.locked_id = Some(id);
                time_diff = mouse_position - grid;
                debug!("Locked tempo marker {} at grid position {}", id, grid);
            }
        }
    }

    // Move grid and commit changes
    if time_diff != 0.0 {
        if let Some(locked_id) = state.locked_id {
            // Take ownership of tempo_map temporarily to avoid borrow issues
            let mut tempo_map = state.tempo_map.take().unwrap();

            let move_result = move_tempo(&mut tempo_map, locked_id, time_diff, true);

            if !move_result {
                // Move failed - BPM would be out of range
                warn!("Move grid failed: BPM would be out of legal range");
                // Put tempo_map back
                state.tempo_map = Some(tempo_map);
                crate::continuous_action::stop_all_continuous_actions();
            } else {
                state.last_position = mouse_position;
                tempo_map.commit();
                state.moved_grid_once = true;
                // Put tempo_map back
                state.tempo_map = Some(tempo_map);

                debug!("Moved grid by {} to position {}", time_diff, mouse_position);
            }
        }
    }
}

/// Initialize tempo map with a default marker
fn init_tempo_map() {
    let reaper = Reaper::get();
    let low = reaper.medium_reaper().low();

    // Get current tempo
    let tempo = unsafe { low.Master_GetTempo() };

    // Create initial tempo marker at position 0
    unsafe {
        low.SetTempoTimeSigMarker(
            std::ptr::null_mut(), // project
            -1,                   // ptidx (-1 to add new)
            0.0,                  // timepos
            -1,                   // measurepos (-1 to use timepos)
            0.0,                  // beatpos
            tempo,                // bpm
            0,                    // timesig_num (0 for no change)
            0,                    // timesig_denom
            false,                // lineartempo
        );
    }

    debug!("Initialized tempo map with marker at 0.0, tempo={}", tempo);
}

/// Register the move grid continuous actions
pub fn register_move_grid_actions() {
    // Register Move Closest Measure Grid Line action
    register_continuous_action(ContinuousAction {
        command_id: "FTS_TEMPO_MOVE_MEASURE_GRID_TO_MOUSE",
        init: move_grid_init,
        action: || {
            set_move_grid_variant(MoveGridVariant::ClosestMeasure);
            move_grid_to_mouse();
        },
        do_undo: move_grid_do_undo,
    });

    // Register Move Closest Measure Grid Line (Constrained) action
    // This variant adds an anchor marker one measure before to prevent
    // the tempo change from affecting earlier measures
    register_continuous_action(ContinuousAction {
        command_id: "FTS_TEMPO_MOVE_MEASURE_GRID_TO_MOUSE_CONSTRAINED",
        init: move_grid_init,
        action: || {
            set_move_grid_variant(MoveGridVariant::ClosestMeasureConstrained);
            move_grid_to_mouse();
        },
        do_undo: move_grid_do_undo,
    });

    // Register Move Closest Measure Grid Line (Fully Constrained) action
    // This variant adds anchor markers BOTH before and after to only
    // affect the relationship between two adjacent measures
    register_continuous_action(ContinuousAction {
        command_id: "FTS_TEMPO_MOVE_MEASURE_GRID_TO_MOUSE_FULLY_CONSTRAINED",
        init: move_grid_init,
        action: || {
            set_move_grid_variant(MoveGridVariant::ClosestMeasureFullyConstrained);
            move_grid_to_mouse();
        },
        do_undo: move_grid_do_undo,
    });

    // Register Move Closest Grid Line action
    register_continuous_action(ContinuousAction {
        command_id: "FTS_TEMPO_MOVE_GRID_TO_MOUSE",
        init: move_grid_init,
        action: || {
            set_move_grid_variant(MoveGridVariant::ClosestGrid);
            move_grid_to_mouse();
        },
        do_undo: move_grid_do_undo,
    });

    // Register Move Closest Tempo Marker action
    register_continuous_action(ContinuousAction {
        command_id: "FTS_TEMPO_MOVE_MARKER_TO_MOUSE",
        init: move_grid_init,
        action: || {
            set_move_grid_variant(MoveGridVariant::ClosestTempo);
            move_grid_to_mouse();
        },
        do_undo: move_grid_do_undo,
    });

    info!("Registered move grid continuous actions");
}
