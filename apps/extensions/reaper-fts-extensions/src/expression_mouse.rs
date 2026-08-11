//! The user's REAPER mouse map, loaded into the expression editor.
//!
//! REAPER keeps every mouse-modifier override the user made in
//! `reaper-mouse.ini`, and `reaper-input` already decodes its MIDI
//! contexts and behaviour ids into typed enums. The expression editor's
//! [`MouseMap`] was deliberately shaped after the same system
//! (`expression-editor-core/src/mouse.rs`), so this module is the small
//! crosswalk the two were designed to meet at: parse the ini once, map
//! each decoded behaviour onto the editor action of the same meaning,
//! and register the result as the editor's host overlay.
//!
//! Only bindings present in the ini are overlaid — REAPER writes a
//! context section only once the user changed something in it, and a
//! missing entry means "REAPER factory default", which is not the same
//! thing as the editor's own default for that slot. Behaviours with no
//! editor equivalent (scrub preview, draw-channel cycling) are left
//! alone rather than mapped to nothing.

use expression_editor_core::mouse::{Action, Context, Gesture, ModKey, MouseMap};
use reaper_input::input::mouse_modifiers::behaviors::midi::{
    MidiNoteClickBehavior, MidiNoteDoubleClickBehavior, MidiNoteEdgeBehavior,
    MidiNoteLeftDragBehavior, MidiPianoRollClickBehavior, MidiPianoRollDoubleClickBehavior,
    MidiPianoRollLeftDragBehavior,
};
use reaper_input::input::mouse_modifiers::behaviors::shared::traits::BehaviorId;
use std::sync::OnceLock;

/// Install the overlay. Call once at extension startup, before the
/// first take is loaded.
pub fn install() {
    expression_editor_core::mouse::set_host_overlay(overlay);
    tracing::info!(
        bindings = parsed().len(),
        "Expression editor mouse map: reaper-mouse.ini overlay installed"
    );
}

/// One decoded ini binding, in the editor's vocabulary.
struct Bound {
    context: Context,
    gesture: Gesture,
    mods: ModKey,
    action: Action,
}

fn overlay(mut map: MouseMap) -> MouseMap {
    for b in parsed() {
        map.set(b.context, b.gesture, ModKey::from_bits(b.mods.bits()), b.action);
    }
    map
}

/// The ini, parsed and crosswalked once. The file only changes when the
/// user edits mouse modifiers in REAPER's preferences; a restart to
/// pick that up matches how REAPER itself treats most of the file.
fn parsed() -> &'static [Bound] {
    static PARSED: OnceLock<Vec<Bound>> = OnceLock::new();
    PARSED.get_or_init(|| {
        let path = reaper_high::Reaper::get()
            .resource_path()
            .as_std_path()
            .join("reaper-mouse.ini");
        match std::fs::read_to_string(&path) {
            Ok(text) => crosswalk(&text),
            Err(e) => {
                tracing::info!("no reaper-mouse.ini to overlay ({e}); editor keeps its presets");
                Vec::new()
            }
        }
    })
}

/// Parse the ini and translate every entry the editor understands.
fn crosswalk(ini: &str) -> Vec<Bound> {
    let mut out = Vec::new();
    let mut section: Option<&str> = None;
    for line in ini.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix('[') {
            section = Some(rest.trim_end_matches(']')).filter(|s| s.starts_with("MM_CTX_MIDI"));
            continue;
        }
        let Some(ctx) = section else { continue };
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let Some(n) = k.strip_prefix("mm_").and_then(|s| s.parse::<u8>().ok()) else {
            continue;
        };
        // Win/Super combos (bit 3) have no editor slot.
        if n >= 8 {
            continue;
        }
        let Some(id) = v.trim().split_whitespace().next().and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if let Some((context, gesture, action)) = translate(ctx, id) {
            out.push(Bound {
                context,
                gesture,
                mods: ModKey::from_bits(n),
                action,
            });
        }
    }
    out
}

