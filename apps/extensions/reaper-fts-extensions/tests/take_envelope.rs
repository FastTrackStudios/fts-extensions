//! REAPER integration: take envelopes, and the dynamics write that
//! rides on them.
//!
//! This is the one part of the audio editor with no standalone
//! equivalent to lean on, because the behaviour under test *is*
//! REAPER's:
//!
//! - **There is no API to create a take envelope.** You bring one into
//!   existence by running the action a user would (`40693` for volume),
//!   which acts on the *selected* item — so the backend has to select
//!   the item, run the action, and put the previous selection back.
//!   Nothing about that dance is visible to a unit test.
//! - **A take envelope is found by enumeration**, not by a getter. The
//!   fallback that walks the take's envelopes looking for the right one
//!   only exists because REAPER offers nothing better.
//!
//! Everything above it — the detectors, the four lanes, the summing,
//! the thinning — is tested against synthetic audio in
//! `expression-editor-audio`, where it belongs.
//!
//! **Every assertion goes through the DAW RPC.** The test binary is a
//! separate process from REAPER and cannot see the extension's memory,
//! so the lanes are written by a test-only action inside REAPER and
//! read back as envelope points REAPER itself reports.
//!
//! Needs a display — the editor's open action brings up a real Dioxus
//! window. Run with
//!   `cargo run -p fts-extensions-xtask -- --virtual takeenv`

use std::time::Duration;

use daw::rpc::Duration as DawDuration;

use daw::rpc::ItemHandle;
use daw::service::PositionInSeconds;
use daw::test::{ReaperTestContext, reaper_test};

const SR: u32 = 44_100;

/// Gate lane + sibilance lane, as written by
/// `FTS_EXPRESSION_EDITOR_TEST_DYNAMICS`.
const GATE_DB: f64 = -3.0;
const SIB_DB: f64 = -5.0;

/// What the two lanes sum to, as the linear multiplier a take volume
/// envelope stores. The summing is the point of the four-lane design:
/// each lane stays independently editable and the envelope REAPER plays
/// is their total.
fn expected_value() -> f64 {
    10f64.powf((GATE_DB + SIB_DB) / 20.0)
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(400)).await;
}

async fn action(ctx: &ReaperTestContext, id: &str) -> eyre::Result<()> {
    ctx.daw.action_registry().execute_action(id).await?;
    settle().await;
    Ok(())
}

/// A mono 16-bit WAV: a 440 Hz tone with a silent tail, so the analysis
/// has both voiced and unvoiced frames to work with.
fn tone_wav(secs: f64) -> Vec<u8> {
    let n = (SR as f64 * secs) as usize;
    let mut d = Vec::with_capacity(44 + n * 2);
    d.extend_from_slice(b"RIFF");
    d.extend_from_slice(&(36 + n as u32 * 2).to_le_bytes());
    d.extend_from_slice(b"WAVE");
    d.extend_from_slice(b"fmt ");
    d.extend_from_slice(&16u32.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&SR.to_le_bytes());
    d.extend_from_slice(&(SR * 2).to_le_bytes());
    d.extend_from_slice(&2u16.to_le_bytes());
    d.extend_from_slice(&16u16.to_le_bytes());
    d.extend_from_slice(b"data");
    d.extend_from_slice(&(n as u32 * 2).to_le_bytes());
    for i in 0..n {
        let t = i as f64 / SR as f64;
        let env = if t > secs * 0.75 { 0.0 } else { 0.6 };
        let s = (t * 440.0 * std::f64::consts::TAU).sin();
        d.extend_from_slice(&((s * env * i16::MAX as f64) as i16).to_le_bytes());
    }
    d
}

