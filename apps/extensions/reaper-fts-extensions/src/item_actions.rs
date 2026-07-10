//! Item Editing Actions
//!
//! Custom actions for media item manipulation.

use reaper_high::Reaper;
use tracing::{debug, info};

/// Split selected items at edit cursor with crossfade on left side.
///
/// This replicates SWS action `SWS_AWSPLITXFADELEFT`:
/// 1. Gets cursor position
/// 2. For each selected item that contains the cursor:
///    - Splits the item at (cursor - fade_length)
///    - Extends left item to overlap by fade_length
///    - Adds fadeout to left item, fadein to right item
/// 3. Handles item grouping properly
pub fn split_items_with_crossfade_left() {
    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();
    let low = medium.low();

    // Begin undo block
    unsafe {
        use std::ffi::CString;
        let undo_name = CString::new("Split items at cursor with crossfade on left").unwrap();
        low.Undo_BeginBlock();

        // Get cursor position
        let cursor_pos = low.GetCursorPositionEx(std::ptr::null_mut());

        // Get default crossfade length from REAPER config
        // "defsplitxfadelen" is the default split crossfade length
        let fade_len = {
            let var_name = CString::new("defsplitxfadelen").unwrap();
            let mut size: i32 = 0;
            let ptr = low.get_config_var(var_name.as_ptr(), &mut size);
            if ptr.is_null() || size < 8 {
                0.01 // Default to 10ms if config not found
            } else {
                let val = *(ptr as *const f64);
                val.abs() // Negative means "not auto", use absolute value
            }
        };

        // Get default crossfade shape
        let fade_shape = {
            let var_name = CString::new("defxfadeshape").unwrap();
            let mut size: i32 = 0;
            let ptr = low.get_config_var(var_name.as_ptr(), &mut size);
            if ptr.is_null() || size < 4 {
                0 // Default shape
            } else {
                *(ptr as *const i32)
            }
        };

        debug!(
            cursor_pos = cursor_pos,
            fade_len = fade_len,
            fade_shape = fade_shape,
            "Split with crossfade parameters"
        );

        // Temporarily disable auto crossfades on split
        // We'll restore it after, but for now just proceed
        // (SWS uses ConfigVarOverride which auto-restores)

        // Get selected items count
        let num_selected = low.CountSelectedMediaItems(std::ptr::null_mut());

        if num_selected == 0 {
            low.Undo_EndBlock(undo_name.as_ptr(), 0);
            return;
        }

        // Collect selected items first (since splitting changes selection)
        let mut items = Vec::with_capacity(num_selected as usize);
        for i in 0..num_selected {
            let item = low.GetSelectedMediaItem(std::ptr::null_mut(), i);
            if !item.is_null() {
                items.push(item);
            }
        }

        // Track grouping
        let num_groups = count_item_groups(low);
        let mut last_group = -1i32;
        let mut group_add = 0i32;

        // Prevent UI refresh during batch operation
        low.PreventUIRefresh(1);

        for item in items {
            // Get item position and length
            let item_start = low.GetMediaItemInfo_Value(item, c_str("D_POSITION"));
            let item_length = low.GetMediaItemInfo_Value(item, c_str("D_LENGTH"));
            let item_group = low.GetMediaItemInfo_Value(item, c_str("I_GROUPID")) as i32;

            // Check if cursor is within item bounds
            if cursor_pos > item_start && cursor_pos < (item_start + item_length) {
                // Split at (cursor - fade_len) to create overlap area
                let split_pos = cursor_pos - fade_len;

                // Only split if split position is after item start
                if split_pos > item_start {
                    let new_item = low.SplitMediaItem(item, split_pos);

                    if !new_item.is_null() {
                        // Get new length of left item after split
                        let left_item_length = low.GetMediaItemInfo_Value(item, c_str("D_LENGTH"));

                        // Extend left item to overlap with right item
                        low.SetMediaItemLength(item, left_item_length + fade_len, false);

                        // Set fadeout on left item
                        low.SetMediaItemInfo_Value(item, c_str("D_FADEOUTLEN_AUTO"), fade_len);
                        low.SetMediaItemInfo_Value(
                            item,
                            c_str("C_FADEOUTSHAPE"),
                            fade_shape as f64,
                        );

                        // Set fadein on right (new) item
                        low.SetMediaItemInfo_Value(new_item, c_str("D_FADEINLEN_AUTO"), fade_len);
                        low.SetMediaItemInfo_Value(
                            new_item,
                            c_str("C_FADEINSHAPE"),
                            fade_shape as f64,
                        );

                        // Handle grouping - assign new item to a new group if original was grouped
                        if item_group > 0 {
                            if item_group != last_group {
                                last_group = item_group;
                                group_add += 1;
                            }
                            low.SetMediaItemInfo_Value(
                                new_item,
                                c_str("I_GROUPID"),
                                (num_groups + group_add) as f64,
                            );
                        }

                        debug!(
                            item_start = item_start,
                            split_pos = split_pos,
                            fade_len = fade_len,
                            "Split item with crossfade"
                        );
                    }
                }
            }

            // Deselect original item (SWS behavior)
            low.SetMediaItemInfo_Value(item, c_str("B_UISEL"), 0.0);
        }

        // Re-enable UI refresh
        low.PreventUIRefresh(-1);

        // Update arrange view
        low.UpdateArrange();

        // End undo block
        low.Undo_EndBlock(undo_name.as_ptr(), 4); // UNDO_STATE_ITEMS = 4
    }

    info!("Split items at cursor with crossfade on left");
}

