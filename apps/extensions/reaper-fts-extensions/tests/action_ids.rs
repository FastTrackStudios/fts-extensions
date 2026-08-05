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
