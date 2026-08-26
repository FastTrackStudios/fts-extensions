//! The midi-tools panels, driven inside a real REAPER.
//!
//! This is the test that would have caught the two bugs the headless
//! suite missed, and it exists because of them.
//!
//! `midi-tools-daw/tests/reaper_velocity_gui.rs` drives the panel on a
//! headless Blitz DOM via `dioxus-test`. That proved the logic and the
//! layout, and it passed the entire time the panel was completely inert
//! inside REAPER — because `dioxus-test` vendors *newer* dioxus pins than
//! the shipping renderer, and those synthesize `pointer*` events the real
//! dock never sends. A harness ahead of the thing it stands in for.
//!
//! So this one uses no harness at all. It talks to a live REAPER with the
//! real extension loaded, and reaches the panel through `DockHost`:
//!
//! - `register_dock` is idempotent by id, so it hands back a handle to
//!   the *already-registered* panel rather than making a new one.
//! - `inject_ui_event` feeds events through the same `forward_mouse_event`
//!   path the SWELL wndproc uses for a real mouse, into the same Blitz
//!   view REAPER renders.
//!
//! Nothing is simulated except the pointer's position.
//!
//! ## Running them
//!
//! One environmental caveat: this rig shares its REAPER profile
//! (`~/fts-dev`) with the interactive dev instance, so if one is open
//! the spawned REAPER contends with it and these fail on socket
//! discovery instead of on what they assert. Close any dev REAPER
//! first. They also need a display — run them under `--virtual`.
//!
//! They were previously blocked by a real bug — every `DockHost` call
//! failed to decode — which turned out to be that fts-extensions never
//! mounted the `dock_host` layer into its router at all. `daw-bridge`
//! mounts it; this host did not, so the calls reached a router with no
//! such service and the reply failed to decode. Reported as a schema
//! mismatch, which is why it read like a vox negotiation bug.
//!
//! ```sh
//! just reaper integration-test panel_
//! ```

use daw::service::dock_host::{DockHandle, DockKind, UiEventDto};
use daw::service::{Duration, MidiNoteCreate, PositionInSeconds};
use daw::test::{ReaperTestContext, reaper_test};
use std::time::Duration as StdDuration;

const PPQ: f64 = 960.0;
const VELOCITY_PANEL: &str = "FTS_MIDI_VELOCITY";

/// Wait for FTS Extensions to finish registering actions.
async fn wait_for_ready(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();
    for _ in 0..30 {
        if let Ok(Some(_)) = actions.lookup_command_id(VELOCITY_PANEL).await {
            return Ok(());
        }
        tokio::time::sleep(StdDuration::from_millis(500)).await;
    }
    eyre::bail!("midi-tools actions not registered — is the extension loaded?")
}

/// A handle to the already-registered velocity panel.
async fn panel(ctx: &ReaperTestContext) -> eyre::Result<DockHandle> {
    Ok(ctx
        .daw
        .dock_host()
        .register_dock(VELOCITY_PANEL, "MIDI Velocity", DockKind::Floating)
        .await?)
}

/// Poll until the panel reaches `want` visibility, or give up.
///
/// Polled rather than asserted immediately: opening a panel builds a
/// window and an embedded Blitz view, which is not synchronous with the
/// action returning.
async fn wait_visible(ctx: &ReaperTestContext, handle: DockHandle, want: bool) -> eyre::Result<()> {
    let dock = ctx.daw.dock_host();
    for _ in 0..40 {
        if dock.is_visible(handle).await? == want {
            return Ok(());
        }
        tokio::time::sleep(StdDuration::from_millis(250)).await;
    }
    eyre::bail!(
        "panel never became {}",
        if want { "visible" } else { "hidden" }
    )
}

/// A MIDI item with `velocities` as eighth notes, the *only* selected
/// item, so the panel binds to it.
///
/// The deselect is load-bearing. `ItemHandle::select` is additive — it
/// selects, it does not *become* the selection — and these tests share
/// one project, so without it an earlier test's item is still selected
/// and comes first. `DawVelocitySink::open` binds to the **first**
/// selected item, so the panel would edit a take nobody asserts on.
///
/// That was half of what kept `panel_click_shapes_the_take` skipped; the
/// other half was the panel never rebinding at all, fixed in
/// `EmbeddedView::remount`.
async fn selected_take(
    ctx: &ReaperTestContext,
    name: &str,
    velocities: &[u8],
) -> eyre::Result<daw::rpc::TakeHandle> {
    let project = ctx.project().clone();
    project.items().deselect_all().await?;
    let track = project.tracks().add(name, None).await?;
    let item = track
        .items()
        .add(
            PositionInSeconds::from_seconds(0.0),
            Duration::from_seconds(velocities.len() as f64 * 0.5),
        )
        .await?;
    item.select().await?;
    let take = item.active_take();
    take.midi()
        .add_notes(
            velocities
                .iter()
                .enumerate()
                .map(|(i, v)| MidiNoteCreate {
                    pitch: 60,
                    velocity: *v,
                    channel: 0,
                    start_ppq: i as f64 * 0.5,
                    length_ppq: PPQ / 4.0,
                })
                .collect(),
        )
        .await?;
    Ok(take)
}

async fn velocities(take: &daw::rpc::TakeHandle) -> eyre::Result<Vec<u8>> {
    Ok(take
        .midi()
        .notes()
        .await?
        .into_iter()
        .map(|n| n.velocity)
        .collect())
}