/// (ini section, behaviour id) → the editor binding it means.
fn translate(ctx: &str, id: u32) -> Option<(Context, Gesture, Action)> {
    match ctx {
        "MM_CTX_MIDI_PIANOROLL" => Some((
            Context::PianoRoll,
            Gesture::Drag,
            piano_roll_drag(MidiPianoRollLeftDragBehavior::from_behavior_id(id))?,
        )),
        "MM_CTX_MIDI_PIANOROLL_CLK" => Some((
            Context::PianoRoll,
            Gesture::Click,
            piano_roll_click(MidiPianoRollClickBehavior::from_behavior_id(id))?,
        )),
        "MM_CTX_MIDI_PIANOROLL_DBLCLK" => Some((
            Context::PianoRoll,
            Gesture::DoubleClick,
            piano_roll_dblclick(MidiPianoRollDoubleClickBehavior::from_behavior_id(id))?,
        )),
        "MM_CTX_MIDI_NOTE" => Some((
            Context::Note,
            Gesture::Drag,
            note_drag(MidiNoteLeftDragBehavior::from_behavior_id(id))?,
        )),
        "MM_CTX_MIDI_NOTE_CLK" => Some((
            Context::Note,
            Gesture::Click,
            note_click(MidiNoteClickBehavior::from_behavior_id(id))?,
        )),
        "MM_CTX_MIDI_NOTE_DBLCLK" => Some((
            Context::Note,
            Gesture::DoubleClick,
            note_dblclick(MidiNoteDoubleClickBehavior::from_behavior_id(id))?,
        )),
        "MM_CTX_MIDI_NOTEEDGE" => Some((
            Context::NoteEdge,
            Gesture::Drag,
            note_edge(MidiNoteEdgeBehavior::from_behavior_id(id))?,
        )),
        _ => None,
    }
}

fn piano_roll_drag(b: MidiPianoRollLeftDragBehavior) -> Option<Action> {
    use MidiPianoRollLeftDragBehavior as B;
    Some(match b {
        B::NoAction => Action::None,
        B::InsertNoteDragToExtendOrChangePitch | B::InsertNoteDragToExtend => {
            Action::InsertNoteDragToExtend
        }
        B::InsertNoteIgnoringSnapDragToExtendOrChangePitch
        | B::InsertNoteIgnoringSnapDragToExtend => Action::InsertNoteDragToExtendNoSnap,
        B::InsertNoteDragToMove
        | B::InsertNoteIgnoringSnapDragToMove
        | B::InsertNoteIgnoringScaleKeyDragToMove
        | B::InsertNoteIgnoringSnapAndScaleKeyDragToMove => Action::InsertNoteDragToMove,
        B::InsertNote => Action::InsertNote,
        B::InsertNoteIgnoringSnap => Action::InsertNoteNoSnap,
        B::InsertNoteDragToEditVelocity | B::InsertNoteIgnoringSnapDragToEditVelocity => {
            Action::InsertNoteDragToEditVelocity
        }
        B::EraseNotes => Action::EraseNotes,
        B::PaintNotes | B::PaintNotesAndChords => Action::PaintNotes,
        B::PaintNotesIgnoringSnap => Action::PaintNotesNoSnap,
        B::PaintARowOfNotesOfTheSamePitch => Action::PaintRowOfNotes,
        B::MarqueeSelectNotes | B::MarqueeSelectNotesAndTime => Action::MarqueeSelect,
        B::MarqueeSelectNotesAndTimeIgnoringSnap => Action::MarqueeSelect,
        B::MarqueeToggleNoteSelection => Action::MarqueeToggle,
        B::MarqueeAddToNoteSelection => Action::MarqueeAdd,
        B::SelectNotesTouchedWhileDragging => Action::SelectTouched,
        B::ToggleSelectionForNotesTouchedWhileDragging => Action::ToggleSelectTouched,
        B::MoveSelectedNotes => Action::MoveNote,
        B::MoveSelectedNotesIgnoringSnap => Action::MoveNoteNoSnap,
        B::CopySelectedNotes => Action::CopyNote,
        B::CopySelectedNotesIgnoringSnap => Action::CopyNoteNoSnap,
        // Scrub, line-paint, stack-paint, time-select: no editor
        // equivalent yet — leave the mode's own binding in place.
        _ => return None,
    })
}

