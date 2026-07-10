//! Extensions → FastTrackStudio submenu.
//!
//! Registers a `hookcustommenu` callback that populates a "FastTrackStudio"
//! submenu under REAPER's Extensions top-level menu. Menu entries are
//! populated dynamically from actions registered with `show_in_menu`.
//!
//! Uses InsertMenuItem (MENUITEMINFO) like helgobox — SWELL_InsertMenu
//! does not work reliably on Linux/SWELL.

use std::sync::OnceLock;

use reaper_high::Reaper;
use reaper_low::{Swell, raw};
use reaper_medium::{Hmenu, HookCustomMenu, MenuHookFlag, ReaperStr};
use tracing::{debug, info};

/// A menu entry: (command_name, display_label).
struct MenuEntry {
    command_name: String,
    label: String,
}

/// Global list of menu entries, populated once at init before the hook is registered.
static MENU_ENTRIES: OnceLock<Vec<MenuEntry>> = OnceLock::new();

/// Populate the menu entries from action defs. Call once before registering the hook.
pub fn set_menu_entries(entries: Vec<(String, String)>) {
    let _ = MENU_ENTRIES.set(
        entries
            .into_iter()
            .map(|(command_name, label)| MenuEntry {
                command_name,
                label,
            })
            .collect(),
    );
}

/// Insert a regular menu item using MENUITEMINFO (helgobox style).
unsafe fn insert_menu_item(menu: raw::HMENU, item_id: u32, text: &str) {
    let swell = Swell::get();
    let mut text_buf: Vec<u8> = text.as_bytes().to_vec();
    text_buf.push(0);
    let mut mi = raw::MENUITEMINFO {
        fMask: raw::MIIM_TYPE | raw::MIIM_DATA | raw::MIIM_ID,
        wID: item_id,
        dwTypeData: text_buf.as_mut_ptr() as *mut _,
        ..std::mem::zeroed()
    };
    swell.InsertMenuItem(menu, -1, 1, &mut mi);
}

/// Insert a submenu using MENUITEMINFO (helgobox style).
unsafe fn insert_submenu(parent: raw::HMENU, submenu: raw::HMENU, text: &str) {
    let swell = Swell::get();
    let mut text_buf: Vec<u8> = text.as_bytes().to_vec();
    text_buf.push(0);
    let mut mi = raw::MENUITEMINFO {
        fMask: raw::MIIM_TYPE | raw::MIIM_DATA | raw::MIIM_SUBMENU,
        hSubMenu: submenu,
        dwTypeData: text_buf.as_mut_ptr() as *mut _,
        ..std::mem::zeroed()
    };
    swell.InsertMenuItem(parent, -1, 1, &mut mi);
}

/// Insert a separator using MENUITEMINFO.
unsafe fn insert_separator(menu: raw::HMENU) {
    let swell = Swell::get();
    let mut mi = raw::MENUITEMINFO {
        fMask: raw::MIIM_TYPE,
        fType: raw::MF_SEPARATOR,
        ..std::mem::zeroed()
    };
    swell.InsertMenuItem(menu, -1, 1, &mut mi);
}

/// The `hookcustommenu` callback. REAPER calls this when populating menus.
pub struct FtsMenuHook;

impl HookCustomMenu for FtsMenuHook {
    fn call(menuidstr: &ReaperStr, menu: Hmenu, flag: MenuHookFlag) {
        if flag != MenuHookFlag::Init {
            return;
        }
        if menuidstr.to_str() != "Main extensions" {
            return;
        }

        let Some(entries) = MENU_ENTRIES.get() else {
            info!("FtsMenuHook: no entries set");
            return;
        };
        if entries.is_empty() {
            info!("FtsMenuHook: entries empty");
            return;
        }

        let swell = Swell::get();
        let reaper = Reaper::get().medium_reaper();

        // Create the "FastTrackStudio" submenu
        let submenu = swell.CreatePopupMenu();
        if submenu.is_null() {
            info!("FtsMenuHook: CreatePopupMenu returned null");
            return;
        }

        // Add each menu item, resolving command IDs at display time
        let mut added = 0;
        for entry in entries {
            let lookup = format!("_{}", entry.command_name);
            let cmd_id = reaper.named_command_lookup(lookup);
            let Some(cmd_id) = cmd_id else {
                debug!("Menu: skipping {} (not registered yet)", entry.command_name);
                continue;
            };

            unsafe {
                insert_menu_item(submenu, cmd_id.get(), &entry.label);
            }
            added += 1;
        }

        info!(
            "FtsMenuHook: added {added}/{} items to submenu",
            entries.len()
        );

        let parent = menu.as_ptr();

        // Add separator before our menu if there are existing items
        let existing = unsafe { swell.GetMenuItemCount(parent) };
        if existing > 0 {
            unsafe { insert_separator(parent) };
        }

        // Insert the "FastTrackStudio" submenu into the Extensions menu
        unsafe {
            insert_submenu(parent, submenu, "FastTrackStudio");
        }

        info!("FtsMenuHook: submenu inserted into Extensions menu");
    }
}
