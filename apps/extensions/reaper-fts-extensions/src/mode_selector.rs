//! Popup-menu mode selector. Bound to `FTS_MODE_SELECTOR`; intended to live
//! on the REAPER main toolbar so a single click pops a dropdown of all
//! session modes. Matches the pattern used by `dock_panel`'s context menu
//! (`reaper_dioxus::dock`): `CreatePopupMenu` + `InsertMenuItem` + a
//! `TrackPopupMenu` call with `TPM_RETURNCMD` so we get the selected ID
//! back inline.

use reaper_low::{Swell, raw};
use session::modes::{self as mode_actions, Mode};

const TPM_RETURNCMD: std::os::raw::c_int = 0x0100;
const TPM_NONOTIFY: std::os::raw::c_int = 0x0080;

/// Handler bound to `FTS_MODE_SELECTOR`. Pops a menu at the cursor with
/// all 8 modes; on selection, switches the current mode.
pub fn show_mode_menu() {
    let swell = Swell::get();
    let menu = swell.CreatePopupMenu();
    if menu.is_null() {
        tracing::warn!("Mode selector: CreatePopupMenu returned null");
        return;
    }

    let current = mode_actions::current_mode();
    for (idx, mode) in Mode::ALL.iter().enumerate() {
        let id = (idx + 1) as u32; // 0 means "no selection" in TrackPopupMenu
        let label = if *mode == current {
            format!("\u{2022} {}", mode.display_name())
        } else {
            format!("   {}", mode.display_name())
        };
        unsafe {
            insert_menu_item(menu, id, &label);
        }
    }

    // Anchor at the current mouse cursor so the popup appears right under
    // the toolbar button the user just clicked.
    let mut point = raw::POINT { x: 0, y: 0 };
    unsafe { swell.GetCursorPos(&mut point as _) };

    // Owner HWND: REAPER's main window so the menu participates in the
    // normal focus/keyboard chain.
    let owner = unsafe { reaper_low::Reaper::get().GetMainHwnd() };

    let selected = unsafe {
        swell.TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            point.x,
            point.y,
            0,
            owner,
            std::ptr::null(),
        )
    };

    if selected == 0 {
        // User dismissed without selecting.
        return;
    }
    let idx = (selected - 1) as usize;
    if let Some(&mode) = Mode::ALL.get(idx) {
        mode_actions::set_mode(mode);
    } else {
        tracing::warn!(selected, "Mode selector: out-of-range menu id");
    }
}

unsafe fn insert_menu_item(menu: raw::HMENU, id: u32, label: &str) {
    let swell = Swell::get();
    let mut label_buf: Vec<u8> = label.as_bytes().to_vec();
    label_buf.push(0);
    let mut mi = raw::MENUITEMINFO {
        fMask: raw::MIIM_TYPE | raw::MIIM_DATA | raw::MIIM_ID,
        wID: id,
        dwTypeData: label_buf.as_mut_ptr() as *mut _,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe {
        swell.InsertMenuItem(menu, -1, 1, &mut mi);
    }
}
