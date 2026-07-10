//! Grid line finding utilities
//!
//! Provides functions to find the closest grid lines at various positions.
//! Based on SWS BR_Util.cpp GetClosestGridLine and GetClosestMeasureGridLine.

use reaper_high::Reaper;
use tracing::debug;

/// Get the project's grid division setting
///
/// Returns the grid division in quarter notes (e.g., 1.0 = quarter note, 0.5 = eighth note).
fn get_grid_div_safe() -> f64 {
    // Default to 1.0 (quarter note)
    // TODO: Read from project config var "projgriddiv" when API is available
    1.0
}

/// Get the closest grid line to a position
///
/// Uses REAPER's SnapToGrid to find the closest grid line.
///
/// # Arguments
/// * `position` - Time position in seconds
///
/// # Returns
/// The time position of the closest grid line
pub fn get_closest_grid_line(position: f64) -> f64 {
    let reaper = Reaper::get();
    let low = reaper.medium_reaper().low();

    // SnapToGrid returns the snapped position
    // We need to check both left and right to find the truly closest
    let snapped = unsafe { low.SnapToGrid(std::ptr::null_mut(), position) };

    // If snapped position is to the right of our position, also check left
    if snapped > position {
        // Get the grid line to the left by snapping a position slightly left of snapped
        let left_pos = snapped - 0.001;
        let left_snapped = unsafe { low.SnapToGrid(std::ptr::null_mut(), left_pos) };

        // Return whichever is closer
        if (position - left_snapped).abs() < (snapped - position).abs() {
            return left_snapped;
        }
    }

    snapped
}

/// Get the closest measure grid line to a position
///
/// This finds the closest grid line that falls on a measure boundary,
/// based on the current time signature at that position.
///
/// Based on SWS BR_Util.cpp GetClosestMeasureGridLine().
///
/// # Arguments
/// * `position` - Time position in seconds
///
/// # Returns
/// The time position of the closest measure grid line
pub fn get_closest_measure_grid_line(position: f64) -> f64 {
    let reaper = Reaper::get();
    let low = reaper.medium_reaper().low();

    let grid_div = get_grid_div_safe();

    // Get time signature at position
    let mut num: i32 = 4;
    let mut denom: i32 = 4;

    unsafe {
        low.TimeMap_GetTimeSigAtTime(
            std::ptr::null_mut(),
            position,
            &mut num as *mut _,
            &mut denom as *mut _,
            std::ptr::null_mut(), // tempo (not needed)
        );
    }

    // Check if we need to override grid division to full measure
    // (den * gridDiv) / 4 < num means grid is smaller than a full measure
    let grid_line = if (denom as f64 * grid_div) / 4.0 < num as f64 {
        // Grid division is smaller than a measure, temporarily use full measure
        // Get the measure containing this position
        let (measures, _, _) = get_measure_info(position);

        // Get position of this measure and next measure
        let measure_start = get_measure_position(measures);
        let measure_end = get_measure_position(measures + 1);

        // Return whichever is closer
        if (position - measure_start).abs() <= (measure_end - position).abs() {
            measure_start
        } else {
            measure_end
        }
    } else {
        // Grid is already at measure level or larger, just use normal snap
        get_closest_grid_line(position)
    };

    debug!(
        "GetClosestMeasureGridLine: pos={}, grid_div={}, time_sig={}/{}, result={}",
        position, grid_div, num, denom, grid_line
    );

    grid_line
}

/// Get measure information at a position
///
/// Returns (measure_number, beats_in_measure, time_sig_num)
fn get_measure_info(position: f64) -> (i32, f64, i32) {
    let reaper = Reaper::get();
    let low = reaper.medium_reaper().low();

    let mut measures: i32 = 0;
    let mut cml: i32 = 0;
    let mut fullbeats: f64 = 0.0;
    let mut cdenom: i32 = 0;

    unsafe {
        low.TimeMap2_timeToBeats(
            std::ptr::null_mut(),
            position,
            &mut measures as *mut _,
            &mut cml as *mut _,
            &mut fullbeats as *mut _,
            &mut cdenom as *mut _,
        );
    }

    (measures, fullbeats, cdenom)
}

/// Get the time position of a measure
pub fn get_measure_position(measure: i32) -> f64 {
    let reaper = Reaper::get();
    let low = reaper.medium_reaper().low();

    unsafe { low.TimeMap2_beatsToTime(std::ptr::null_mut(), 0.0, &measure as *const _) }
}

/// Get the measure number containing a time position
pub fn get_measure_at_position(position: f64) -> i32 {
    let (measures, _, _) = get_measure_info(position);
    measures
}

