//! Integration tests for FTS Extensions.
//!
//! These tests run against a live REAPER instance via the daw test harness.
//! The FTS Extensions plugin must be installed in REAPER's UserPlugins.

use daw::rpc::Project;
use session::ruler_lanes::CoreLane;

/// Canonical lane indices from the session domain (SONG=0, SECTIONS=1, MARKS=2).
const SECTIONS_LANE: u32 = CoreLane::Sections.lane_index();
const MARKS_LANE: u32 = CoreLane::Marks.lane_index();
use daw::test::{ReaperTestContext, reaper_test};
use std::time::Duration;

/// Wait for FTS Extensions to finish registering actions.
/// The extension registers asynchronously after REAPER starts (~1-2s).
async fn wait_for_ready(ctx: &ReaperTestContext) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    for i in 0..30 {
        // Probe a well-known action as a readiness check
        if let Ok(Some(_id)) = actions.lookup_command_id("FTS_LAUNCHER_TOGGLE").await {
            ctx.log(&format!("Actions ready after {}ms", i * 500));
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    eyre::bail!("FTS actions not registered after 15s — is the extension loaded?");
}

/// Wait specifically for an input action to appear in the host registry.
async fn wait_for_input_action(ctx: &ReaperTestContext, command_name: &str) -> eyre::Result<u32> {
    let actions = ctx.daw.action_registry();

    for i in 0..30 {
        if let Ok(Some(id)) = actions.lookup_command_id(command_name).await {
            ctx.log(&format!(
                "{command_name} registered after {}ms (cmd_id={id})",
                i * 500
            ));
            return Ok(id);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    eyre::bail!("{command_name} was not registered after 15s");
}

/// Wait for an input action to appear in REAPER's main action list.
async fn wait_for_input_action_list(
    ctx: &ReaperTestContext,
    command_name: &str,
) -> eyre::Result<()> {
    let actions = ctx.daw.action_registry();

    for i in 0..30 {
        if actions
            .is_in_action_list(command_name)
            .await
            .unwrap_or(false)
        {
            ctx.log(&format!(
                "{command_name} appeared in REAPER's action list after {}ms",
                i * 500
            ));
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    eyre::bail!("{command_name} did not appear in REAPER's action list after 15s");
}

async fn execute_registered_action(
    ctx: &ReaperTestContext,
    command_name: &str,
) -> eyre::Result<()> {
    wait_for_input_action(ctx, command_name).await?;
    let executed = ctx
        .daw
        .action_registry()
        .execute_named_action(command_name)
        .await?;
    assert!(executed, "{command_name} should execute");
    Ok(())
}

async fn wait_for_marker_named(
    ctx: &ReaperTestContext,
    name: &str,
) -> eyre::Result<daw::service::Marker> {
    let project = ctx.daw.current_project().await?;
    for _ in 0..40 {
        if let Some(marker) = project
            .markers()
            .all()
            .await?
            .into_iter()
            .find(|marker| marker.name == name)
        {
            return Ok(marker);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    eyre::bail!("marker {name} was not inserted");
}

async fn wait_for_region_named(
    ctx: &ReaperTestContext,
    name: &str,
) -> eyre::Result<daw::service::Region> {
    let project = ctx.daw.current_project().await?;
    for _ in 0..40 {
        if let Some(region) = project
            .regions()
            .all()
            .await?
            .into_iter()
            .find(|region| region.name == name)
        {
            return Ok(region);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    eyre::bail!("region {name} was not inserted");
}

async fn wait_for_region_names(
    project: &Project,
    expected: &[&str],
) -> eyre::Result<Vec<daw::service::Region>> {
    for _ in 0..40 {
        let mut regions: Vec<daw::service::Region> = project.regions().all().await?;
        regions.sort_by(|a, b| a.start_seconds().total_cmp(&b.start_seconds()));
        let names: Vec<_> = regions.iter().map(|region| region.name.as_str()).collect();
        if names == expected {
            return Ok(regions);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let mut regions: Vec<daw::service::Region> = project.regions().all().await?;
    regions.sort_by(|a, b| a.start_seconds().total_cmp(&b.start_seconds()));
    let names: Vec<_> = regions.iter().map(|region| region.name.clone()).collect();
    eyre::bail!("region names did not settle to {expected:?}; got {names:?}");
}

/// Verify the extension loaded and at least one action is registered.
#[reaper_test]
async fn extension_loaded(ctx: &ReaperTestContext) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    ctx.log("FTS Extensions loaded and actions registered");
    Ok(())
}

/// Verify the FTS-Input passthrough toggle is registered as an action in the host.
#[reaper_test]
async fn input_toggle_passthrough_is_registered(ctx: &ReaperTestContext) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let actions = ctx.daw.action_registry();

    let cmd_id = wait_for_input_action(ctx, "FTS_INPUT_TOGGLE_PASSTHROUGH").await?;

    let registered = actions
        .is_registered("FTS_INPUT_TOGGLE_PASSTHROUGH")
        .await?;
    assert!(
        registered,
        "FTS_INPUT_TOGGLE_PASSTHROUGH should be registered"
    );

    wait_for_input_action_list(ctx, "FTS_INPUT_TOGGLE_PASSTHROUGH").await?;

    ctx.log(&format!(
        "FTS_INPUT_TOGGLE_PASSTHROUGH registered with cmd_id={cmd_id}"
    ));
    Ok(())
}

/// Verify the legacy test toggle is also present in REAPER's action list.
///
/// This is a control case for comparing the legacy action registration path
/// with the module-based input registration path.
#[reaper_test]
async fn legacy_test_toggle_is_in_action_list(ctx: &ReaperTestContext) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let actions = ctx.daw.action_registry();

    let cmd_id = wait_for_input_action(ctx, "FTS_TEST_TOGGLE").await?;
    let in_list = actions.is_in_action_list("FTS_TEST_TOGGLE").await?;
    assert!(
        in_list,
        "FTS_TEST_TOGGLE should appear in REAPER's action list"
    );

    ctx.log(&format!("FTS_TEST_TOGGLE registered with cmd_id={cmd_id}"));
    Ok(())
}

/// Verify another input-module toggle action is present in REAPER's action list.
///
/// This distinguishes a single broken action from a broader module-registration issue.
#[reaper_test]
async fn input_profile_selector_is_in_action_list(ctx: &ReaperTestContext) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let actions = ctx.daw.action_registry();

    let cmd_id = wait_for_input_action(ctx, "FTS_INPUT_PROFILE_SELECTOR").await?;
    let in_list = actions
        .is_in_action_list("FTS_INPUT_PROFILE_SELECTOR")
        .await?;
    assert!(
        in_list,
        "FTS_INPUT_PROFILE_SELECTOR should appear in REAPER's action list"
    );

    ctx.log(&format!(
        "FTS_INPUT_PROFILE_SELECTOR registered with cmd_id={cmd_id}"
    ));
    Ok(())
}

/// Verify the input status panel toggle is also present in REAPER's action list.
#[reaper_test]
async fn input_status_panel_is_in_action_list(ctx: &ReaperTestContext) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let actions = ctx.daw.action_registry();

    let cmd_id = wait_for_input_action(ctx, "FTS_INPUT_TOGGLE_STATUS_PANEL").await?;
    let in_list = actions
        .is_in_action_list("FTS_INPUT_TOGGLE_STATUS_PANEL")
        .await?;
    assert!(
        in_list,
        "FTS_INPUT_TOGGLE_STATUS_PANEL should appear in REAPER's action list"
    );

    ctx.log(&format!(
        "FTS_INPUT_TOGGLE_STATUS_PANEL registered with cmd_id={cmd_id}"
    ));
    Ok(())
}

/// Verify all expected static actions are registered across all modules.
///
/// This list is the single source of truth for what fts-extensions should
/// register. If a module adds or removes actions, update this test.
#[reaper_test]
async fn all_actions_registered(ctx: &ReaperTestContext) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let actions = ctx.daw.action_registry();

    // ── Legacy actions (fts-extensions/src/actions.rs) ──
    let legacy = [
        "FTS_TEMPO_MOVE_MEASURE_GRID_TO_MOUSE",
        "FTS_TEMPO_MOVE_MEASURE_GRID_TO_MOUSE_CONSTRAINED",
        "FTS_TEMPO_MOVE_MEASURE_GRID_TO_MOUSE_FULLY_CONSTRAINED",
        "FTS_TEMPO_MOVE_GRID_TO_MOUSE",
        "FTS_TEMPO_MOVE_MARKER_TO_MOUSE",
        "FTS_TEMPO_SNAP_GRID_TO_TRANSIENT",
        "FTS_TEMPO_SNAP_GRID_TO_TRANSIENT_CONSTRAINED",
        "FTS_TEMPO_SNAP_GRID_TO_TRANSIENT_FULLY_CONSTRAINED",
        "FTS_SPLIT_ITEMS_CROSSFADE_LEFT",
        "FTS_TEST_TOGGLE",
        "FTS_INFO",
    ];

    // ── Launcher module ──
    let launcher = ["FTS_LAUNCHER_TOGGLE"];

    // ── Session module (prefix: fts.session) ──
    let session = [
        "FTS_SESSION_TOGGLE_PLAYBACK",
        "FTS_SESSION_TOGGLE_SONG_LOOP",
        "FTS_SESSION_SMART_NEXT",
        "FTS_SESSION_SMART_PREVIOUS",
        "FTS_SESSION_NEXT_SONG",
        "FTS_SESSION_PREVIOUS_SONG",
        "FTS_SESSION_NEXT_SECTION",
        "FTS_SESSION_PREVIOUS_SECTION",
        "FTS_SESSION_BUILD_SETLIST",
        "FTS_SESSION_LOAD_DEMO_SETLIST",
        "FTS_SESSION_INSERT_INTRO_REGION",
        "FTS_SESSION_INSERT_VERSE_REGION",
        "FTS_SESSION_INSERT_PRE_CHORUS_REGION",
        "FTS_SESSION_INSERT_CHORUS_REGION",
        "FTS_SESSION_INSERT_BRIDGE_REGION",
        "FTS_SESSION_INSERT_OUTRO_REGION",
        "FTS_SESSION_INSERT_INSTRUMENTAL_REGION",
        "FTS_SESSION_INSERT_SOLO_REGION",
        "FTS_SESSION_INSERT_HITS_REGION",
        "FTS_SESSION_INSERT_INTERLUDE_REGION",
        "FTS_SESSION_INSERT_BREAKDOWN_REGION",
        "FTS_SESSION_INSERT_VAMP_REGION",
        "FTS_SESSION_INSERT_COUNT_IN_REGION",
        "FTS_SESSION_INSERT_END_REGION",
        "FTS_SESSION_INSERT_COUNT_IN_MARKER",
        "FTS_SESSION_INSERT_START_MARKER",
        "FTS_SESSION_INSERT_END_MARKER",
        "FTS_SESSION_INSERT_SONGSTART_MARKER",
        "FTS_SESSION_INSERT_SONGEND_MARKER",
        "FTS_SESSION_ORGANIZE_EVERYTHING",
        "FTS_SESSION_ORGANIZE_SELECTED_TRACKS",
        "FTS_SESSION_CREATE_NEW_DRUM_KIT",
        "FTS_SESSION_CREATE_NEW_ELECTRONIC_DRUMS",
        "FTS_SESSION_CREATE_NEW_BASS_GUITAR",
        "FTS_SESSION_CREATE_NEW_ELECTRIC_GUITAR",
        "FTS_SESSION_CREATE_NEW_ACOUSTIC_GUITAR",
        "FTS_SESSION_CREATE_NEW_KEYS",
        "FTS_SESSION_CREATE_NEW_SYNTH",
        "FTS_SESSION_CREATE_NEW_ORCHESTRAL_STRINGS",
        "FTS_SESSION_CREATE_NEW_ORCHESTRAL_BRASS",
        "FTS_SESSION_CREATE_NEW_ORCHESTRAL_WOODWINDS",
        "FTS_SESSION_CREATE_NEW_ORCHESTRAL_PERCUSSION",
        "FTS_SESSION_CREATE_NEW_SFX",
        "FTS_SESSION_CREATE_NEW_LEAD_VOCALS",
        "FTS_SESSION_CREATE_NEW_BACKGROUND_VOCALS",
        "FTS_SESSION_AUTO_COLOR_COLOR_ALL",
        "FTS_SESSION_AUTO_COLOR_COLOR_SELECTED",
        "FTS_SESSION_AUTO_COLOR_TOGGLE",
        "FTS_SESSION_AUTO_COLOR_CLEAR_ALL",
        "FTS_SESSION_AUTO_COLOR_CLEAR_SELECTED",
        "FTS_SESSION_TRACK_MANAGER_ADD_CHANNEL",
        "FTS_SESSION_TRACK_MANAGER_ADD_LAYER",
        "FTS_SESSION_TRACK_MANAGER_ADD_MULTI_MIC",
        "FTS_SESSION_TRACK_MANAGER_ADD_PERFORMER",
        "FTS_SESSION_TRACK_MANAGER_REORGANIZE_SELECTED_BY_PERFORMER",
        "FTS_SESSION_TRACK_MANAGER_REORGANIZE_SELECTED_BY_ARRANGEMENT",
    ];

    // ── Dynamic-template module (registered directly so the visibility
    // manager / template / auto-color actions are bindable in REAPER's
    // action list, not just reachable via the FTS_SESSION_* wrappers) ──
    let template = [
        "FTS_VISIBILITY_MANAGER_TOGGLE_DRUMS",
        "FTS_VISIBILITY_MANAGER_TOGGLE_PERCUSSION",
        "FTS_VISIBILITY_MANAGER_TOGGLE_BASS",
        "FTS_VISIBILITY_MANAGER_TOGGLE_GUITARS",
        "FTS_VISIBILITY_MANAGER_TOGGLE_KEYS",
        "FTS_VISIBILITY_MANAGER_TOGGLE_SYNTHS",
        "FTS_VISIBILITY_MANAGER_TOGGLE_HORNS",
        "FTS_VISIBILITY_MANAGER_TOGGLE_HARMONICA",
        "FTS_VISIBILITY_MANAGER_TOGGLE_STRINGS",
        "FTS_VISIBILITY_MANAGER_TOGGLE_VOCALS",
        "FTS_VISIBILITY_MANAGER_TOGGLE_CHOIR",
        "FTS_VISIBILITY_MANAGER_TOGGLE_ORCHESTRA",
        "FTS_VISIBILITY_MANAGER_TOGGLE_SFX",
        "FTS_VISIBILITY_MANAGER_TOGGLE_GUIDE",
        "FTS_VISIBILITY_MANAGER_TOGGLE_REFERENCE",
        "FTS_VISIBILITY_MANAGER_TOGGLE_STEM_SPLIT",
        "FTS_VISIBILITY_MANAGER_SHOW_ALL",
        "FTS_VISIBILITY_MANAGER_HIDE_ALL",
        "FTS_VISIBILITY_MANAGER_PROFILE_DRUM_EDITING",
        "FTS_VISIBILITY_MANAGER_PROFILE_MIDI_EDITING",
        "FTS_VISIBILITY_MANAGER_MODE_ORGANIZE",
        "FTS_VISIBILITY_MANAGER_MODE_WRITE",
        "FTS_VISIBILITY_MANAGER_MODE_PRODUCE",
        "FTS_VISIBILITY_MANAGER_MODE_RECORD",
        "FTS_VISIBILITY_MANAGER_MODE_EDIT",
        "FTS_VISIBILITY_MANAGER_MODE_MIX",
        "FTS_VISIBILITY_MANAGER_MODE_MASTER",
        "FTS_VISIBILITY_MANAGER_MODE_LIVE",
        "FTS_VISIBILITY_MANAGER_MODE_VIDEO",
        "FTS_VISIBILITY_MANAGER_MODE_SCORING",
        "FTS_VISIBILITY_MANAGER_REBUILD_CACHE",
    ];

    // ── Input module (static actions only; dynamic presets/workflows depend on config) ──
    let input = [
        "FTS_INPUT_TOGGLE",
        "FTS_INPUT_TOGGLE_PASSTHROUGH",
        "FTS_INPUT_TOGGLE_DEBUG_LOGGING",
        "FTS_INPUT_DEBUG_MOUSE_CONTEXT",
        "FTS_INPUT_RESET_ALL_OVERRIDES",
        "FTS_INPUT_MOUSE_RESET_TO_PROFILE",
        "FTS_INPUT_PROFILE_SELECTOR",
        "FTS_INPUT_WORKFLOW_SELECTOR",
        "FTS_INPUT_TOGGLE_STATUS_PANEL",
        "FTS_INPUT_DEV_TEST_MOUSE_MODIFIER_IDS",
        "FTS_INPUT_DEV_RESET_ITEM_CLICK_MODIFIERS",
        // FTS_INPUT_WORKFLOW_DEACTIVATE is dynamic — only registered when workflows are loaded
    ];

    // Collect all expected actions
    let all_expected: Vec<&str> = [
        &legacy[..],
        &launcher[..],
        &session[..],
        &template[..],
        &input[..],
    ]
    .concat();

    let mut missing = Vec::new();
    let mut found = 0usize;

    for name in &all_expected {
        match actions.lookup_command_id(name).await {
            Ok(Some(id)) => {
                ctx.log(&format!("  {name}: OK (cmd_id={id})"));
                found += 1;
            }
            _ => {
                ctx.log(&format!("  {name}: MISSING"));
                missing.push(*name);
            }
        }
    }

    ctx.log(&format!(
        "\n  {found}/{} actions registered",
        all_expected.len()
    ));

    assert!(
        missing.is_empty(),
        "Missing {} actions: {:?}",
        missing.len(),
        missing
    );
    Ok(())
}

#[reaper_test]
async fn session_keyflow_marker_action_inserts_marker(ctx: &ReaperTestContext) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let project = ctx.daw.current_project().await?;
    project.transport().set_position(2.0).await?;

    execute_registered_action(ctx, "FTS_SESSION_INSERT_START_MARKER").await?;

    let marker = wait_for_marker_named(ctx, "=START").await?;
    assert_eq!(marker.lane, Some(MARKS_LANE), "=START should be on MARKS lane");
    assert_eq!(marker.position_seconds(), 2.0);
    assert!(marker.color.is_some(), "=START should get a default color");
    Ok(())
}

#[reaper_test]
async fn session_keyflow_region_action_inserts_region(ctx: &ReaperTestContext) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let project = ctx.daw.current_project().await?;
    project.transport().set_position(4.0).await?;
    project.transport().set_time_selection(4.0, 12.0).await?;

    execute_registered_action(ctx, "FTS_SESSION_INSERT_CHORUS_REGION").await?;

    let region = wait_for_region_named(ctx, "CH").await?;
    assert_eq!(region.lane, Some(SECTIONS_LANE), "CH should be on SECTIONS lane");
    assert_eq!(region.start_seconds(), 4.0);
    assert_eq!(region.end_seconds(), 12.0);
    assert!(region.color.is_some(), "CH should get a default color");
    project.transport().clear_time_selection().await?;
    Ok(())
}

#[reaper_test(isolated)]
async fn session_keyflow_region_action_uses_default_length_and_advances_cursor(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let project = ctx.project().clone();

    project.transport().clear_time_selection().await?;
    project.transport().set_position(0.0).await?;
    let (measure, beat, fraction) = project.tempo_map().time_to_musical(0.0).await?;
    let expected_end = project
        .tempo_map()
        .musical_to_time(measure + 8, beat, fraction)
        .await?;

    execute_registered_action(ctx, "FTS_SESSION_INSERT_CHORUS_REGION").await?;

    let region = wait_for_region_named(ctx, "CH").await?;
    assert_eq!(region.lane, Some(SECTIONS_LANE), "CH should be on SECTIONS lane");
    assert_eq!(region.start_seconds(), 0.0);
    assert!(
        region.end_seconds().is_finite(),
        "CH should have a finite default end"
    );
    assert!(
        (region.end_seconds() - expected_end).abs() <= 0.001,
        "CH should default to 8 measures"
    );

    let position = project.transport().get_position().await?;
    assert!(
        (position - region.end_seconds()).abs() <= 0.001,
        "edit cursor should advance to the inserted section end"
    );
    Ok(())
}

#[reaper_test(isolated)]
async fn session_keyflow_region_action_ignores_oversized_time_selection(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let project = ctx.project().clone();

    project
        .transport()
        .set_time_selection(0.0, 64_000.0)
        .await?;
    project.transport().set_position(0.0).await?;
    let (measure, beat, fraction) = project.tempo_map().time_to_musical(0.0).await?;
    let expected_end = project
        .tempo_map()
        .musical_to_time(measure + 4, beat, fraction)
        .await?;

    execute_registered_action(ctx, "FTS_SESSION_INSERT_OUTRO_REGION").await?;

    let region = wait_for_region_named(ctx, "OUT").await?;
    assert_eq!(region.lane, Some(SECTIONS_LANE), "OUT should be on SECTIONS lane");
    assert_eq!(region.start_seconds(), 0.0);
    assert!(
        (region.end_seconds() - expected_end).abs() <= 0.001,
        "oversized time selection should be ignored in favor of the 4-measure OUT default"
    );
    project.transport().clear_time_selection().await?;
    Ok(())
}

#[reaper_test(isolated)]
async fn session_keyflow_count_in_region_defaults_to_two_measures_and_pink(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let project = ctx.project().clone();

    project.transport().clear_time_selection().await?;
    project.transport().set_position(0.0).await?;
    let (measure, beat, fraction) = project.tempo_map().time_to_musical(0.0).await?;
    let expected_end = project
        .tempo_map()
        .musical_to_time(measure + 2, beat, fraction)
        .await?;

    execute_registered_action(ctx, "FTS_SESSION_INSERT_COUNT_IN_REGION").await?;

    let region = wait_for_region_named(ctx, "COUNT").await?;
    assert_eq!(region.lane, Some(SECTIONS_LANE), "COUNT should be on SECTIONS lane");
    assert_eq!(region.start_seconds(), 0.0);
    assert!(
        (region.end_seconds() - expected_end).abs() <= 0.001,
        "COUNT should default to 2 measures"
    );
    assert_eq!(region.color, Some(0x01EC4899), "COUNT should be pink");
    Ok(())
}

#[reaper_test(isolated)]
async fn session_keyflow_section_actions_retroactively_update_chorus_names(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let project = ctx.project().clone();

    project.transport().set_position(0.0).await?;
    project.transport().set_time_selection(0.0, 8.0).await?;
    execute_registered_action(ctx, "FTS_SESSION_INSERT_CHORUS_REGION").await?;
    let regions = wait_for_region_names(&project, &["CH"]).await?;
    assert_eq!(regions[0].lane, Some(SECTIONS_LANE), "CH should be on SECTIONS lane");

    project.transport().set_position(8.0).await?;
    project.transport().set_time_selection(8.0, 16.0).await?;
    execute_registered_action(ctx, "FTS_SESSION_INSERT_CHORUS_REGION").await?;
    let regions = wait_for_region_names(&project, &["CH A", "CH B"]).await?;
    assert_eq!(regions[0].start_seconds(), 0.0);
    assert_eq!(regions[1].start_seconds(), 8.0);

    project.transport().set_position(32.0).await?;
    project.transport().set_time_selection(32.0, 40.0).await?;
    execute_registered_action(ctx, "FTS_SESSION_INSERT_CHORUS_REGION").await?;
    let regions = wait_for_region_names(&project, &["CH 1A", "CH 1B", "CH 2"]).await?;
    assert_eq!(regions[0].start_seconds(), 0.0);
    assert_eq!(regions[1].start_seconds(), 8.0);
    assert_eq!(regions[2].start_seconds(), 32.0);
    assert!(regions.iter().all(|region| region.lane == Some(SECTIONS_LANE)));
    assert!(regions.iter().all(|region| region.color.is_some()));

    project.transport().clear_time_selection().await?;
    Ok(())
}

#[reaper_test(isolated)]
async fn session_keyflow_section_insert_truncates_overlapping_section(
    ctx: &ReaperTestContext,
) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let project = ctx.project().clone();

    project.transport().set_position(0.0).await?;
    project.transport().set_time_selection(0.0, 8.0).await?;
    execute_registered_action(ctx, "FTS_SESSION_INSERT_VERSE_REGION").await?;
    wait_for_region_names(&project, &["VS"]).await?;

    project.transport().set_position(8.0).await?;
    project.transport().set_time_selection(8.0, 16.0).await?;
    execute_registered_action(ctx, "FTS_SESSION_INSERT_CHORUS_REGION").await?;
    wait_for_region_names(&project, &["VS", "CH"]).await?;

    project.transport().clear_time_selection().await?;
    project.transport().set_position(4.0).await?;
    execute_registered_action(ctx, "FTS_SESSION_INSERT_PRE_CHORUS_REGION").await?;

    let regions = wait_for_region_names(&project, &["VS", "PRE-CH", "CH"]).await?;
    assert_eq!(regions[0].start_seconds(), 0.0);
    assert_eq!(regions[0].end_seconds(), 4.0);
    assert_eq!(regions[1].start_seconds(), 4.0);
    assert_eq!(regions[1].end_seconds(), 8.0);
    assert_eq!(regions[2].start_seconds(), 8.0);
    assert_eq!(regions[2].end_seconds(), 16.0);
    assert!(
        regions
            .windows(2)
            .all(|pair| { pair[0].end_seconds() <= pair[1].start_seconds() + 0.001 })
    );

    Ok(())
}

/// The midi-tools panels must be reachable as REAPER actions.
///
/// This is the whole point of `midi_tools_panel`: the crates and their
/// standalone examples proved the tools work, but until the actions
/// register there is no way to reach them from inside REAPER. If this
/// fails, the feature is invisible to the user however well it works.
#[reaper_test]
async fn midi_tools_actions_are_registered(ctx: &ReaperTestContext) -> eyre::Result<()> {
    wait_for_ready(ctx).await?;
    let actions = ctx.daw.action_registry();

    for id in ["FTS_MIDI_VELOCITY", "FTS_MIDI_ARP"] {
        let cmd_id = actions
            .lookup_command_id(id)
            .await?
            .ok_or_else(|| eyre::eyre!("{id} is not registered"))?;
        ctx.log(&format!("{id} → command id {cmd_id}"));
        eyre::ensure!(cmd_id > 0, "{id} registered with a null command id");

        eyre::ensure!(
            actions.is_in_action_list(id).await?,
            "{id} is registered but missing from the action list — it would \
             be unbindable"
        );
    }
    Ok(())
}
