//! REAPER integration: the panel opens, and its edits reach a real
//! take.
//!
//! Deliberately narrow. Every piece of *logic* — the conversion, the
//! session round trip, the edit operations — is tested against the
//! standalone backend in `expression-editor-daw`, because that is code
//! in our control and a test needing REAPER to check arithmetic is a
//! slow test for no reason.
//!
//! What only exists inside REAPER, and so is only testable here:
//!
//! - the panel registering and docking through `reaper-dioxus`;
//! - the open action finding REAPER's actual item selection;
//! - a write landing in a real take that REAPER then reports back.
//!
//! **Every assertion goes through the DAW RPC.** The test binary is a
//! separate process from REAPER, so it cannot see the extension's
//! memory — an assertion on an in-process counter would always read
//! zero and prove nothing. "Did the editor load?" is therefore answered
//! by making an edit that shows up in the take.
//!
//! Lives in `fts-extensions` rather than the editor's own crate because
//! the panel only exists once this extension has registered the module.
//!
//! Needs a display: the panel is a real Dioxus window, and opening one
//! headless aborts REAPER inside GDK. Run with
//!   `cargo run -p fts-extensions-xtask -- --virtual`

use std::time::Duration;

use daw::rpc::ItemHandle;
use daw::service::MidiNoteCreate;
use daw::test::{ReaperTestContext, reaper_test};

/// Let REAPER's main thread process what we just asked for.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(400)).await;
}

const SEEDED: [u8; 3] = [60, 64, 67];

/// A track with one MIDI item holding a known set of notes.
async fn seed_item(ctx: &ReaperTestContext) -> eyre::Result<ItemHandle> {
    let track = ctx.project().tracks().add("Expression Editor", None).await?;
    let notes = SEEDED
        .iter()
        .enumerate()
        .map(|(i, &pitch)| MidiNoteCreate {
            channel: 0,
            pitch,
            velocity: 100,
            start_ppq: 960.0 * i as f64,
            length_ppq: 960.0,
        })
        .collect();
    let item = track
        .items()
        .create_midi_item_with_notes(0.0, 4.0, notes)
        .await?
        .expect("REAPER must create the MIDI item");
    settle().await;
    Ok(item)
}

async fn action(ctx: &ReaperTestContext, id: &str) -> eyre::Result<()> {
    ctx.daw.action_registry().execute_action(id).await?;
    settle().await;
    Ok(())
}

/// The take's pitches, sorted — the only thing both processes agree on.
async fn pitches(item: &ItemHandle) -> eyre::Result<Vec<u8>> {
    let mut p: Vec<u8> = item
        .active_take()
        .midi()
        .notes()
        .await?
        .iter()
        .map(|n| n.pitch)
        .collect();
    p.sort_unstable();
    Ok(p)
}

#[reaper_test(isolated)]
async fn the_panel_registers_and_toggles(ctx: &ReaperTestContext) -> eyre::Result<()> {
    // Registration is the thing that silently does not happen when the
    // reaper-dioxus service has not come up.
    action(ctx, "FTS_EXPRESSION_EDITOR_TOGGLE").await?;
    ctx.assert_panel_visible(expression_editor_reaper::PANEL_ID)
        .await?;

    action(ctx, "FTS_EXPRESSION_EDITOR_TOGGLE").await?;
    ctx.assert_panel_hidden(expression_editor_reaper::PANEL_ID)
        .await?;
    Ok(())
}

#[reaper_test(isolated)]
async fn opening_on_a_selection_shows_the_panel(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let item = seed_item(ctx).await?;
    item.select().await?;
    settle().await;

    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;
    ctx.assert_panel_visible(expression_editor_reaper::PANEL_ID)
        .await?;
    // Opening must not disturb the material.
    assert_eq!(pitches(&item).await?, SEEDED.to_vec());
    Ok(())
}

#[reaper_test(isolated)]
async fn a_load_edit_and_write_round_trip_reaches_the_take(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    let item = seed_item(ctx).await?;
    item.select().await?;
    settle().await;

    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;
    // The edit runs inside REAPER, through the editor's own Edit
    // pipeline — the only way this proves the pipeline works.
    action(ctx, "FTS_EXPRESSION_EDITOR_TEST_TRANSPOSE").await?;
    action(ctx, "FTS_EXPRESSION_EDITOR_WRITE").await?;

    // Ask REAPER, not the editor. If the load silently failed the
    // session would be empty, the transpose a no-op, and the take
    // unchanged — so this single assertion covers the whole path.
    assert_eq!(
        pitches(&item).await?,
        vec![72, 76, 79],
        "REAPER must report the transposed take"
    );
    Ok(())
}

#[reaper_test(isolated)]
async fn a_write_with_no_edit_preserves_the_take(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    let item = seed_item(ctx).await?;
    item.select().await?;
    settle().await;

    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;
    action(ctx, "FTS_EXPRESSION_EDITOR_WRITE").await?;

    // Read → convert → write with no edit in between must be lossless.
    // Any asymmetry in the conversion shows up here as drift.
    assert_eq!(pitches(&item).await?, SEEDED.to_vec());
    Ok(())
}

#[reaper_test(isolated)]
async fn repeated_writes_replace_rather_than_append(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    let item = seed_item(ctx).await?;
    item.select().await?;
    settle().await;
    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;

    // The failure mode a naive add-notes implementation has.
    action(ctx, "FTS_EXPRESSION_EDITOR_WRITE").await?;
    action(ctx, "FTS_EXPRESSION_EDITOR_WRITE").await?;

    assert_eq!(
        pitches(&item).await?,
        SEEDED.to_vec(),
        "still three notes, not six"
    );
    Ok(())
}

#[reaper_test(isolated)]
async fn reload_makes_the_take_authoritative(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let item = seed_item(ctx).await?;
    item.select().await?;
    settle().await;
    action(ctx, "FTS_EXPRESSION_EDITOR_OPEN").await?;

    // Edit, throw it away by reloading, then write: the take should be
    // exactly what it started as.
    action(ctx, "FTS_EXPRESSION_EDITOR_TEST_TRANSPOSE").await?;
    action(ctx, "FTS_EXPRESSION_EDITOR_RELOAD").await?;
    action(ctx, "FTS_EXPRESSION_EDITOR_WRITE").await?;

    assert_eq!(
        pitches(&item).await?,
        SEEDED.to_vec(),
        "reload discards the edit in favour of the take"
    );
    Ok(())
}
