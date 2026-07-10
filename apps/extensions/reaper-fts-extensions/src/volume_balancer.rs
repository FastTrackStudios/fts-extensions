//! Volume balancer — constant-sum fader linking.
//!
//! Rust port of the old "FTS Volume Balancer" ReaScript: tracks are linked
//! into groups; when the user moves one fader, the others move inversely so
//! the group's total (linear) volume stays constant. Unlike the Lua original
//! (which renormalized everything to a 1.0 sum, including the fader the user
//! just set), this keeps the user's fader where they put it and rescales only
//! the other members to restore the previous total.
//!
//! Groups live in **project ext state** (section `FTS_VOL_BALANCER`, keys
//! `group.<name>` holding `;`-joined track GUIDs) so they persist with the
//! project and other modules can register groups — dynamic-template writes a
//! `group.Parallel` entry when creating the drum-kit Parallel tracks.

use std::ffi::CString;
use std::os::raw::c_char;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Project ext-state section shared with dynamic-template.
const SECTION: &str = "FTS_VOL_BALANCER";
/// Re-read groups from project ext state every N timer ticks (~1s at 30Hz).
const RESYNC_TICKS: u32 = 30;
/// Linear-volume change below this is noise, not a fader move.
const EPSILON: f64 = 0.0001;

static ENABLED: AtomicBool = AtomicBool::new(true);
/// Set after link/unlink edits to force a group reload on the next tick.
static DIRTY: AtomicBool = AtomicBool::new(true);

struct Group {
    key: String,
    guids: Vec<String>,
    /// Volumes at the previous tick (parallel to `guids`); empty until the
    /// group has been observed once.
    prev: Vec<f64>,
}

#[derive(Default)]
struct State {
    groups: Vec<Group>,
    tick: u32,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if on {
        // Stale prev volumes would mis-detect a "change" on the first tick.
        if let Some(state) = STATE.lock().unwrap().as_mut() {
            for group in &mut state.groups {
                group.prev.clear();
            }
        }
    }
}

fn cstring(s: &str) -> Option<CString> {
    CString::new(s).ok()
}

/// GUIDs arrive in mixed shapes (`{...}` from `guidToString`, braceless from
/// reaper-high's `to_string_without_braces`, mixed case) — compare them
/// canonicalized: braceless, lowercase.
fn normalize_guid(guid: &str) -> String {
    guid.trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .to_ascii_lowercase()
}