fn piano_roll_click(b: MidiPianoRollClickBehavior) -> Option<Action> {
    use MidiPianoRollClickBehavior as B;
    Some(match b {
        B::NoAction => Action::None,
        B::DeselectAllNotes => Action::DeselectAll,
        B::DeselectAllNotesAndMoveEditCursor
        | B::DeselectAllNotesAndMoveEditCursorIgnoringSnap => Action::DeselectAll,
        B::InsertNote | B::InsertNoteLeavingOtherNotesSelected => Action::InsertNote,
        B::InsertNoteIgnoringSnap => Action::InsertNoteNoSnap,
        _ => return None,
    })
}

fn piano_roll_dblclick(b: MidiPianoRollDoubleClickBehavior) -> Option<Action> {
    use MidiPianoRollDoubleClickBehavior as B;
    Some(match b {
        B::NoAction => Action::None,
        B::InsertNote => Action::InsertNote,
        B::InsertNoteIgnoringSnap => Action::InsertNoteNoSnap,
        _ => return None,
    })
}

fn note_drag(b: MidiNoteLeftDragBehavior) -> Option<Action> {
    use MidiNoteLeftDragBehavior as B;
    Some(match b {
        B::NoAction => Action::None,
        B::MoveNote => Action::MoveNote,
        B::MoveNoteIgnoringSnap => Action::MoveNoteNoSnap,
        B::MoveNoteOnOneAxisOnly | B::MoveNoteOnOneAxisOnlyIgnoringSnap => Action::MoveNoteOneAxis,
        B::MoveNoteHorizontally | B::MoveNoteHorizontallyIgnoringSnap => {
            Action::MoveNoteHorizontally
        }
        B::MoveNoteVertically | B::MoveNoteVerticallyIgnoringScaleKey => Action::MoveNoteVertically,
        B::MoveNoteIgnoringSelection | B::MoveNoteIgnoringSnapAndSelection => {
            Action::MoveNoteIgnoringSelection
        }
        B::CopyNote | B::CopyNoteHorizontally | B::CopyNoteVertically => Action::CopyNote,
        B::CopyNoteIgnoringSnap | B::CopyNoteHorizontallyIgnoringSnap => Action::CopyNoteNoSnap,
        B::EditNoteVelocity => Action::EditNoteVelocity,
        B::EditNoteVelocityFine => Action::EditNoteVelocityFine,
        B::EraseNotes => Action::EraseNotes,
        B::MarqueeSelectNotes | B::MarqueeSelectNotesAndTime => Action::MarqueeSelect,
        B::MarqueeSelectNotesAndTimeIgnoringSnap => Action::MarqueeSelect,
        B::MarqueeToggleNoteSelection => Action::MarqueeToggle,
        B::MarqueeAddToNoteSelection => Action::MarqueeAdd,
        B::SelectNotesTouchedWhileDragging => Action::SelectTouched,
        B::ToggleSelectionForNotesTouchedWhileDragging => Action::ToggleSelectTouched,
        B::StretchNotePositionsIgnoringSnapArpeggiate => Action::StretchNotePositions,
        B::StretchNoteLengthsIgnoringSnapArpeggiatorLegato
        | B::StretchNoteLengthsArpeggiateLegato => Action::StretchNotes,
        _ => return None,
    })
}

