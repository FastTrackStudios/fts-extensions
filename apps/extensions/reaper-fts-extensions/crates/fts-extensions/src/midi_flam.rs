//! MIDI editor "flam" insertion.
//!
//! A flam is a drum ornament: a soft grace note struck just before the main
//! note. We insert the main note at the mouse cursor using REAPER's own
//! "Insert note at mouse cursor" action (which handles the screen → time/pitch
//! mapping), then add a grace note a fixed ~30 ms earlier at reduced velocity,
//! ending exactly where the main note begins.

use reaper_high::Reaper;

/// Grace note offset before the main note, in seconds. Fixed (tempo
/// independent) for an acoustic-flam feel.
const FLAM_GRACE_OFFSET_SECS: f64 = 0.030;
/// Grace note velocity as a fraction of the main note's velocity.
const FLAM_GRACE_VEL_RATIO: f64 = 0.5;

/// MIDI Editor section command: unselect all events.
const ME_UNSELECT_ALL: i32 = 40214;
/// MIDI Editor section command: insert note at mouse cursor.
const ME_INSERT_NOTE_AT_MOUSE: i32 = 40001;

/// Insert a flam at the mouse cursor in the active MIDI editor.
pub fn insert_flam_at_mouse() {
    let reaper = Reaper::get();
    let medium = reaper.medium_reaper();
    let low = medium.low();

    let Some(editor) = medium.midi_editor_get_active() else {
        reaper.show_console_msg("FTS: Flam — no active MIDI editor\n");
        return;
    };
    let hwnd = editor.as_ptr();

    let take = unsafe { low.MIDIEditor_GetTake(hwnd) };
    if take.is_null() {
        reaper.show_console_msg("FTS: Flam — no active MIDI take\n");
        return;
    }

    // Insert the main note at the mouse, isolated: unselect everything first
    // so the freshly-inserted note is the only selected one and we can find it.
    unsafe {
        low.MIDIEditor_OnCommand(hwnd, ME_UNSELECT_ALL);
        low.MIDIEditor_OnCommand(hwnd, ME_INSERT_NOTE_AT_MOUSE);
    }

    // Locate the just-inserted (only selected) note.
    let mut note_cnt = 0;
    let mut cc_cnt = 0;
    let mut text_cnt = 0;
    unsafe { low.MIDI_CountEvts(take, &mut note_cnt, &mut cc_cnt, &mut text_cnt) };

    let mut main: Option<(f64, i32, i32, i32)> = None; // (start_ppq, chan, pitch, vel)
    for idx in 0..note_cnt {
        let mut selected = false;
        let mut muted = false;
        let mut start_ppq = 0.0;
        let mut end_ppq = 0.0;
        let mut chan = 0;
        let mut pitch = 0;
        let mut vel = 0;
        let ok = unsafe {
            low.MIDI_GetNote(
                take,
                idx,
                &mut selected,
                &mut muted,
                &mut start_ppq,
                &mut end_ppq,
                &mut chan,
                &mut pitch,
                &mut vel,
            )
        };
        if ok && selected {
            main = Some((start_ppq, chan, pitch, vel));
            break;
        }
    }

    // No selected note → the insert didn't land (e.g. mouse not over the
    // piano roll). Nothing to flam; leave the editor untouched.
    let Some((main_start_ppq, chan, pitch, vel)) = main else {
        return;
    };

    // Grace note: FLAM_GRACE_OFFSET_SECS earlier (in project time, so it's a
    // fixed wall-clock offset), at reduced velocity, ending where the main
    // note starts.
    let main_proj_t = unsafe { low.MIDI_GetProjTimeFromPPQPos(take, main_start_ppq) };
    let grace_proj_t = main_proj_t - FLAM_GRACE_OFFSET_SECS;
    let grace_start_ppq = unsafe { low.MIDI_GetPPQPosFromProjTime(take, grace_proj_t) };
    let grace_vel = ((vel as f64 * FLAM_GRACE_VEL_RATIO).round() as i32).clamp(1, 127);

    let no_sort = false;
    unsafe {
        low.MIDI_InsertNote(
            take,
            false, // selected
            false, // muted
            grace_start_ppq,
            main_start_ppq,
            chan,
            pitch,
            grace_vel,
            &no_sort,
        );
        low.MIDI_Sort(take);
    }
}
