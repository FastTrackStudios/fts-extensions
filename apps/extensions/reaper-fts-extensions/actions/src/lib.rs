//! fts-extensions' `#[architect::actions]` trait *declarations* only —
//! pure metadata (id/description/category/group per action), no REAPER
//! bindings and no fts-extensions internals. Split out from
//! `fts-extensions` (which builds `cdylib`-only and pulls the full REAPER
//! binding stack) so that other Rust consumers — namely `fts-cli`'s
//! generated action menu — can depend on the metadata without dragging in
//! anything REAPER-specific.
//!
//! `fts-extensions` itself depends on this crate and implements both
//! traits on its own `FtsActionsImpl` (see
//! `crates/fts-extensions/src/architect_actions.rs`), forwarding each
//! method to the real handler. Consumers that only need `ActionMeta`
//! (e.g. `fts session --actions`) never need an impl at all —
//! `<Trait>Actions::all()` is generated purely from the trait
//! declaration.

/// Tempo-grid move/snap actions — none shown in REAPER's Extensions menu
/// (matches the original `action()`, not `menu_action()`, in
/// `fts-extensions`' legacy `actions::build_action_defs()`).
#[architect::actions(namespace = "FTS_TEMPO")]
pub trait FtsTempoGridActions {
    #[action(
        description = "Move closest measure grid line to mouse cursor (perform until shortcut released)",
        group = "Move"
    )]
    fn move_measure_grid_to_mouse(&self);

    #[action(
        description = "Move closest measure grid line to mouse cursor — constrained (perform until shortcut released)",
        group = "Move"
    )]
    fn move_measure_grid_to_mouse_constrained(&self);

    #[action(
        description = "Move closest measure grid line to mouse cursor — fully constrained (perform until shortcut released)",
        group = "Move"
    )]
    fn move_measure_grid_to_mouse_fully_constrained(&self);

    #[action(
        description = "Move closest grid line to mouse cursor (perform until shortcut released)",
        group = "Move"
    )]
    fn move_grid_to_mouse(&self);

    #[action(
        description = "Move closest tempo marker to mouse cursor (perform until shortcut released)",
        group = "Move"
    )]
    fn move_marker_to_mouse(&self);

    #[action(
        description = "Snap closest measure grid line to next transient",
        group = "Snap"
    )]
    fn snap_grid_to_transient(&self);

    #[action(
        description = "Snap closest measure grid line to next transient — constrained",
        group = "Snap"
    )]
    fn snap_grid_to_transient_constrained(&self);

    #[action(
        description = "Snap closest measure grid line to next transient — fully constrained",
        group = "Snap"
    )]
    fn snap_grid_to_transient_fully_constrained(&self);
}

/// General FTS utility actions — a mix of hidden and Extensions-menu-visible
/// entries; `category` is set only where the original constructor was
/// `menu_action`/`toggle_menu_action` (empty `category` == not shown in
/// menu, per `daw_reaper`'s `ActionBackend` impl).
#[architect::actions(namespace = "FTS")]
pub trait FtsActions {
    #[action(description = "Move cursor left creating time selection by measure")]
    fn move_cursor_left_creating_time_selection_by_measure(&self);

    #[action(description = "Move cursor right creating time selection by measure")]
    fn move_cursor_right_creating_time_selection_by_measure(&self);

    #[action(description = "Split selected items at cursor with crossfade on left")]
    fn split_items_crossfade_left(&self);

    #[action(
        description = "Smart duplicate: duplicate selected items forward by the selection's measure span (group + color preserving, tempo-aware)",
        category = "Editing"
    )]
    fn smart_duplicate(&self);

    #[action(description = "Test Toggle", category = "Test", toggleable = true)]
    fn test_toggle(&self);

    #[action(
        description = "Sync: Toggle clock-sync (multicast peer discovery)",
        category = "Sync"
    )]
    fn clock_sync_toggle(&self);

    #[action(
        description = "Sync: Toggle drift correction (auto-rate-change)",
        category = "Sync"
    )]
    fn drift_correction_toggle(&self);

    #[action(
        description = "MIDI mode: Drums (drum-map view + drum keybinds)",
        category = "MIDI Editor"
    )]
    fn midi_mode_drums(&self);

    #[action(description = "MIDI mode: Cycle to next", category = "MIDI Editor")]
    fn midi_mode_cycle(&self);

    #[action(description = "MIDI: Insert flam at mouse cursor")]
    fn midi_insert_flam(&self);

    #[action(description = "FastTrackStudio Info", category = "Info")]
    fn info(&self);

    #[action(description = "Volume Balancer: toggle constant-sum fader linking")]
    fn volbal_toggle(&self);

    #[action(description = "Volume Balancer: link selected tracks (constant total volume)")]
    fn volbal_link_selected(&self);

    #[action(description = "Volume Balancer: unlink groups containing selected tracks")]
    fn volbal_unlink_selected(&self);
}