fn note_click(b: MidiNoteClickBehavior) -> Option<Action> {
    use MidiNoteClickBehavior as B;
    Some(match b {
        B::NoAction => Action::None,
        B::SelectNote
        | B::SelectNoteAndMoveEditCursor
        | B::SelectNoteAndMoveEditCursorIgnoringSnap => Action::SelectNote,
        B::ToggleNoteSelection => Action::ToggleNoteSelection,
        B::AddNoteToSelection => Action::AddNoteToSelection,
        B::EraseNote => Action::EraseNote,
        B::ToggleNoteMute => Action::ToggleNoteMute,
        B::SetNoteChannelHigher => Action::SetNoteChannelHigher,
        B::SetNoteChannelLower => Action::SetNoteChannelLower,
        B::DoubleNoteLength => Action::DoubleNoteLength,
        B::HalveNoteLength => Action::HalveNoteLength,
        B::SelectNoteAndAllLaterNotes | B::AddNoteAndAllLaterNotesToSelection => {
            Action::SelectNoteAndLater
        }
        B::SelectNoteAndAllLaterNotesOfSamePitch
        | B::AddNoteAndAllLaterNotesOfSamePitchToSelection => Action::SelectNoteAndLaterSameRow,
        B::SelectAllNotesInMeasure | B::AddAllNotesInMeasureToSelection => {
            Action::SelectAllInMeasure
        }
        B::Unknown(_) => return None,
    })
}

fn note_dblclick(b: MidiNoteDoubleClickBehavior) -> Option<Action> {
    use MidiNoteDoubleClickBehavior as B;
    Some(match b {
        B::NoAction => Action::None,
        B::EraseNote => Action::EraseNote,
        _ => return None,
    })
}

fn note_edge(b: MidiNoteEdgeBehavior) -> Option<Action> {
    use MidiNoteEdgeBehavior as B;
    Some(match b {
        B::NoAction => Action::None,
        B::MoveNoteEdge
        | B::MoveNoteEdgeIgnoringSelection => Action::MoveNoteEdge,
        B::MoveNoteEdgeIgnoringSnap | B::MoveNoteEdgeIgnoringSnapAndSelection => {
            Action::MoveNoteEdgeNoSnap
        }
        B::StretchNotes | B::StretchNotesIgnoringSnap => Action::StretchNotes,
        B::Unknown(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The crosswalk on a real excerpt: the shapes this file's parser
    /// must survive are exactly the ones REAPER writes.
    #[test]
    fn parses_and_translates_a_real_excerpt() {
        let ini = "[hasimported]\nMM_CTX_MIDI_PIANOROLL=1\n\
                   [MM_CTX_MIDI_PIANOROLL]\nmm_0=7 m\nmm_2=12 m\nmm_6=3 m\nmm_9=1 m\n\
                   [MM_CTX_MIDI_NOTE]\nmm_0=1 m\nmm_2=9 m\n\
                   [MM_CTX_ITEM]\nmm_0=2 m\n";
        let bounds = crosswalk(ini);
        let find = |ctx, mods: u8| {
            bounds
                .iter()
                .find(|b| b.context == ctx && b.mods.bits() == mods)
                .map(|b| b.action)
        };
        // 7 = marquee select; 12 = insert-drag-to-move (the ctrl-draw
        // binding this bridge exists for); 3 = erase.
        assert_eq!(find(Context::PianoRoll, 0), Some(Action::MarqueeSelect));
        assert_eq!(
            find(Context::PianoRoll, 2),
            Some(Action::InsertNoteDragToMove)
        );
        assert_eq!(find(Context::PianoRoll, 6), Some(Action::EraseNotes));
        assert_eq!(find(Context::Note, 0), Some(Action::MoveNote));
        assert_eq!(find(Context::Note, 2), Some(Action::EditNoteVelocity));
        // mm_9 carries the Win bit and must be dropped; MM_CTX_ITEM is
        // not a MIDI context and must be ignored entirely.
        assert!(bounds.iter().all(|b| b.mods.bits() < 8));
        assert_eq!(bounds.len(), 5);
    }
}