/// Group names listed in the `groups` index key.
fn group_index() -> Vec<String> {
    use daw::service::ExtState;
    daw_reaper::Reaper
        .get_project(daw::service::ProjectContext::Current, SECTION, "groups")
        .unwrap_or_default()
        .split(';')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Read every linked group from the current project's ext state (index key
/// `groups` lists the names; each `group.<name>` holds its GUIDs).
fn load_groups() -> Vec<Group> {
    use daw::service::ExtState;
    let reaper = daw_reaper::Reaper;
    let project = || daw::service::ProjectContext::Current;
    let mut groups = Vec::new();
    for name in group_index() {
        let key = format!("group.{name}");
        let Some(val) = reaper.get_project(project(), SECTION, &key) else {
            continue;
        };
        let guids: Vec<String> = val
            .split(';')
            .map(normalize_guid)
            .filter(|g| !g.is_empty())
            .collect();
        if guids.len() >= 2 {
            groups.push(Group {
                key,
                guids,
                prev: Vec::new(),
            });
        }
    }
    groups
}

fn buf_to_string(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

/// GUID string (`{...}`) of a track.
fn track_guid_string(track: *mut reaper_low::raw::MediaTrack) -> String {
    let low = reaper_low::Reaper::get();
    let guid = unsafe { low.GetTrackGUID(track) };
    if guid.is_null() {
        return String::new();
    }
    let mut buf = [0_u8; 64];
    unsafe {
        low.guidToString(guid, buf.as_mut_ptr() as *mut c_char);
    }
    normalize_guid(&buf_to_string(&buf))
}

/// Snapshot every project track as (guid, track pointer).
fn project_tracks() -> Vec<(String, *mut reaper_low::raw::MediaTrack)> {
    let low = reaper_low::Reaper::get();
    let count = unsafe { low.CountTracks(std::ptr::null_mut()) };
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let tr = unsafe { low.GetTrack(std::ptr::null_mut(), i) };
        if tr.is_null() {
            continue;
        }
        let guid = track_guid_string(tr);
        if !guid.is_empty() {
            out.push((guid, tr));
        }
    }
    out
}

fn get_vol(track: *mut reaper_low::raw::MediaTrack) -> f64 {
    let low = reaper_low::Reaper::get();
    let Some(parm) = cstring("D_VOL") else {
        return 1.0;
    };
    unsafe { low.GetMediaTrackInfo_Value(track, parm.as_ptr()) }
}

fn set_vol(track: *mut reaper_low::raw::MediaTrack, vol: f64) {
    let low = reaper_low::Reaper::get();
    let Some(parm) = cstring("D_VOL") else {
        return;
    };
    unsafe {
        low.SetMediaTrackInfo_Value(track, parm.as_ptr(), vol.max(0.0));
    }
}

/// Per-tick poll, called from the main-thread timer callback.
pub fn poll() {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let mut state_guard = STATE.lock().unwrap();
    let state = state_guard.get_or_insert_with(State::default);
    state.tick = state.tick.wrapping_add(1);

    if DIRTY.swap(false, Ordering::AcqRel) || state.tick % RESYNC_TICKS == 0 {
        let fresh = load_groups();
        // Keep prev volumes for groups whose membership didn't change so a
        // resync never masks an in-flight fader move.
        let mut merged = Vec::with_capacity(fresh.len());
        for mut group in fresh {
            if let Some(old) = state
                .groups
                .iter()
                .find(|g| g.key == group.key && g.guids == group.guids)
            {
                group.prev = old.prev.clone();
            }
            merged.push(group);
        }
        if merged.len() != state.groups.len()
            || merged
                .iter()
                .zip(&state.groups)
                .any(|(a, b)| a.key != b.key || a.guids != b.guids)
        {
            tracing::info!(
                "[volume-balancer] groups synced: {:?}",
                merged
                    .iter()
                    .map(|g| format!("{} ({} tracks)", g.key, g.guids.len()))
                    .collect::<Vec<_>>()
            );
        }
        state.groups = merged;
    }
    if state.groups.is_empty() {
        return;
    }

    let tracks = project_tracks();
    let find = |guid: &str| tracks.iter().find(|(g, _)| g == guid).map(|(_, tr)| *tr);

    for group in &mut state.groups {
        // Resolve all members; a group with missing tracks (deleted, other
        // project tab) sits out until everything resolves again.
        let mut members = Vec::with_capacity(group.guids.len());
        for guid in &group.guids {
            match find(guid) {
                Some(tr) => members.push(tr),
                None => {
                    group.prev.clear();
                    members.clear();
                    break;
                }
            }
        }
        if members.len() < 2 {
            continue;
        }
        let vols: Vec<f64> = members.iter().map(|&tr| get_vol(tr)).collect();
        if group.prev.len() != vols.len() {
            group.prev = vols;
            continue;
        }

        let changed: Vec<usize> = (0..vols.len())
            .filter(|&i| (vols[i] - group.prev[i]).abs() > EPSILON)
            .collect();
        // Exactly one fader moved → user gesture; compensate the rest.
        // Several at once (undo, project load, our own writes) → resync.
        if let [moved] = changed[..] {
            let prev_sum: f64 = group.prev.iter().sum();
            let others_prev: f64 = prev_sum - group.prev[moved];
            let remaining = (prev_sum - vols[moved]).max(0.0);
            if others_prev > EPSILON {
                let scale = remaining / others_prev;
                for (i, &tr) in members.iter().enumerate() {
                    if i != moved {
                        set_vol(tr, group.prev[i] * scale);
                    }
                }
            }
        }
        group.prev = members.iter().map(|&tr| get_vol(tr)).collect();
    }
}

/// Link the currently selected tracks (≥2) into a new balancer group.
pub fn link_selected_tracks() {
    let low = reaper_low::Reaper::get();
    let count = unsafe { low.CountSelectedTracks(std::ptr::null_mut()) };
    if count < 2 {
        reaper_high::Reaper::get()
            .show_console_msg("FTS Volume Balancer: select at least 2 tracks to link\n");
        return;
    }
    let mut guids = Vec::with_capacity(count as usize);
    for i in 0..count {
        let tr = unsafe { low.GetSelectedTrack(std::ptr::null_mut(), i) };
        if tr.is_null() {
            continue;
        }
        let guid = track_guid_string(tr);
        if !guid.is_empty() {
            guids.push(guid);
        }
    }
    if guids.len() < 2 {
        return;
    }
    // First free sel.<n> slot.
    let names = group_index();
    let mut n = 1;
    while names.iter().any(|name| name == &format!("sel.{n}")) {
        n += 1;
    }
    save_group(&format!("sel.{n}"), &guids.join(";"));
    DIRTY.store(true, Ordering::Release);
    reaper_high::Reaper::get().show_console_msg(format!(
        "FTS Volume Balancer: linked {} tracks (sel.{n})\n",
        guids.len()
    ));
}

/// Write a group + keep the `groups` index in sync (same format
/// dynamic-template's create actions write).
fn save_group(name: &str, guid_csv: &str) {
    use daw::service::ExtState;
    let reaper = daw_reaper::Reaper;
    let project = || daw::service::ProjectContext::Current;
    if let Err(err) = reaper.set_project(project(), SECTION, &format!("group.{name}"), guid_csv) {
        tracing::warn!("[volume-balancer] failed to save group {name}: {err}");
        return;
    }
    let mut names = group_index();
    if !names.iter().any(|n| n == name) {
        names.push(name.to_string());
        if let Err(err) = reaper.set_project(project(), SECTION, "groups", &names.join(";")) {
            tracing::warn!("[volume-balancer] failed to update group index: {err}");
        }
    }
}

/// Remove every group containing any currently selected track.
pub fn unlink_selected_tracks() {
    let low = reaper_low::Reaper::get();
    let count = unsafe { low.CountSelectedTracks(std::ptr::null_mut()) };
    let mut selected = Vec::new();
    for i in 0..count {
        let tr = unsafe { low.GetSelectedTrack(std::ptr::null_mut(), i) };
        if !tr.is_null() {
            selected.push(track_guid_string(tr));
        }
    }
    if selected.is_empty() {
        return;
    }
    use daw::service::ExtState;
    let reaper = daw_reaper::Reaper;
    let project = || daw::service::ProjectContext::Current;
    let mut removed = 0;
    let mut kept: Vec<String> = Vec::new();
    for name in group_index() {
        let key = format!("group.{name}");
        let in_group = reaper
            .get_project(project(), SECTION, &key)
            .is_some_and(|val| {
                val.split(';')
                    .map(normalize_guid)
                    .any(|g| selected.contains(&g))
            });
        if in_group {
            let _ = reaper.delete_project(project(), SECTION, &key);
            removed += 1;
        } else {
            kept.push(name);
        }
    }
    if let Err(err) = reaper.set_project(project(), SECTION, "groups", &kept.join(";")) {
        tracing::warn!("[volume-balancer] failed to update group index: {err}");
    }
    DIRTY.store(true, Ordering::Release);
    reaper_high::Reaper::get().show_console_msg(format!(
        "FTS Volume Balancer: unlinked {removed} group(s)\n"
    ));
}

#[cfg(test)]
mod tests {
    /// The compensation math: one fader moves, the rest scale uniformly so
    /// the group total is unchanged.
    #[test]
    fn compensation_keeps_total_constant() {
        let prev = [1.0, 1.0, 1.0, 1.0, 1.0];
        let mut vols = prev;
        vols[2] = 2.0; // user pushes one fader up 6 dB

        let prev_sum: f64 = prev.iter().sum();
        let others_prev: f64 = prev_sum - prev[2];
        let remaining = (prev_sum - vols[2]).max(0.0);
        let scale = remaining / others_prev;
        for (i, v) in vols.iter_mut().enumerate() {
            if i != 2 {
                *v = prev[i] * scale;
            }
        }
        let total: f64 = vols.iter().sum();
        assert!((total - prev_sum).abs() < 1e-9);
        assert!((vols[0] - 0.75).abs() < 1e-9);
        assert_eq!(vols[2], 2.0);
    }
}
