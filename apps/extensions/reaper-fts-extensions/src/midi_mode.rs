//! MIDI editor "modes" — a per-MIDI-editor analogue of session
//! [`mode_actions`](session::mode_actions). Switching into a MIDI mode (e.g.
//! `Drums`) activates a matching reaper-input workflow (`midi-<slug>`) for its
//! keybinds and applies the mode's MIDI-editor setup (see [`crate::midi_mode_input`]).
//!
//! Kept local to fts-extensions rather than in the `session` crate because
//! these modes are MIDI-editor specific. The eventual plan is to switch the
//! active MIDI mode automatically based on the primary MIDI editor track.

use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

/// A MIDI editor mode. Start with `Drums`; add variants as more are designed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiMode {
    /// Drum programming: drum-map note names, triangle note view, flam, etc.
    Drums,
}

impl MidiMode {
    /// All modes, in cycle order.
    pub const ALL: [MidiMode; 1] = [MidiMode::Drums];

    /// Stable lowercase identifier used in workflow ids and action ids.
    pub fn slug(self) -> &'static str {
        match self {
            MidiMode::Drums => "drums",
        }
    }

    /// Reverse of [`MidiMode::slug`]. Case-insensitive, trims whitespace.
    /// Reserved for the planned auto-switch-by-track feature.
    #[allow(dead_code)]
    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug.trim().to_ascii_lowercase().as_str() {
            "drums" => Some(MidiMode::Drums),
            _ => None,
        }
    }

    /// Title-cased display name.
    pub fn display_name(self) -> &'static str {
        match self {
            MidiMode::Drums => "Drums",
        }
    }
}

impl fmt::Display for MidiMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

type Listener = Arc<dyn Fn(MidiMode) + Send + Sync>;

// `None` until a MIDI mode is first entered.
static CURRENT: OnceLock<Mutex<Option<MidiMode>>> = OnceLock::new();
static LISTENERS: OnceLock<Mutex<Vec<Listener>>> = OnceLock::new();

fn current_cell() -> &'static Mutex<Option<MidiMode>> {
    CURRENT.get_or_init(|| Mutex::new(None))
}

fn listeners_cell() -> &'static Mutex<Vec<Listener>> {
    LISTENERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// The currently active MIDI mode, or `None` if none has been entered.
pub fn current_midi_mode() -> Option<MidiMode> {
    *current_cell().lock().unwrap()
}

/// Register a callback fired whenever [`set_midi_mode`] transitions to a
/// different mode. Listeners run outside the state lock (like the session
/// mode system) so they may call back in safely.
pub fn add_midi_mode_change_listener<F>(listener: F)
where
    F: Fn(MidiMode) + Send + Sync + 'static,
{
    if let Ok(mut guard) = listeners_cell().lock() {
        guard.push(Arc::new(listener));
    }
}

fn fire_listeners(mode: MidiMode) {
    let listeners: Vec<Listener> = match listeners_cell().lock() {
        Ok(guard) => guard.clone(),
        Err(_) => return,
    };
    for listener in listeners {
        listener(mode);
    }
}

/// Switch into `mode`. No-op (no listeners fired) if already active.
pub fn set_midi_mode(mode: MidiMode) {
    {
        let mut guard = current_cell().lock().unwrap();
        if *guard == Some(mode) {
            tracing::debug!(midi_mode = %mode, "[midi-mode] set_midi_mode no-op (already active)");
            return;
        }
        *guard = Some(mode);
    }
    fire_listeners(mode);
}

/// Cycle to the next MIDI mode (wraps). Enters the first mode if none active.
pub fn cycle_midi_mode() {
    let all = MidiMode::ALL;
    let next = match current_midi_mode() {
        None => all[0],
        Some(current) => {
            let idx = all.iter().position(|&m| m == current).unwrap_or(0);
            all[(idx + 1) % all.len()]
        }
    };
    set_midi_mode(next);
}