/// Click at panel-local `(x, y)` with the real event path.
async fn click(ctx: &ReaperTestContext, handle: DockHandle, x: f32, y: f32) -> eyre::Result<()> {
    let dock = ctx.daw.dock_host();
    // Move first: a real pointer is somewhere before it presses, and
    // hover state can matter to hit-testing.
    dock.inject_event(handle, UiEventDto::PointerMove { x, y })
        .await?;
    dock.inject_event(handle, UiEventDto::PointerDown { x, y, button: 0 })
        .await?;
    tokio::time::sleep(StdDuration::from_millis(60)).await;
    dock.inject_event(handle, UiEventDto::PointerUp { x, y, button: 0 })
        .await?;
    tokio::time::sleep(StdDuration::from_millis(250)).await;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────

/// The action opens the panel, and firing it again closes it.
///
/// This is what "clicking the toolbar button" reduces to — the button
/// runs the action. If this passes and the button still does nothing, the
/// fault is in the toolbar's *section*, not the panel.
#[reaper_test]
async fn panel_toggles_via_its_action(ctx: &ReaperTestContext) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let handle = panel(ctx).await?;
    let actions = ctx.daw.action_registry();

    // Start from a known state rather than assuming it's closed.
    if ctx.daw.dock_host().is_visible(handle).await? {
        actions.execute_named_action(VELOCITY_PANEL).await?;
        wait_visible(ctx, handle, false).await?;
    }

    actions.execute_named_action(VELOCITY_PANEL).await?;
    wait_visible(ctx, handle, true).await?;
    ctx.log("action opened the panel");

    actions.execute_named_action(VELOCITY_PANEL).await?;
    wait_visible(ctx, handle, false).await?;
    ctx.log("action closed the panel");
    Ok(())
}

/// The panel renders something, in the real renderer.
///
/// A panel that opens but draws nothing is a failure mode the visibility
/// check alone cannot see — and Blitz rendering is exactly the part no
/// headless test covers.
#[reaper_test]
async fn panel_actually_renders_in_reaper(ctx: &ReaperTestContext) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let _take = selected_take(ctx, "Panel Render", &[64; 8]).await?;
    let handle = panel(ctx).await?;
    let dock = ctx.daw.dock_host();

    if !dock.is_visible(handle).await? {
        ctx.daw
            .action_registry()
            .execute_named_action(VELOCITY_PANEL)
            .await?;
        wait_visible(ctx, handle, true).await?;
    }
    tokio::time::sleep(StdDuration::from_millis(600)).await;

    let pixels = dock
        .capture_pixels(handle)
        .await?
        .ok_or_else(|| eyre::eyre!("panel is visible but captured no pixels"))?;
    ctx.log(&format!(
        "captured {}x{} ({} bytes)",
        pixels.width,
        pixels.height,
        pixels.bgra.len()
    ));

    eyre::ensure!(pixels.width > 0 && pixels.height > 0, "empty panel surface");

    // Not all one colour — i.e. it drew *something*, not a blank fill.
    let distinct = pixels
        .bgra
        .chunks(4)
        .step_by(37)
        .map(|px| (px[0], px[1], px[2]))
        .collect::<std::collections::HashSet<_>>()
        .len();
    ctx.log(&format!("{distinct} distinct sampled colours"));
    eyre::ensure!(
        distinct > 3,
        "panel surface looks blank ({distinct} colours)"
    );
    Ok(())
}

/// The headline: click a curve preset in the REAL panel, and the REAL
/// take changes.
///
/// This is the exact interaction that was once silently broken — the
/// widgets listened for `pointer*` while the dock dispatched `mouse*`.
/// Everything headless passed; this is what tells the truth.
///
/// It depends on `selected_take` leaving exactly one item selected —
/// see the note there.
///
/// Coordinates are panel-local and hand-derived from the layout: the
/// "Rise" chip is the first entry of the CURVE row, which sits under the
/// fixed-height (132px) preview strip. Brittle by nature — if the panel
/// is reflowed this needs updating, and the failure will say so via the
/// captured colour counts above.
#[reaper_test]
async fn panel_click_shapes_the_take(ctx: &ReaperTestContext) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let take = selected_take(ctx, "Panel Click", &[64; 16]).await?;
    let before = velocities(&take).await?;
    ctx.log(&format!("before → {before:?}"));

    let handle = panel(ctx).await?;
    let dock = ctx.daw.dock_host();

    // Re-open so the panel binds to the item this test just selected.
    if dock.is_visible(handle).await? {
        ctx.daw
            .action_registry()
            .execute_named_action(VELOCITY_PANEL)
            .await?;
        wait_visible(ctx, handle, false).await?;
    }
    ctx.daw
        .action_registry()
        .execute_named_action(VELOCITY_PANEL)
        .await?;
    wait_visible(ctx, handle, true).await?;
    tokio::time::sleep(StdDuration::from_millis(800)).await;

    // "Rise": first chip of the CURVE row.
    click(ctx, handle, 41.0, 219.0).await?;

    let after = velocities(&take).await?;
    ctx.log(&format!("after → {after:?}"));

    eyre::ensure!(
        after != before,
        "clicking the curve preset changed nothing — the panel is not \
         receiving events (this is the pointer-vs-mouse failure mode)"
    );
    eyre::ensure!(
        after.windows(2).all(|w| w[0] <= w[1]),
        "a rise must arrive as a rise: {after:?}"
    );
    Ok(())
}
