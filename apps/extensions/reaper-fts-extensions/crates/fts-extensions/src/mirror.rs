//! Live MIDI mirror — source-of-truth tracks with library-adjusted twins.
//!
//! A **source track** holds score-time MIDI (what the user edits, what
//! exports back to notation). Its **mirror track** (hidden) holds the
//! library-facing copy: legato/attack timing pulls, re-anchored keyswitches,
//! re-bow pedal — produced by `keyflow_orchestra::mirror_part`. The mirror is
//! what routes to the sampler; it is regenerated, never hand-edited.
//!
//! Polled from `timer_callback`: every few ticks each source item's MIDI
//! content is hashed; on change the paired mirror item is rebuilt. Editing a
//! note on the source therefore updates the performance copy live.
//!
//! Tagging (track ext state, persists in the project):
//! - `P_EXT:FTS_MIRROR_ROLE`    = `source`
//! - `P_EXT:FTS_MIRROR_TARGET`  = mirror track GUID
//! - `P_EXT:FTS_MIRROR_PROFILE` = optional profile override
//!   (`strings`/`woodwinds`/`brass`/`brass_trumpet`/`generic`/`harp`);
//!   otherwise detected from the track name.
//!
//! Mirror items carry `P_EXT:FTS_MIRROR_SRC` = source item GUID so stale
//! copies can be found and replaced.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::hash::{Hash, Hasher};
use std::os::raw::c_char;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use keyflow_orchestra::engine::CcEvent;
use keyflow_orchestra::{Config, MidiNote, ProfileKind, detect_profile, mirror_part};
use reaper_low::raw::{MediaItem, MediaItem_Take, MediaTrack};

const ROLE_KEY: &str = "P_EXT:FTS_MIRROR_ROLE";
const TARGET_KEY: &str = "P_EXT:FTS_MIRROR_TARGET";
const PROFILE_KEY: &str = "P_EXT:FTS_MIRROR_PROFILE";
const ITEM_SRC_KEY: &str = "P_EXT:FTS_MIRROR_SRC";

/// Rescan/diff cadence in timer ticks (~30 Hz timer → ~10 Hz diff).
const POLL_TICKS: u32 = 3;

static ENABLED: AtomicBool = AtomicBool::new(true);
static TICK: AtomicU32 = AtomicU32::new(0);
/// source item GUID → (content hash at last regeneration, mirror track GUID).
static HASHES: Mutex<Option<HashMap<String, (u64, String)>>> = Mutex::new(None);

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if !on {
        *HASHES.lock().unwrap() = None; // full rebuild on re-enable
    }
}

// ---------------------------------------------------------------------
// Small raw-API helpers
// ---------------------------------------------------------------------