/// Count the number of item groups in the project
fn count_item_groups(low: &reaper_low::Reaper) -> i32 {
    let mut max_group = 0i32;

    unsafe {
        let num_tracks = low.CountTracks(std::ptr::null_mut());
        for t in 0..num_tracks {
            let track = low.GetTrack(std::ptr::null_mut(), t);
            if track.is_null() {
                continue;
            }

            let num_items = low.CountTrackMediaItems(track);
            for i in 0..num_items {
                let item = low.GetTrackMediaItem(track, i);
                if !item.is_null() {
                    let group_id = low.GetMediaItemInfo_Value(item, c_str("I_GROUPID")) as i32;
                    if group_id > max_group {
                        max_group = group_id;
                    }
                }
            }
        }
    }

    max_group
}

/// Helper to create C string pointer for REAPER API calls
fn c_str(s: &str) -> *const std::os::raw::c_char {
    // SAFETY: These are all static strings that don't contain null bytes
    // and are used immediately in REAPER API calls
    static CACHE: std::sync::OnceLock<std::collections::HashMap<&'static str, std::ffi::CString>> =
        std::sync::OnceLock::new();

    // For known static strings, we can use a simple match
    // This avoids allocation for common cases
    match s {
        "D_POSITION" => b"D_POSITION\0".as_ptr() as *const _,
        "D_LENGTH" => b"D_LENGTH\0".as_ptr() as *const _,
        "I_GROUPID" => b"I_GROUPID\0".as_ptr() as *const _,
        "D_FADEOUTLEN_AUTO" => b"D_FADEOUTLEN_AUTO\0".as_ptr() as *const _,
        "D_FADEINLEN_AUTO" => b"D_FADEINLEN_AUTO\0".as_ptr() as *const _,
        "C_FADEOUTSHAPE" => b"C_FADEOUTSHAPE\0".as_ptr() as *const _,
        "C_FADEINSHAPE" => b"C_FADEINSHAPE\0".as_ptr() as *const _,
        "B_UISEL" => b"B_UISEL\0".as_ptr() as *const _,
        _ => {
            // Fallback for unknown strings - leak to get static lifetime
            let cstring = std::ffi::CString::new(s).unwrap();
            let ptr = cstring.as_ptr();
            std::mem::forget(cstring);
            ptr
        }
    }
}
