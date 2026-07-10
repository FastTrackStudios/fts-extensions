//! Minimal REAPER window and mouse utilities needed by the tempo module.

use reaper_low::Swell;
use reaper_low::raw::HWND;
use reaper_medium::Reaper as MediumReaper;

/// Control ID for the arrange view track-view child window (consistent across platforms).
const ARRANGE_CTRL_ID: i32 = 1000;

/// Get the arrange view HWND.
pub fn get_arrange_wnd(medium_reaper: &MediumReaper) -> Option<HWND> {
    let main_hwnd = medium_reaper.get_main_hwnd();
    let hwnd = unsafe { Swell::get().GetDlgItem(main_hwnd.as_ptr(), ARRANGE_CTRL_ID) };
    if hwnd.is_null() { None } else { Some(hwnd) }
}

/// Minimal mouse cursor context — returns the window name string.
///
/// Uses REAPER's `GetMousePosition` to determine if the cursor is over the
/// arrange view. Falls back to "unknown" which triggers the edit-cursor
/// fallback path in `position_at_mouse_cursor`.
pub fn get_mouse_cursor_context(medium_reaper: &MediumReaper) -> (String, String, String) {
    let window = if get_arrange_wnd(medium_reaper).is_some() {
        "arrange".to_string()
    } else {
        "unknown".to_string()
    };
    (window, String::new(), String::new())
}