/// A track with one item whose take plays a real file on disk.
///
/// There is no "insert media file" in the facade, so the item is made
/// the way the facade does allow and its take is then pointed at the
/// WAV — which is all REAPER needs to treat it as audio.
async fn seed_audio_item(ctx: &ReaperTestContext) -> eyre::Result<ItemHandle> {
    let dir = std::env::temp_dir().join(format!("fts-take-env-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("tone.wav");
    std::fs::write(&path, tone_wav(1.5))?;

    let track = ctx.project().tracks().add("Take Envelope", None).await?;
    let item = track
        .items()
        .add(
            PositionInSeconds::from_seconds(0.0),
            DawDuration::from_seconds(1.5),
        )
        .await?;
    item.active_take()
        .set_source_file(&path.to_string_lossy())
        .await?;
    item.select().await?;
    settle().await;
    Ok(item)
}

/// The points on the take's volume envelope, as REAPER reports them.
async fn take_volume_points(item: &ItemHandle) -> eyre::Result<Vec<(f64, f64)>> {
    let env = item.active_take().envelopes().await?.volume();
    Ok(env
        .points()
        .await?
        .iter()
        .map(|p| (p.time.as_seconds(), p.value))
        .collect())
}

#[reaper_test(isolated)]
async fn takeenv_reading_does_not_create_one(ctx: &ReaperTestContext) -> eyre::Result<()> {
    // The premise the rest of the file rests on, and a trap this test
    // caught: resolving a take envelope is what *makes* it, so the
    // backend has to decline for reads. Before that split, merely
    // asking a take what points it had left a new envelope and an undo
    // entry behind on every take the editor looked at.
    //
    // It also means a non-empty read later is proof the write path ran,
    // rather than proof REAPER hands out envelopes for free.
    let item = seed_audio_item(ctx).await?;
    assert!(
        take_volume_points(&item).await?.is_empty(),
        "a fresh take has no volume envelope to report points from"
    );
    Ok(())
}

#[reaper_test(isolated)]
async fn takeenv_writing_lanes_creates_it_and_reaper_reports_the_sum(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    let item = seed_audio_item(ctx).await?;

    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;
    action(ctx, "FTS_EXPRESSION_EDITOR_TEST_DYNAMICS").await?;

    let points = take_volume_points(&item).await?;
    assert!(
        !points.is_empty(),
        "the action ran `40693` on the selected item and REAPER made an \
         envelope — if this is empty, either the load found no audio take \
         or the creation dance failed"
    );

    // Both lanes are flat, so thinning should collapse them to the two
    // endpoints. Asserting the value, not just the count: a lane that
    // wrote through un-summed would land at -3 or -5 instead.
    let want = expected_value();
    for (time, value) in &points {
        assert!(
            (value - want).abs() < 0.01,
            "point at {time}s is {value}, wanted the summed {want}"
        );
    }
    // And the envelope spans the take rather than sitting at the origin.
    let last = points.last().expect("at least one point").0;
    assert!(
        last > 0.5,
        "the envelope should cover the take, but ends at {last}s"
    );
    Ok(())
}

#[reaper_test(isolated)]
async fn takeenv_a_second_write_replaces_rather_than_accumulates(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    // The failure an add-only writer has, and the reason `write_dynamics`
    // clears first: with the envelope already in existence the second
    // pass takes the *other* branch of the creation code — the
    // enumeration fallback — so this covers a path the first write does
    // not.
    let item = seed_audio_item(ctx).await?;
    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;

    action(ctx, "FTS_EXPRESSION_EDITOR_TEST_DYNAMICS").await?;
    let first = take_volume_points(&item).await?;
    action(ctx, "FTS_EXPRESSION_EDITOR_TEST_DYNAMICS").await?;
    let second = take_volume_points(&item).await?;

    assert_eq!(
        first.len(),
        second.len(),
        "the same lanes written twice give the same envelope, not twice as many points"
    );
    Ok(())
}

#[reaper_test(isolated)]
async fn takeenv_creation_leaves_the_selection_as_it_was(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    // `40693` acts on the selection, so the backend has to select the
    // item to run it. A user with three items selected must not find
    // two of them deselected afterwards.
    let a = seed_audio_item(ctx).await?;
    let b = seed_audio_item(ctx).await?;
    a.select().await?;
    // `b` was selected by its own seeding; select `a` alongside it.
    settle().await;
    let before = ctx.project().items().selected().await?.len();

    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;
    action(ctx, "FTS_EXPRESSION_EDITOR_TEST_DYNAMICS").await?;

    let after = ctx.project().items().selected().await?.len();
    assert_eq!(
        before, after,
        "the selection survived the action REAPER makes us run"
    );
    let _ = b;
    Ok(())
}

#[reaper_test(isolated)]
async fn takeenv_breaths_and_sibilants_land_as_take_markers(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    // The other half of the write: markers on the item so the spans are
    // visible in the arrange view without opening the editor.
    let item = seed_audio_item(ctx).await?;
    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;
    action(ctx, "FTS_EXPRESSION_EDITOR_TEST_DYNAMICS").await?;

    // A synthetic tone has no real sibilants, so this asserts the write
    // path is wired rather than that detection found anything: markers
    // are allowed to be empty, but must not error, and any that exist
    // must sit inside the take.
    let markers = item.active_take().markers().await?;
    for m in &markers {
        let at = m.source_position_seconds;
        assert!(
            (0.0..=1.6).contains(&at),
            "take marker at {at}s is outside the take"
        );
    }
    Ok(())
}