/// Get the time position of the measure before the given position
///
/// Returns the start time of the previous measure, or 0.0 if at measure 0.
pub fn get_previous_measure_position(position: f64) -> f64 {
    let current_measure = get_measure_at_position(position);
    if current_measure <= 0 {
        return 0.0;
    }
    get_measure_position(current_measure - 1)
}

/// Get the time position of the measure after the given position
///
/// Returns the start time of the next measure.
pub fn get_next_measure_position(position: f64) -> f64 {
    let current_measure = get_measure_at_position(position);
    get_measure_position(current_measure + 1)
}

/// Get the closest grid line on the left side of a position
pub fn get_closest_left_side_grid_line(position: f64) -> f64 {
    let grid = get_closest_grid_line(position);
    if grid <= position {
        return grid;
    }

    // Grid is to the right, find the one to the left
    let left_pos = grid - 0.001;
    get_closest_grid_line(left_pos)
}

/// Get the closest grid line on the right side of a position
pub fn get_closest_right_side_grid_line(position: f64) -> f64 {
    let grid = get_closest_grid_line(position);
    if grid >= position {
        return grid;
    }

    // Grid is to the left, find the one to the right
    let right_pos = grid + 0.001;
    get_closest_grid_line(right_pos)
}

/// Get position at mouse cursor
///
/// Returns the time position at the current mouse cursor location,
/// or None if the mouse is not over a valid area.
///
/// Based on SWS BR_MouseUtil.cpp PositionAtMouseCursor().
///
/// # Arguments
/// * `_check_ruler` - If true, also check if mouse is over the ruler area (currently always allowed)
pub fn position_at_mouse_cursor(_check_ruler: bool) -> Option<f64> {
    let reaper = Reaper::get();
    let low = reaper.medium_reaper().low();

    // Get mouse position in screen coordinates
    let mut mouse_x: i32 = 0;
    let mut mouse_y: i32 = 0;
    unsafe {
        low.GetMousePosition(&mut mouse_x, &mut mouse_y);
    }

    let mouse_point = reaper_low::raw::POINT {
        x: mouse_x,
        y: mouse_y,
    };

    // Try to get arrange window and calculate position
    if let Some(arrange_hwnd) = crate::reaper_utils::get_arrange_wnd(reaper.medium_reaper()) {
        // Convert mouse to client coordinates relative to arrange window
        let mut client_point = mouse_point;
        unsafe {
            reaper_low::Swell::get().ScreenToClient(arrange_hwnd, &mut client_point);
        }

        // Get arrange view start/end times
        let mut start_time: f64 = 0.0;
        let mut end_time: f64 = 0.0;
        unsafe {
            low.GetSet_ArrangeView2(
                std::ptr::null_mut(), // current project
                false,                // isSet = false (get)
                0,                    // screen_x_start (not used when getting)
                0,                    // screen_x_end (not used when getting)
                &mut start_time,
                &mut end_time,
            );
        }

        // Get horizontal zoom level (pixels per second)
        let h_zoom = low.GetHZoomLevel();

        // If we have valid zoom and the client x is reasonable, calculate position
        // We're more permissive here - if client_x is negative or very large,
        // it might just mean mouse is slightly outside arrange but still usable
        if h_zoom > 0.0 {
            // Calculate position: start + client_x / hzoom
            let position = start_time + (client_point.x as f64) / h_zoom;

            // Only reject if position would be very negative (clearly wrong)
            if position >= -10.0 {
                debug!(
                    "Mouse position: screen=({},{}), client_x={}, h_zoom={:.2}, start={:.2}, position={:.4}",
                    mouse_x, mouse_y, client_point.x, h_zoom, start_time, position
                );
                return Some(position.max(0.0)); // Clamp to >= 0
            }
        }

        debug!(
            "Position calculation failed: client_x={}, h_zoom={}, start={}",
            client_point.x, h_zoom, start_time
        );
    } else {
        debug!("Could not get arrange window handle");
    }

    // Fallback: Check window context and use edit cursor as last resort
    let (window, _segment, _details) =
        crate::reaper_utils::get_mouse_cursor_context(reaper.medium_reaper());

    // For MIDI editor or if we're in a "reasonable" window, use edit cursor
    if window == "midi_editor" || window == "arrange" || window == "ruler" || window == "unknown" {
        let edit_cursor = unsafe { low.GetCursorPosition() };
        debug!(
            "Using edit cursor as fallback: window={}, position={}",
            window, edit_cursor
        );
        return Some(edit_cursor);
    }

    debug!("Mouse not in valid area: window={}", window);
    None
}
