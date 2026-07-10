//! Rename REAPER floating toolbars to match the session mode layout.
//!
//! The 8 session modes each own 3 floating toolbars. On extension load we
//! ensure the first 24 floating toolbar sections in `reaper-menu.ini` have
//! titles like `Organize 1`, `Organize 2`, `Organize 3`, `Write 1`, ...
//! REAPER reads the file at startup, so renames take effect on the *next*
//! launch — this just makes the slots discoverable + addressable by name.

use reaper_config::menu::{
    ActionItem, CommandId, IconSpec, MenuConfig, MenuItem, MenuSection, MenuSectionId,
};
use session::mode_actions::Mode;
use std::path::Path;

/// Number of floating toolbars reserved per mode that opts in via
/// [`Mode::has_toolbars`]. Currently 9 modes claim 3 toolbars each →
/// 27 of REAPER's 32 slots; the remaining 5 stay user-customisable.
/// `Mode::Scoring` is the lone opt-out — it shares no toolbars.
pub const TOOLBARS_PER_MODE: u32 = 3;

/// Total floating toolbar slots managed by the mode system.
pub const RESERVED_TOOLBAR_COUNT: u32 = (Mode::ALL.len() as u32) * TOOLBARS_PER_MODE;

/// Map a 1-based floating-toolbar index to a [`MenuSectionId`]. The typed
/// variants only model toolbars 1..=16; higher indices fall back to
/// [`MenuSectionId::Other`] using the same `Floating toolbar N` header
/// REAPER itself writes, so round-trips are still identity-preserving.
fn floating_toolbar(n: u32) -> MenuSectionId {
    match n {
        1 => MenuSectionId::FloatingToolbar1,
        2 => MenuSectionId::FloatingToolbar2,
        3 => MenuSectionId::FloatingToolbar3,
        4 => MenuSectionId::FloatingToolbar4,
        5 => MenuSectionId::FloatingToolbar5,
        6 => MenuSectionId::FloatingToolbar6,
        7 => MenuSectionId::FloatingToolbar7,
        8 => MenuSectionId::FloatingToolbar8,
        9 => MenuSectionId::FloatingToolbar9,
        10 => MenuSectionId::FloatingToolbar10,
        11 => MenuSectionId::FloatingToolbar11,
        12 => MenuSectionId::FloatingToolbar12,
        13 => MenuSectionId::FloatingToolbar13,
        14 => MenuSectionId::FloatingToolbar14,
        15 => MenuSectionId::FloatingToolbar15,
        16 => MenuSectionId::FloatingToolbar16,
        _ => MenuSectionId::Other(format!("Floating toolbar {n}")),
    }
}

/// Desired `(section, title)` for every mode toolbar slot. Ordering is
/// mode-major: Organize 1..3, Write 1..3, ..., Live 1..3.
fn desired_titles() -> Vec<(MenuSectionId, String)> {
    let mut out = Vec::with_capacity(RESERVED_TOOLBAR_COUNT as usize);
    // Walk only modes that own toolbars (skips `Scoring`). Keep the
    // overall slot-index continuous so the toolbar-N numbering stays
    // dense — Scoring's 3 would-be slots simply don't get claimed.
    let mut n = 0u32;
    for mode in Mode::ALL.iter().filter(|m| m.has_toolbars()) {
        for slot in 1..=TOOLBARS_PER_MODE {
            n += 1;
            out.push((
                floating_toolbar(n),
                format!("{} {}", mode.display_name(), slot),
            ));
        }
    }
    out
}

/// Apply the mode toolbar naming scheme to `<resource_path>/reaper-menu.ini`.
///
/// Idempotent: only writes when a title is missing or different. Returns
/// `Ok(true)` if the file was modified, `Ok(false)` if nothing changed.
/// Missing file is treated as an empty config (caller can still seed all
/// 24 sections).
pub fn rename_for_modes(resource_path: &Path) -> std::io::Result<bool> {
    let menu_path = resource_path.join("reaper-menu.ini");
    let original = std::fs::read_to_string(&menu_path).unwrap_or_default();
    let mut config = MenuConfig::parse(&original)
        .map_err(|e| std::io::Error::other(format!("parse reaper-menu.ini: {e}")))?;

    let mut changed = false;
    for (id, title) in desired_titles() {
        match config.section_mut(&id) {
            Some(section) => {
                if section.title.as_deref() != Some(title.as_str()) {
                    section.title = Some(title.clone());
                    changed = true;
                }
                if ensure_label_first(section, &title) {
                    changed = true;
                }
            }
            None => {
                config.sections.push(MenuSection {
                    id,
                    title: Some(title.clone()),
                    items: vec![MenuItem::Label { text: title }],
                });
                changed = true;
            }
        }
    }

    if ensure_main_toolbar_mode_selector(&mut config) {
        changed = true;
    }

    if changed {
        std::fs::write(&menu_path, config.serialize())?;
    }
    Ok(changed)
}

