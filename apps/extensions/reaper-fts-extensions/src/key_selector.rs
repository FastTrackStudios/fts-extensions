//! Popup-menu key selector, bound to `FTS_KEY_SELECTOR`.
//!
//! Meant to sit on the main toolbar: one click pops every key, and
//! choosing one drops a key change at the edit cursor on the KEY track.
//! Same shape as [`crate::mode_selector`] — `CreatePopupMenu` +
//! `InsertMenuItem` + `TrackPopupMenu` with `TPM_RETURNCMD` so the
//! selection comes back inline.
//!
//! Thirty keys is too many for one flat list, so majors and minors are
//! separated by a divider and the key currently in force is bulleted.

use daw::service::{ProjectContext, transport::service::Transport};
use reaper_low::{Swell, raw};
use session::key;

const TPM_RETURNCMD: std::os::raw::c_int = 0x0100;
const TPM_NONOTIFY: std::os::raw::c_int = 0x0080;

/// The keys offered, in circle-of-fifths order out from C in both
/// directions so the common ones come first and the enharmonics sit
/// beside each other.
const MAJORS: [&str; 15] = [
    "C", "G", "D", "A", "E", "B", "F#", "C#", "F", "Bb", "Eb", "Ab", "Db", "Gb", "Cb",
];
const MINORS: [&str; 15] = [
    "A", "E", "B", "F#", "C#", "G#", "D#", "A#", "D", "G", "C", "F", "Bb", "Eb", "Ab",
];

/// Handler bound to `FTS_KEY_SELECTOR`.
pub fn show_key_menu() {
    let swell = Swell::get();
    let menu = swell.CreatePopupMenu();
    if menu.is_null() {
        tracing::warn!("Key selector: CreatePopupMenu returned null");
        return;
    }

    let project = ProjectContext::Current;
    let at = Transport::get_position(&daw_reaper::Reaper, project.clone());
    let current = key::key_at(&daw_reaper::Reaper, project, at).map(|k| key::format_key(&k));

    let mut id = 1u32;
    for (label, major) in MAJORS
        .iter()
        .map(|r| (format!("{r} major"), true))
        .chain(std::iter::once(("-".to_string(), true)))
        .chain(MINORS.iter().map(|r| (format!("{r} minor"), false)))
    {
        if label == "-" {
            // A separator keeps the majors and minors apart without a
            // submenu, which would cost an extra click on a toolbar.
            unsafe { insert_separator(menu) };
            continue;
        }
        let shown = if current.as_deref() == Some(label.as_str()) {
            format!("\u{2022} {label}")
        } else {
            format!("   {label}")
        };
        unsafe { insert_menu_item(menu, id, &shown) };
        let _ = major;
        id += 1;
    }

    let mut point = raw::POINT { x: 0, y: 0 };
    unsafe { swell.GetCursorPos(&mut point as _) };
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
        return; // dismissed
    }

    let idx = (selected - 1) as usize;
    let (root, major) = if idx < MAJORS.len() {
        (MAJORS[idx], true)
    } else if let Some(root) = MINORS.get(idx - MAJORS.len()) {
        (*root, false)
    } else {
        tracing::warn!(selected, "Key selector: out-of-range menu id");
        return;
    };

    if let Err(err) = set_key(root, major) {
        tracing::warn!(%err, root, major, "Key selector: could not set the key");
    }
}

fn set_key(root: &str, major: bool) -> eyre::Result<()> {
    let key = key::key_from_name(root, major)
        .ok_or_else(|| eyre::eyre!("{root} is not a note"))?;
    let project = ProjectContext::Current;
    let at = Transport::get_position(&daw_reaper::Reaper, project.clone());
    key::set_key_at(&daw_reaper::Reaper, project, at, &key)
        .map_err(|e| eyre::eyre!("{e:?}"))?;
    Ok(())
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
    unsafe { swell.InsertMenuItem(menu, -1, 1, &mut mi) };
}

unsafe fn insert_separator(menu: raw::HMENU) {
    let swell = Swell::get();
    let mut label_buf: Vec<u8> = b"-".to_vec();
    label_buf.push(0);
    let mut mi = raw::MENUITEMINFO {
        fMask: raw::MIIM_TYPE,
        dwTypeData: label_buf.as_mut_ptr() as *mut _,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { swell.InsertMenuItem(menu, -1, 1, &mut mi) };
}
