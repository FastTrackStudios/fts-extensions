//! The midi-tools panels, as REAPER actions.
//!
//! `features/midi-tools` is DAW-agnostic: the panels take no props and
//! read their backend from Dioxus context, and the backend is any type
//! implementing the `daw` service traits. This module is the twenty lines
//! that say "the backend is this REAPER, and here are the actions that
//! open the panels".
//!
//! Rendered `Native` (Blitz), not `Desktop` (WebView), because the
//! WebView path breaks VST parity — see the signal UI rules. The
//! midi-tools panels are written for it: inline styles throughout, no
//! external stylesheet, no `asset!()`.

use daw::module::{ActionDef, DockPosition, PanelComponent, PanelDef, PanelRenderer};
use daw::reaper_ui::prelude::*;
use midi_tools_daw::{DawArpSink, DawVelocitySink};
use midi_tools_ui::{ArpPanel, ArpSinkHandle, SinkHandle, VelocityPanel};

/// Blitz renders these panels without a stylesheet, so the only thing
/// worth injecting is the bits Blitz gets wrong on its own. The panels
/// carry their own theme.
const BLITZ_FIXES: &str = r#"
button { cursor: pointer !important; }
:root { color-scheme: dark; }
"#;

/// The velocity panel, bound to this REAPER.
///
/// The sink is built per mount rather than held in a static: it binds to
/// whatever item is selected when you open the panel, and rebuilding it
/// is how re-opening the panel retargets. A cached one would keep
/// pointing at the take you first used it on.
#[component]
pub fn MidiVelocityPanel() -> Element {
    use_context_provider(|| SinkHandle::new(DawVelocitySink::new(daw_reaper::Reaper)));

    rsx! {
        document::Style { {BLITZ_FIXES} }
        VelocityPanel {}
    }
}

/// The arpeggiator panel, bound to this REAPER.
#[component]
pub fn MidiArpPanel() -> Element {
    use_context_provider(|| ArpSinkHandle::new(DawArpSink::new(daw_reaper::Reaper)));

    rsx! {
        document::Style { {BLITZ_FIXES} }
        ArpPanel {}
    }
}

pub fn panel_defs() -> [PanelDef; 2] {
    [
        PanelDef {
            id: "FTS_MIDI_VELOCITY",
            title: "MIDI Velocity",
            component: PanelComponent::from_fn_ptr(MidiVelocityPanel as fn() -> _ as *const ()),
            default_dock: DockPosition::Floating,
            renderer: PanelRenderer::Native,
            // Tall and narrow: the panel is a stack of sections, and it
            // sits beside a MIDI editor rather than over it.
            default_size: (520.0, 760.0),
            toggle_action: Some("FTS_MIDI_VELOCITY"),
        },
        PanelDef {
            id: "FTS_MIDI_ARP",
            title: "MIDI Arpeggiator",
            component: PanelComponent::from_fn_ptr(MidiArpPanel as fn() -> _ as *const ()),
            default_dock: DockPosition::Floating,
            renderer: PanelRenderer::Native,
            default_size: (520.0, 700.0),
            toggle_action: Some("FTS_MIDI_ARP"),
        },
    ]
}

pub fn action_defs() -> [ActionDef; 2] {
    [
        ActionDef::new("FTS_MIDI_VELOCITY", "FTS: MIDI Velocity", || {
            daw::reaper_ui::dock::toggle_panel("FTS_MIDI_VELOCITY");
        })
        .in_menu(),
        ActionDef::new("FTS_MIDI_ARP", "FTS: MIDI Arpeggiator", || {
            daw::reaper_ui::dock::toggle_panel("FTS_MIDI_ARP");
        })
        .in_menu(),
    ]
}