/// Make sure the section's first item is a non-clickable label with the
/// given text — gives every mode toolbar a visible name so you can tell
/// which toolbar is which without hovering. Returns `true` if `items`
/// was modified.
fn ensure_label_first(section: &mut MenuSection, label_text: &str) -> bool {
    if let Some(MenuItem::Label { text }) = section.items.first() {
        if text == label_text {
            return false;
        }
    }
    // Replace any pre-existing label-at-position-0; otherwise prepend.
    let already_has_label_at_zero = matches!(section.items.first(), Some(MenuItem::Label { .. }));
    if already_has_label_at_zero {
        section.items[0] = MenuItem::Label {
            text: label_text.to_string(),
        };
    } else {
        section.items.insert(
            0,
            MenuItem::Label {
                text: label_text.to_string(),
            },
        );
    }
    true
}

/// Named command id that REAPER uses in `reaper-menu.ini` to reference the
/// mode selector action. REAPER prefixes registered extension actions with
/// `_` in the menu file; the bare command name (no underscore) is what we
/// register via `register_action_main_thread`.
const MODE_SELECTOR_NAMED_COMMAND: &str = "_FTS_MODE_SELECTOR";

/// Ensure the main toolbar has a `_FTS_MODE_SELECTOR` button rendered as a
/// text label. Returns `true` if the config was modified.
///
/// **Does not create the `[Main toolbar]` section from scratch.** REAPER
/// falls back to its built-in default main toolbar when the section is
/// absent; writing a fresh single-item section overrides that default and
/// makes the toolbar appear empty. We only append to a section the user
/// has already customised. If a previous run of this function created a
/// section containing only our selector, self-heal by removing it so the
/// built-in default returns.
fn ensure_main_toolbar_mode_selector(config: &mut MenuConfig) -> bool {
    if let Some(section) = config.section(&MenuSectionId::MainToolbar) {
        let only_our_selector = section.items.len() == 1
            && matches!(
                &section.items[0],
                MenuItem::Action(a)
                    if matches!(&a.command, CommandId::Named(s) if s == MODE_SELECTOR_NAMED_COMMAND)
            );
        if only_our_selector {
            config
                .sections
                .retain(|s| s.id != MenuSectionId::MainToolbar);
            tracing::info!(
                "Removed stray `[Main toolbar]` section that only held our \
                 mode selector — REAPER's built-in default toolbar will \
                 return on next launch"
            );
            return true;
        }
    }

    let section = match config.section_mut(&MenuSectionId::MainToolbar) {
        Some(s) => s,
        None => {
            // User hasn't customised the main toolbar. Don't override the
            // built-in by creating a section here — they can add the
            // selector through Customize Toolbars or it'll get picked up
            // automatically once they customise.
            tracing::info!(
                "Skipping main-toolbar mode selector injection: REAPER is \
                 using its built-in default main toolbar (no `[Main toolbar]` \
                 section in reaper-menu.ini)"
            );
            return false;
        }
    };

    let already = section.items.iter().any(|item| {
        matches!(
            item,
            MenuItem::Action(a)
                if matches!(&a.command, CommandId::Named(s) if s == MODE_SELECTOR_NAMED_COMMAND)
        )
    });
    if already {
        return false;
    }

    section.items.push(MenuItem::Action(ActionItem {
        command: CommandId::Named(MODE_SELECTOR_NAMED_COMMAND.to_string()),
        title: Some("Mode".to_string()),
        icon: Some(IconSpec::Text),
        flags: 0,
    }));
    true
}
