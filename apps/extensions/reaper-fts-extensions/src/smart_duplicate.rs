//! Smart duplicate — measure-relative item duplication.
//!
//! Port of MPL's ReaScript "Smart duplicate items, use measure shift"
//! (forum.cockos.com/showthread.php?t=188335, v1.34). Duplicates the
//! selected items forward by the *measure span* of the selection rather
//! than by a raw time offset, so copies land on the same beat of a later
//! measure even across tempo/time-signature changes. Duplication is the
//! DAW's native clone, so takes / fades / group IDs / colors carry over. If
//! a shifted copy would still overlap the source selection, the shift is
//! bumped by one extra measure. Selection moves onto the new copies so a
//! repeated Ctrl+D walks one item forward 1 → 2 → 3 → 4.
//!
//! Written entirely against the `daw` crate's synchronous service traits
//! (`daw::service::{Items, PositionConversion, Projects}`) dispatched on the
//! host-provided REAPER backend (`daw_reaper::Reaper`) — no async, no raw
//! FFI. Same pattern as `volume_balancer`'s `ExtState` use.

use daw::service::{
    ItemRef, Items, MeasureMode, PositionConversion, PositionInBeats, PositionInSeconds,
    ProjectContext, Projects,
};
use tracing::info;

const UNDO_LABEL: &str = "Smart duplicate items";

/// The backend the host installs — a zero-sized unit that implements every
/// `daw::service` trait against the live REAPER project.
fn daw() -> daw_reaper::Reaper {
    daw_reaper::Reaper
}

fn ctx() -> ProjectContext {
    ProjectContext::Current
}

/// A selected item reduced to the measure/beat coordinates the shift needs.
struct Coord {
    guid: String,
    /// Start position as beats-within-its-measure + that measure's index.
    beats_since: f64,
    measure: i32,
    /// End measure index and end position in cumulative beats.
    end_measure: i32,
    end_fullbeats: f64,
}

/// Entry point wired to the `smart_duplicate` architect action.
pub fn smart_duplicate() {
    let daw = daw();
    let selected = daw.get_selected_items(ctx());
    if selected.is_empty() {
        return;
    }

    let coords: Vec<Coord> = selected
        .iter()
        .map(|it| {
            let start = daw.time_to_beats(ctx(), it.position, MeasureMode::IgnoreMeasure);
            let end_time =
                PositionInSeconds::from_seconds(it.position.as_seconds() + it.length.as_seconds());
            let end = daw.time_to_beats(ctx(), end_time, MeasureMode::IgnoreMeasure);
            Coord {
                guid: it.guid.clone(),
                beats_since: start.beats_since_measure.as_beats(),
                measure: start.measure_index,
                end_measure: end.measure_index,
                end_fullbeats: end.full_beats.as_beats(),
            }
        })
        .collect();

    let (measure_shift, end_fullbeatsmax) = measure_span(&coords);
    let shift = measure_shift + overlap_increment(&coords, measure_shift, end_fullbeatsmax);

    daw.begin_undo_block(ctx(), UNDO_LABEL);

    let mut new_guids = Vec::with_capacity(coords.len());
    for c in &coords {
        // Native clone (preserves takes / fades / group / color); it leaves
        // the fresh copy as the project's sole selection, so we read it back
        // to get its real GUID.
        if daw
            .duplicate_item(ctx(), ItemRef::Guid(c.guid.clone()))
            .is_none()
        {
            continue;
        }
        let Some(copy) = daw.get_selected_items(ctx()).into_iter().next() else {
            continue;
        };
        let pos = daw.beats_to_time(
            ctx(),
            PositionInBeats::from_beats(c.beats_since),
            MeasureMode::FromMeasureAtIndex(c.measure + shift),
        );
        let _ = daw.set_position(ctx(), ItemRef::Guid(copy.guid.clone()), pos);
        new_guids.push(copy.guid);
    }

    // Move the selection onto the new copies only, so a repeated Ctrl+D
    // walks a single item forward (1 → 2 → 3 → 4) instead of duplicating a
    // growing selection (2 → 4 → 8).
    let _ = daw.select_all_items(ctx(), false);
    for guid in &new_guids {
        let _ = daw.set_selected(ctx(), ItemRef::Guid(guid.clone()), true);
    }

    daw.end_undo_block(ctx(), UNDO_LABEL, None);

    info!(items = new_guids.len(), "Smart duplicate items");
}

/// Measure span of the selection (min shift is one measure), plus the
/// largest end-position in cumulative beats (for the overlap check).
fn measure_span(coords: &[Coord]) -> (i32, f64) {
    let mut meas_min = i32::MAX;
    let mut meas_max = 0;
    let mut end_fullbeatsmax = 0.0_f64;

    for c in coords {
        meas_min = meas_min.min(c.measure);
        meas_max = meas_max.max(c.end_measure);
        end_fullbeatsmax = end_fullbeatsmax.max(c.end_fullbeats);
    }

    ((meas_max - meas_min).max(1), end_fullbeatsmax)
}

/// Return 1 if any shifted copy would start before the source selection
/// ends (so the shift needs one more measure), else 0.
fn overlap_increment(coords: &[Coord], measure_shift: i32, end_fullbeatsmax: f64) -> i32 {
    let daw = daw();
    let sel_end = daw
        .beats_to_time(
            ctx(),
            PositionInBeats::from_beats(end_fullbeatsmax),
            MeasureMode::IgnoreMeasure,
        )
        .as_seconds();
    for c in coords {
        let shifted = daw
            .beats_to_time(
                ctx(),
                PositionInBeats::from_beats(c.beats_since),
                MeasureMode::FromMeasureAtIndex(c.measure + measure_shift),
            )
            .as_seconds();
        if shifted < sel_end {
            return 1;
        }
    }
    0
}