fn buf_to_string(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn track_guid(track: *mut MediaTrack) -> String {
    let low = reaper_low::Reaper::get();
    let guid = unsafe { low.GetTrackGUID(track) };
    if guid.is_null() {
        return String::new();
    }
    let mut buf = [0 as c_char; 64];
    unsafe { low.guidToString(guid, buf.as_mut_ptr()) };
    buf_to_string(&buf)
}

fn get_track_ext(track: *mut MediaTrack, key: &str) -> Option<String> {
    let low = reaper_low::Reaper::get();
    let key = CString::new(key).ok()?;
    let mut buf = [0 as c_char; 256];
    let ok =
        unsafe { low.GetSetMediaTrackInfo_String(track, key.as_ptr(), buf.as_mut_ptr(), false) };
    let s = buf_to_string(&buf);
    (ok && !s.is_empty()).then_some(s)
}

fn set_track_ext(track: *mut MediaTrack, key: &str, value: &str) {
    let low = reaper_low::Reaper::get();
    let (Ok(key), Ok(mut value)) = (
        CString::new(key),
        CString::new(value).map(|v| v.into_bytes_with_nul()),
    ) else {
        return;
    };
    unsafe {
        low.GetSetMediaTrackInfo_String(
            track,
            key.as_ptr(),
            value.as_mut_ptr() as *mut c_char,
            true,
        );
    }
}

fn get_item_ext(item: *mut MediaItem, key: &str) -> Option<String> {
    let low = reaper_low::Reaper::get();
    let key = CString::new(key).ok()?;
    let mut buf = [0 as c_char; 256];
    let ok = unsafe { low.GetSetMediaItemInfo_String(item, key.as_ptr(), buf.as_mut_ptr(), false) };
    let s = buf_to_string(&buf);
    (ok && !s.is_empty()).then_some(s)
}

fn set_item_ext(item: *mut MediaItem, key: &str, value: &str) {
    let low = reaper_low::Reaper::get();
    let (Ok(key), Ok(mut value)) = (
        CString::new(key),
        CString::new(value).map(|v| v.into_bytes_with_nul()),
    ) else {
        return;
    };
    unsafe {
        low.GetSetMediaItemInfo_String(item, key.as_ptr(), value.as_mut_ptr() as *mut c_char, true);
    }
}

fn item_guid(item: *mut MediaItem) -> String {
    get_item_ext(item, "GUID").unwrap_or_default()
}

fn find_track_by_guid(guid: &str) -> Option<*mut MediaTrack> {
    let low = reaper_low::Reaper::get();
    let n = unsafe { low.CountTracks(std::ptr::null_mut()) };
    for i in 0..n {
        let tr = unsafe { low.GetTrack(std::ptr::null_mut(), i) };
        if !tr.is_null() && track_guid(tr) == guid {
            return Some(tr);
        }
    }
    None
}

fn parse_profile(name: &str) -> Option<ProfileKind> {
    Some(match name {
        "strings" => ProfileKind::Strings,
        "woodwinds" => ProfileKind::Woodwinds,
        "brass" => ProfileKind::Brass,
        "brass_trumpet" => ProfileKind::BrassTrumpet,
        "generic" => ProfileKind::Generic,
        "harp" => ProfileKind::Harp,
        _ => return None,
    })
}

fn track_name(track: *mut MediaTrack) -> String {
    get_track_ext(track, "P_NAME").unwrap_or_default()
}

// ---------------------------------------------------------------------
// Source-item reading
// ---------------------------------------------------------------------

struct SourceMidi {
    notes: Vec<MidiNote>,
    ccs: Vec<CcEvent>,
    /// Content hash (notes + CCs + item bounds).
    hash: u64,
    /// Item bounds in project seconds.
    start_s: f64,
    end_s: f64,
}

/// Read one MIDI take's events into keyflow-orchestra's QN domain.
fn read_source(item: *mut MediaItem, take: *mut MediaItem_Take) -> SourceMidi {
    let low = reaper_low::Reaper::get();
    let mut hasher = DefaultHasher::new();

    let start_s = unsafe { low.GetMediaItemInfo_Value(item, c"D_POSITION".as_ptr()) };
    let len_s = unsafe { low.GetMediaItemInfo_Value(item, c"D_LENGTH".as_ptr()) };
    let end_s = start_s + len_s;
    start_s.to_bits().hash(&mut hasher);
    len_s.to_bits().hash(&mut hasher);

    let (mut note_cnt, mut cc_cnt, mut text_cnt) = (0, 0, 0);
    unsafe { low.MIDI_CountEvts(take, &mut note_cnt, &mut cc_cnt, &mut text_cnt) };

    let mut notes = Vec::with_capacity(note_cnt as usize);
    for idx in 0..note_cnt {
        let (mut sel, mut muted) = (false, false);
        let (mut sppq, mut eppq) = (0.0, 0.0);
        let (mut chan, mut pitch, mut vel) = (0, 0, 0);
        let ok = unsafe {
            low.MIDI_GetNote(
                take, idx, &mut sel, &mut muted, &mut sppq, &mut eppq, &mut chan, &mut pitch,
                &mut vel,
            )
        };
        if !ok || muted {
            continue;
        }
        let start_qn = unsafe { low.MIDI_GetProjQNFromPPQPos(take, sppq) };
        let end_qn = unsafe { low.MIDI_GetProjQNFromPPQPos(take, eppq) };
        (sppq.to_bits(), eppq.to_bits(), chan, pitch, vel).hash(&mut hasher);
        notes.push(MidiNote {
            start_qn,
            end_qn,
            chan: (chan + 1).clamp(1, 16) as u8, // orchestra is 1-based
            pitch,
            vel: vel.clamp(1, 127) as u8,
        });
    }

    let mut ccs = Vec::with_capacity(cc_cnt as usize);
    for idx in 0..cc_cnt {
        let (mut sel, mut muted) = (false, false);
        let mut ppq = 0.0;
        let (mut chanmsg, mut chan, mut msg2, mut msg3) = (0, 0, 0, 0);
        let ok = unsafe {
            low.MIDI_GetCC(
                take,
                idx,
                &mut sel,
                &mut muted,
                &mut ppq,
                &mut chanmsg,
                &mut chan,
                &mut msg2,
                &mut msg3,
            )
        };
        if !ok || muted || chanmsg != 0xB0 {
            continue; // CC messages only
        }
        let qn = unsafe { low.MIDI_GetProjQNFromPPQPos(take, ppq) };
        (ppq.to_bits(), chan, msg2, msg3).hash(&mut hasher);
        ccs.push(CcEvent {
            qn,
            chan: (chan + 1).clamp(1, 16) as u8,
            cc: msg2 as u8,
            val: msg3 as u8,
        });
    }

    SourceMidi {
        notes,
        ccs,
        hash: hasher.finish(),
        start_s,
        end_s,
    }
}

/// Project tempo map as keyflow-orchestra tempo points.
fn tempo_points() -> Vec<keyflow_orchestra::score::TempoPoint> {
    let low = reaper_low::Reaper::get();
    let proj = std::ptr::null_mut();
    let n = unsafe { low.CountTempoTimeSigMarkers(proj) };
    let mut out = Vec::with_capacity(n.max(1) as usize);
    for i in 0..n {
        let (mut timepos, mut measurepos, mut beatpos, mut bpm) = (0.0, 0, 0.0, 0.0);
        let (mut num, mut denom) = (0, 0);
        let mut linear = false;
        let ok = unsafe {
            low.GetTempoTimeSigMarker(
                proj,
                i,
                &mut timepos,
                &mut measurepos,
                &mut beatpos,
                &mut bpm,
                &mut num,
                &mut denom,
                &mut linear,
            )
        };
        if ok && bpm > 0.0 {
            let qn = unsafe { low.TimeMap2_timeToQN(proj, timepos) };
            out.push(keyflow_orchestra::score::TempoPoint { qn, bpm });
        }
    }
    if out.is_empty() {
        let (mut bpm, mut bpi) = (120.0, 4.0);
        unsafe { low.GetProjectTimeSignature2(proj, &mut bpm, &mut bpi) };
        out.push(keyflow_orchestra::score::TempoPoint { qn: 0.0, bpm });
    }
    out
}

// ---------------------------------------------------------------------
// Mirror-item writing
// ---------------------------------------------------------------------

/// Delete any existing mirror item for this source, then write a fresh one.
fn write_mirror_item(
    mirror_track: *mut MediaTrack,
    src_guid: &str,
    src: &SourceMidi,
    out: &keyflow_orchestra::MirrorOutput,
) {
    let low = reaper_low::Reaper::get();
    let proj = std::ptr::null_mut();

    // Remove stale copies (all mirror items tagged with this source GUID).
    let mut i = unsafe { low.CountTrackMediaItems(mirror_track) } - 1;
    while i >= 0 {
        let it = unsafe { low.GetTrackMediaItem(mirror_track, i) };
        if !it.is_null() && get_item_ext(it, ITEM_SRC_KEY).as_deref() == Some(src_guid) {
            unsafe { low.DeleteTrackMediaItem(mirror_track, it) };
        }
        i -= 1;
    }

    if out.notes.is_empty() {
        return;
    }

    // Item bounds: open early enough for pulled note-ons, keep the source end.
    let earliest_qn = out
        .notes
        .iter()
        .map(|n| n.start_qn)
        .fold(f64::INFINITY, f64::min);
    let earliest_s = unsafe { low.TimeMap2_QNToTime(proj, earliest_qn) };
    let start_s = src.start_s.min(earliest_s - 0.01);
    let no = false;
    let item = unsafe { low.CreateNewMIDIItemInProj(mirror_track, start_s, src.end_s, &no) };
    if item.is_null() {
        return;
    }
    let take = unsafe { low.GetActiveTake(item) };
    if take.is_null() {
        return;
    }

    let no_sort = true;
    for n in &out.notes {
        let sppq = unsafe { low.MIDI_GetPPQPosFromProjQN(take, n.start_qn) };
        let eppq = unsafe { low.MIDI_GetPPQPosFromProjQN(take, n.end_qn) };
        unsafe {
            low.MIDI_InsertNote(
                take,
                false,
                false,
                sppq,
                eppq,
                (n.chan as i32 - 1).clamp(0, 15),
                n.pitch,
                n.vel as i32,
                &no_sort,
            );
        }
    }
    for e in &out.ccs {
        let ppq = unsafe { low.MIDI_GetPPQPosFromProjQN(take, e.qn) };
        unsafe {
            low.MIDI_InsertCC(
                take,
                false,
                false,
                ppq,
                0xB0,
                (e.chan as i32 - 1).clamp(0, 15),
                e.cc as i32,
                e.val as i32,
            );
        }
    }
    unsafe { low.MIDI_Sort(take) };
    set_item_ext(item, ITEM_SRC_KEY, src_guid);
}

// ---------------------------------------------------------------------
// Poll loop
// ---------------------------------------------------------------------

/// Called every timer tick from `timer_callback`.
pub fn poll() {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let tick = TICK.fetch_add(1, Ordering::Relaxed);
    if tick % POLL_TICKS != 0 {
        return;
    }

    let low = reaper_low::Reaper::get();
    let n_tracks = unsafe { low.CountTracks(std::ptr::null_mut()) };

    let mut lock = HASHES.lock().unwrap();
    let hashes = lock.get_or_insert_with(HashMap::new);
    let mut seen: HashSet<String> = HashSet::new();
    let mut any_write = false;

    for ti in 0..n_tracks {
        let track = unsafe { low.GetTrack(std::ptr::null_mut(), ti) };
        if track.is_null() || get_track_ext(track, ROLE_KEY).as_deref() != Some("source") {
            continue;
        }
        let Some(target_guid) = get_track_ext(track, TARGET_KEY) else {
            continue;
        };
        let Some(mirror_track) = find_track_by_guid(&target_guid) else {
            // Mirror track was deleted — recreate it hidden and re-link.
            if let Some(new_track) = create_mirror_track(track) {
                set_track_ext(track, TARGET_KEY, &track_guid(new_track));
            }
            continue;
        };

        let profile = get_track_ext(track, PROFILE_KEY)
            .as_deref()
            .and_then(parse_profile)
            .unwrap_or_else(|| detect_profile(&track_name(track)));

        let n_items = unsafe { low.CountTrackMediaItems(track) };
        for ii in 0..n_items {
            let item = unsafe { low.GetTrackMediaItem(track, ii) };
            if item.is_null() {
                continue;
            }
            let take = unsafe { low.GetActiveTake(item) };
            if take.is_null() || !unsafe { low.TakeIsMIDI(take) } {
                continue;
            }
            let guid = item_guid(item);
            if guid.is_empty() {
                continue;
            }
            seen.insert(guid.clone());

            let src = read_source(item, take);
            if hashes.get(&guid).map(|(h, _)| *h) == Some(src.hash) {
                continue; // unchanged
            }

            let cfg = Config {
                profile,
                ..Default::default()
            };
            let tempos = tempo_points();
            let out = mirror_part(&src.notes, &src.ccs, None, &cfg, &tempos);
            write_mirror_item(mirror_track, &guid, &src, &out);
            hashes.insert(guid, (src.hash, target_guid.clone()));
            any_write = true;
        }
    }

    // Source items that vanished (any track) → drop their mirror copies.
    let stale: Vec<(String, String)> = hashes
        .iter()
        .filter(|(g, _)| !seen.contains(*g))
        .map(|(g, (_, mt))| (g.clone(), mt.clone()))
        .collect();
    for (guid, mirror_guid) in stale {
        if let Some(mirror_track) = find_track_by_guid(&mirror_guid) {
            let mut i = unsafe { low.CountTrackMediaItems(mirror_track) } - 1;
            while i >= 0 {
                let it = unsafe { low.GetTrackMediaItem(mirror_track, i) };
                if !it.is_null() && get_item_ext(it, ITEM_SRC_KEY).as_deref() == Some(&guid) {
                    unsafe { low.DeleteTrackMediaItem(mirror_track, it) };
                    any_write = true;
                }
                i -= 1;
            }
        }
        hashes.remove(&guid);
    }

    if any_write {
        unsafe { low.UpdateArrange() };
    }
}

// ---------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------

/// Create a hidden mirror track right after the given source track.
fn create_mirror_track(source: *mut MediaTrack) -> Option<*mut MediaTrack> {
    let low = reaper_low::Reaper::get();
    let idx = unsafe { low.GetMediaTrackInfo_Value(source, c"IP_TRACKNUMBER".as_ptr()) } as i32;
    if idx <= 0 {
        return None;
    }
    // IP_TRACKNUMBER is 1-based; inserting at `idx` places the new track
    // immediately after the source.
    unsafe { low.InsertTrackAtIndex(idx, false) };
    let track = unsafe { low.GetTrack(std::ptr::null_mut(), idx) };
    if track.is_null() {
        return None;
    }
    let name = format!("{} [mirror]", track_name(source));
    set_track_ext(track, "P_NAME", &name);
    unsafe {
        low.SetMediaTrackInfo_Value(track, c"B_SHOWINTCP".as_ptr(), 0.0);
        low.SetMediaTrackInfo_Value(track, c"B_SHOWINMIXER".as_ptr(), 0.0);
    }
    Some(track)
}

/// Action: make each selected track a mirror source (creates its hidden
/// mirror track if not already linked).
pub fn link_selected_tracks() {
    let low = reaper_low::Reaper::get();
    let n = unsafe { low.CountSelectedTracks(std::ptr::null_mut()) };
    let mut linked = 0;
    // Collect first — inserting tracks while iterating shifts indices.
    let selected: Vec<*mut MediaTrack> = (0..n)
        .map(|i| unsafe { low.GetSelectedTrack(std::ptr::null_mut(), i) })
        .filter(|t| !t.is_null())
        .collect();
    for track in selected {
        if get_track_ext(track, TARGET_KEY)
            .as_deref()
            .and_then(find_track_by_guid)
            .is_some()
        {
            continue; // already linked
        }
        let Some(mirror) = create_mirror_track(track) else {
            continue;
        };
        set_track_ext(track, ROLE_KEY, "source");
        set_track_ext(track, TARGET_KEY, &track_guid(mirror));
        linked += 1;
    }
    *HASHES.lock().unwrap() = None; // force full regeneration
    reaper_high::Reaper::get().show_console_msg(format!("FTS Mirror: linked {linked} track(s)\n"));
}

/// Action: unlink selected source tracks (tags cleared; mirror tracks are
/// left in place — delete them manually if unwanted).
pub fn unlink_selected_tracks() {
    let low = reaper_low::Reaper::get();
    let n = unsafe { low.CountSelectedTracks(std::ptr::null_mut()) };
    let mut unlinked = 0;
    for i in 0..n {
        let track = unsafe { low.GetSelectedTrack(std::ptr::null_mut(), i) };
        if track.is_null() || get_track_ext(track, ROLE_KEY).is_none() {
            continue;
        }
        set_track_ext(track, ROLE_KEY, "");
        set_track_ext(track, TARGET_KEY, "");
        set_track_ext(track, PROFILE_KEY, "");
        unlinked += 1;
    }
    reaper_high::Reaper::get()
        .show_console_msg(format!("FTS Mirror: unlinked {unlinked} track(s)\n"));
}

/// Action: force a full regeneration of every mirror on the next tick.
pub fn regenerate_all() {
    *HASHES.lock().unwrap() = None;
    reaper_high::Reaper::get().show_console_msg("FTS Mirror: full regeneration queued\n");
}
