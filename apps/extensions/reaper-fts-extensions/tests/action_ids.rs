//! Invariants over the *whole* registered action set.
//!
//! Every action module pins its own generated ids, but a per-module test
//! is blind to the two failure modes that actually bite:
//!
//! - the same command id registered by two different systems (the legacy
//!   `actions_proto::define_actions!` blocks and an
//!   `#[architect::actions]` trait emitting the same string), and
//! - the same *operation* registered under two different ids, which puts
//!   two entries in REAPER's action list for one thing and means a
//!   keybinding on the wrong one silently does nothing.
//!
//! This runs the real registration path — `register_all_architect_actions`,
//! the same function `plugin_main` calls — against a collecting backend,
//! so it sees exactly what REAPER would see. No REAPER required: registration
//! only stores `(meta, handler)`, and the handlers are never invoked.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use architect::action::{ActionBackend, ActionMeta};

/// An [`ActionBackend`] that records what it was handed instead of
/// registering it with a host.
#[derive(Clone, Default)]
struct CollectingBackend {
    seen: Arc<Mutex<Vec<&'static ActionMeta>>>,
}

impl ActionBackend for CollectingBackend {
    fn register(
        &self,
        meta: &'static ActionMeta,
        _handler: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    ) {
        self.seen.lock().unwrap().push(meta);
    }
}

impl CollectingBackend {
    fn collect() -> Vec<&'static ActionMeta> {
        let backend = CollectingBackend::default();
        reaper_fts_extensions::register_all_architect_actions(&backend);
        let seen = backend.seen.lock().unwrap().clone();
        assert!(
            !seen.is_empty(),
            "no actions registered — is the mod-session feature off?"
        );
        seen
    }
}

