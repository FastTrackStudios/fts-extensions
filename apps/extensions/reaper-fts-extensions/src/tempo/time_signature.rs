//! Time-signature insertion actions.
//!
//! `insert_time_signature_at_cursor` drops a time-signature marker at the
//! start of the measure containing the edit cursor — always flooring, so a
//! cursor 99% through measure 4 still targets measure 4.
//!
//! Holding **Shift** while the action fires makes it a *single-measure*
//! insert: the signature applies for exactly one measure, then a second
//! marker restores whatever signature was in effect before (4/4 → one bar of
//! 2/4 → back to 4/4). The Shift check reads the live keyboard state, so the
//! same action works both ways from one binding.

use std::ffi::CString;

/// Virtual-key code for Shift (same value on every SWELL platform).
const VK_SHIFT: i32 = 0x10;

/// The signatures we register insert actions for. Mirrored in
/// `actions::build_action_defs`.
pub const TIME_SIGNATURES: &[(i32, i32)] = &[
    (2, 4),
    (3, 4),
    (4, 4),
    (5, 4),
    (6, 4),
    (7, 4),
    (3, 8),
    (5, 8),
    (6, 8),
    (7, 8),
    (9, 8),
    (12, 8),
    (13, 8),
];

fn shift_held() -> bool {
    let swell = reaper_low::Swell::get();
    swell.GetAsyncKeyState(VK_SHIFT) & 0x8000 != 0
}

/// Signature + tempo in effect at a measure: `(num, denom, tempo, start_time)`.
fn measure_info(measure: i32) -> (i32, i32, f64, f64) {
    let low = reaper_low::Reaper::get();
    let mut qn_start = 0.0f64;
    let mut qn_end = 0.0f64;
    let mut num = 0i32;
    let mut denom = 0i32;
    let mut tempo = 0.0f64;
    let start_time = unsafe {
        low.TimeMap_GetMeasureInfo(
            std::ptr::null_mut(),
            measure,
            &mut qn_start,
            &mut qn_end,
            &mut num,
            &mut denom,
            &mut tempo,
        )
    };
    (num, denom, tempo, start_time)
}

/// Marker fields from `GetTempoTimeSigMarker`.
struct TempoMarker {
    measurepos: i32,
    beatpos: f64,
    bpm: f64,
    timesig_num: i32,
    lineartempo: bool,
}

fn get_marker(idx: i32) -> Option<TempoMarker> {
    let low = reaper_low::Reaper::get();
    let mut timepos = 0.0f64;
    let mut measurepos = 0i32;
    let mut beatpos = 0.0f64;
    let mut bpm = 0.0f64;
    let mut num = 0i32;
    let mut denom = 0i32;
    let mut lineartempo = false;
    let ok = unsafe {
        low.GetTempoTimeSigMarker(
            std::ptr::null_mut(),
            idx,
            &mut timepos,
            &mut measurepos,
            &mut beatpos,
            &mut bpm,
            &mut num,
            &mut denom,
            &mut lineartempo,
        )
    };
    ok.then_some(TempoMarker {
        measurepos,
        beatpos,
        bpm,
        timesig_num: num,
        lineartempo,
    })
}

/// Index of an existing tempo/timesig marker sitting exactly on `measure`'s
/// downbeat, if any.
fn marker_at_measure(measure: i32) -> Option<(i32, TempoMarker)> {
    let low = reaper_low::Reaper::get();
    let (.., start_time) = measure_info(measure);
    // Small forward bias so a marker exactly on the boundary is found even
    // with float noise in the computed measure start.
    let idx = unsafe { low.FindTempoTimeSigMarker(std::ptr::null_mut(), start_time + 1e-9) };
    if idx < 0 {
        return None;
    }
    let marker = get_marker(idx)?;
    (marker.measurepos == measure && marker.beatpos.abs() < 1e-6).then_some((idx, marker))
}

/// Insert (or update) a time-signature marker on `measure`'s downbeat.
///
/// An existing marker on the downbeat is edited in place (keeping its tempo
/// and ramp shape); otherwise a new marker is added carrying the tempo
/// already in effect there, so the tempo map is unchanged.
fn upsert_timesig(measure: i32, num: i32, denom: i32) -> bool {
    let low = reaper_low::Reaper::get();
    if let Some((idx, marker)) = marker_at_measure(measure) {
        unsafe {
            low.SetTempoTimeSigMarker(
                std::ptr::null_mut(),
                idx,
                -1.0,
                measure,
                0.0,
                marker.bpm,
                num,
                denom,
                marker.lineartempo,
            )
        }
    } else {
        let (.., tempo, _) = measure_info(measure);
        unsafe {
            low.SetTempoTimeSigMarker(
                std::ptr::null_mut(),
                -1,
                -1.0,
                measure,
                0.0,
                tempo,
                num,
                denom,
                false,
            )
        }
    }
}

/// Insert a `num/denom` time signature at the measure containing the edit
/// cursor. With Shift held, the signature lasts a single measure and the
/// previous signature is restored on the next downbeat.
pub fn insert_time_signature_at_cursor(num: i32, denom: i32) {
    insert_impl(num, denom, shift_held());
}

/// One-shot variant: always inserts a single measure of `num/denom` and
/// restores the previous signature on the next downbeat, regardless of
/// modifier state. Used by key sequences (e.g. `T 2 4`) where Shift can't be
/// reliably held through the whole chord sequence.
pub fn insert_single_measure_time_signature(num: i32, denom: i32) {
    insert_impl(num, denom, true);
}

fn insert_impl(num: i32, denom: i32, single_measure: bool) {
    let low = reaper_low::Reaper::get();
    let proj = std::ptr::null_mut();

    let cursor = low.GetCursorPosition();
    let mut measure = 0i32;
    unsafe {
        low.TimeMap2_timeToBeats(
            proj,
            cursor,
            &mut measure,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    // Signature in effect *before* the edit — what a single-measure insert
    // restores.
    let (prev_num, prev_denom, ..) = measure_info(measure);

    unsafe { low.Undo_BeginBlock2(proj) };

    let mut ok = upsert_timesig(measure, num, denom);

    if ok && single_measure && (prev_num, prev_denom) != (num, denom) {
        // Restore the previous signature one measure later — unless the
        // project already changes signature there, which takes precedence.
        let next = measure + 1;
        let already_changes = marker_at_measure(next).is_some_and(|(_, m)| m.timesig_num > 0);
        if !already_changes {
            ok &= upsert_timesig(next, prev_num, prev_denom);
        }
    }

    let desc = if single_measure {
        format!("Insert single measure of {num}/{denom}")
    } else {
        format!("Insert {num}/{denom} time signature")
    };
    let c_desc = CString::new(desc).unwrap_or_default();
    unsafe { low.Undo_EndBlock2(proj, c_desc.as_ptr(), -1) };

    low.UpdateTimeline();

    if !ok {
        reaper_high::Reaper::get()
            .show_console_msg(format!("FTS: failed to insert {num}/{denom} marker\n"));
    }
}