/// Two traits emitting the same command id is a silent conflict: REAPER's
/// registry is idempotent per id, so whichever registers second is simply
/// discarded, and the surviving handler may be the wrong one.
#[test]
fn architect_action_ids_are_unique() {
    let mut by_id: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for meta in CollectingBackend::collect() {
        by_id
            .entry(meta.id)
            .or_default()
            .push(format!("{}::{}", meta.trait_name, meta.method_name));
    }

    let dupes: Vec<_> = by_id.iter().filter(|(_, v)| v.len() > 1).collect();
    assert!(
        dupes.is_empty(),
        "duplicate architect action ids:\n{}",
        dupes
            .iter()
            .map(|(id, owners)| format!("  {id}  <-  {}", owners.join(", ")))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The legacy `session_actions` block must not declare anything the
/// architect traits already emit. Both paths register with REAPER, so an
/// overlap means one operation with two registrations — and historically
/// one of them wired to nothing.
#[test]
fn legacy_session_defs_do_not_collide_with_architect_ids() {
    let architect_ids: std::collections::BTreeSet<&str> =
        CollectingBackend::collect().iter().map(|m| m.id).collect();

    let overlap: Vec<String> = session::session_actions::definitions()
        .iter()
        .map(|def| def.id.to_command_id())
        .filter(|id| architect_ids.contains(id.as_str()))
        .collect();

    assert!(
        overlap.is_empty(),
        "legacy session_actions entries duplicate architect ids \
         (delete the define_actions! entry — the macro already emits it):\n{}",
        overlap
            .iter()
            .map(|id| format!("  {id}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Everything still declared the legacy way, with the reason it's still
/// there. The point is that adding an entry to `session_actions` is a
/// decision someone has to make on purpose: the default path for a new
/// REAPER action is an `#[architect::actions]` trait, and anything left
/// in the `define_actions!` block is either awaiting migration or
/// deliberately not a REAPER command at all.
///
/// If this fails because you added an entry: migrate it instead, or add
/// it here with a note saying why it can't be.
#[test]
fn legacy_session_defs_are_an_explicit_allowlist() {
    // RPC-only. These route through `SetlistServiceImpl::go_to_song_impl`
    // / `go_to_section_impl`, which depend on `ensure_song_hydrated` — an
    // async, timeout-bounded rebuild path that can't be collapsed into a
    // synchronous REAPER action callback without either risking a
    // main-thread deadlock or silently no-opping on a cache miss. They
    // have never been dispatchable as REAPER commands; see
    // `session_proto::playback`'s module doc.
    const RPC_ONLY: &[&str] = &[
        "FTS_SESSION_SMART_NEXT",
        "FTS_SESSION_SMART_PREVIOUS",
        "FTS_SESSION_NEXT_SONG",
        "FTS_SESSION_PREVIOUS_SONG",
        "FTS_SESSION_NEXT_SECTION",
        "FTS_SESSION_PREVIOUS_SECTION",
    ];

    // Compatibility aliases for `dynamic_template` actions that this crate
    // does not implement. They survive only because committed FTS config
    // still binds these names — `reaper-input`'s tracks.styx /
    // mode-organize.styx and `fts-icons`' tracks.toml. Retiring them means
    // repointing that config at the FTS_DYNAMIC_TEMPLATE_* ids AND
    // re-running `fts-icons build --install`, so it's a sequenced change,
    // not a refactor. See `dynamic_template::daw_module::dispatch_session_command`.
    //
    // Every alias with no committed binding is already gone.
    const DYNAMIC_TEMPLATE_ALIASES: &[&str] = &[
        "FTS_SESSION_ORGANIZE_EVERYTHING",
        "FTS_SESSION_ORGANIZE_SELECTED_TRACKS",
        "FTS_SESSION_CREATE_NEW_DRUM_KIT",
        "FTS_SESSION_CREATE_NEW_ELECTRONIC_DRUMS",
        "FTS_SESSION_CREATE_NEW_BASS_GUITAR",
        "FTS_SESSION_CREATE_NEW_ELECTRIC_GUITAR",
        "FTS_SESSION_CREATE_NEW_ACOUSTIC_GUITAR",
        "FTS_SESSION_CREATE_NEW_KEYS",
        "FTS_SESSION_CREATE_NEW_SYNTH",
        "FTS_SESSION_CREATE_NEW_LEAD_VOCALS",
        "FTS_SESSION_CREATE_NEW_BACKGROUND_VOCALS",
        "FTS_SESSION_CREATE_NEW_ORCHESTRAL_BRASS",
        "FTS_SESSION_CREATE_NEW_ORCHESTRAL_WOODWINDS",
        "FTS_SESSION_CREATE_NEW_ORCHESTRAL_STRINGS",
        "FTS_SESSION_CREATE_NEW_ORCHESTRAL_PERCUSSION",
        "FTS_SESSION_CREATE_NEW_SFX",
    ];

    let actual: Vec<String> = session::session_actions::definitions()
        .iter()
        .map(|def| def.id.to_command_id())
        .collect();

    let unexpected: Vec<&String> = actual
        .iter()
        .filter(|id| {
            !RPC_ONLY.contains(&id.as_str())
                && !DYNAMIC_TEMPLATE_ALIASES.contains(&id.as_str())
        })
        .collect();

    assert!(
        unexpected.is_empty(),
        "unexpected legacy session_actions entries — migrate to \
         #[architect::actions] or document here why not:\n{}",
        unexpected
            .iter()
            .map(|id| format!("  {id}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Not an assertion — a way to print the authoritative registered id set.
/// `cargo test -p fts-extensions --test action_ids -- --ignored --nocapture dump`
#[test]
#[ignore = "diagnostic: prints every registered action id"]
fn dump_all_ids() {
    for meta in CollectingBackend::collect() {
        println!("{}\t{}", meta.id, meta.trait_name);
    }
}

// ─── Committed config must reference actions that actually exist ────────

/// Files in this repo that bind REAPER named commands. A `_FTS_*` string
/// in any of them is a keybinding or a toolbar button: if the id doesn't
/// resolve, the key does nothing and the button does nothing, silently.
const BINDING_CONFIGS: &[&str] = &[
    "../../../features/reaper/reaper-input/config/config/fasttrackstudio/tracks.styx",
    "../../../features/reaper/reaper-input/config/config/workflows/mode-organize.styx",
    "../../../features/reaper/fts-icons/examples/tracks.toml",
];

/// Bindings that are known-dead today, with the reason. Every one is a
/// `FTS_SESSION_CREATE_NEW_*` alias whose target doesn't exist:
/// `dispatch_session_command` maps the name through to
/// `FTS_DYNAMIC_TEMPLATE_CREATE_NEW_<suffix>`, and for these the suffix
/// names no action, so the dispatch is a no-op.
///
/// Fixing them means repointing the config at the real
/// `FTS_DYNAMIC_TEMPLATE_CREATE_NEW_*` ids (which do exist for all but
/// MIX_BUS) and re-running `fts-icons build --install` for the toolbar.
/// Listed here rather than silently tolerated so the count can only go
/// down.
const KNOWN_DEAD_BINDINGS: &[&str] = &[
    // Not declared in session_actions at all, so REAPER can't even
    // resolve the named command. Real ids exist under FTS_DYNAMIC_TEMPLATE_.
    "_FTS_SESSION_CREATE_NEW_PERCUSSION",
    "_FTS_SESSION_CREATE_NEW_PIANO",
    "_FTS_SESSION_CREATE_NEW_ELECTRIC_KEYS",
    "_FTS_SESSION_CREATE_NEW_ORGAN",
    "_FTS_SESSION_CREATE_NEW_SYNTH_ARP",
    "_FTS_SESSION_CREATE_NEW_SYNTH_BASS",
    "_FTS_SESSION_CREATE_NEW_SYNTH_LEAD",
    "_FTS_SESSION_CREATE_NEW_SYNTH_PAD",
    // No dynamic-template action of this name exists under any prefix —
    // this one needs a decision, not a rename.
    "_FTS_SESSION_CREATE_NEW_MIX_BUS",
    // Declared and registered, but the alias maps to a
    // FTS_DYNAMIC_TEMPLATE_CREATE_NEW_* id that doesn't exist, so the
    // button resolves and then does nothing.
    "_FTS_SESSION_CREATE_NEW_SYNTH",
    "_FTS_SESSION_CREATE_NEW_ORCHESTRAL_BRASS",
    "_FTS_SESSION_CREATE_NEW_ORCHESTRAL_WOODWINDS",
    "_FTS_SESSION_CREATE_NEW_ORCHESTRAL_STRINGS",
    "_FTS_SESSION_CREATE_NEW_ORCHESTRAL_PERCUSSION",
];

/// Resolve a config's `_FTS_…` reference the way REAPER plus the alias
/// shim would: the id must either be registered directly, or be an
/// `FTS_SESSION_*` alias whose mapped target is registered.
fn resolves(reference: &str, registered: &std::collections::BTreeSet<&str>) -> bool {
    let id = reference.trim_start_matches('_');
    if registered.contains(id) {
        return true;
    }
    let mapped = match id {
        "FTS_SESSION_ORGANIZE_EVERYTHING" => "FTS_DYNAMIC_TEMPLATE_SORT_ALL".to_string(),
        "FTS_SESSION_ORGANIZE_SELECTED_TRACKS" => "FTS_DYNAMIC_TEMPLATE_SORT_SELECTED".to_string(),
        other => match other.strip_prefix("FTS_SESSION_CREATE_NEW_") {
            Some(suffix) => {
                let suffix = match suffix {
                    "ELECTRONIC_DRUMS" => "ELECTRONIC_KIT",
                    "SYNTH_BASS" => "BASS_SYNTH",
                    s => s,
                };
                format!("FTS_DYNAMIC_TEMPLATE_CREATE_NEW_{suffix}")
            }
            None => return false,
        },
    };
    registered.contains(mapped.as_str())
}

/// A keybinding or toolbar button pointing at an id nothing registers is
/// invisible: REAPER shows the binding, pressing it does nothing, and no
/// error surfaces anywhere. This test is the only thing that connects the
/// two halves.
#[test]
fn committed_bindings_resolve_to_registered_actions() {
    // Both registration systems: the architect traits, and this crate's
    // own legacy `ActionDefs` list (the FTS_TEMPO_* / FTS_ITEM_* family).
    // A config reference is live if either one claims it.
    let architect: Vec<&'static ActionMeta> = CollectingBackend::collect();
    let legacy: Vec<String> = reaper_fts_extensions::actions::build_action_defs()
        .into_iter()
        .map(|(id, ..)| id)
        .collect();
    let mut registered: std::collections::BTreeSet<&str> =
        architect.iter().map(|m| m.id).collect();
    registered.extend(legacy.iter().map(|s| s.as_str()));

    let re = regex_lite_fts_ids();
    let mut dead: Vec<String> = Vec::new();
    let mut stale_allowlist: Vec<&str> = KNOWN_DEAD_BINDINGS.to_vec();

    for rel in BINDING_CONFIGS {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        for reference in re(&text) {
            if resolves(&reference, &registered) {
                stale_allowlist.retain(|k| *k != reference);
                continue;
            }
            if KNOWN_DEAD_BINDINGS.contains(&reference.as_str()) {
                stale_allowlist.retain(|k| *k != reference);
                continue;
            }
            dead.push(format!("  {reference}  ({rel})"));
        }
    }

    assert!(
        dead.is_empty(),
        "committed config binds action ids that nothing registers — \
         these keys/buttons silently do nothing:\n{}",
        dead.join("\n")
    );
    assert!(
        stale_allowlist.is_empty(),
        "KNOWN_DEAD_BINDINGS lists entries that now resolve (or are gone) \
         — delete them from the list:\n{}",
        stale_allowlist
            .iter()
            .map(|k| format!("  {k}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Pull every `"_FTS_…"` quoted action reference out of a config file.
/// Deliberately format-agnostic: styx and toml both quote them, and a
/// dumb scan can't go stale against either grammar.
fn regex_lite_fts_ids() -> impl Fn(&str) -> Vec<String> {
    |text: &str| {
        let mut out = Vec::new();
        for (i, _) in text.match_indices("\"_FTS_") {
            let rest = &text[i + 1..];
            if let Some(end) = rest.find('"') {
                out.push(rest[..end].to_string());
            }
        }
        out
    }
}
