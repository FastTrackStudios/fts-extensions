//! Reaper-Dioxus UI Component Test Panel
//!
//! Renders the architect-ui Showcase in the native Dioxus/Blitz renderer.

use daw::module::{ActionDef, DockPosition, PanelComponent, PanelDef, PanelRenderer};
use daw::reaper_ui::prelude::*;

const TAILWIND_CSS: &str = include_str!("../assets/tailwind.css");
const FTS_THEME_CSS: &str = architect_ui::THEME_CSS;

const BLITZ_FIXES: &str = r#"
input, textarea, select, button { cursor: auto !important; }
input:disabled, textarea:disabled, button:disabled { cursor: not-allowed !important; }
:root { color-scheme: dark; }
"#;

#[component]
pub fn UiTestPanel() -> Element {
    rsx! {
        document::Style { {TAILWIND_CSS} }
        document::Style { {FTS_THEME_CSS} }
        document::Style { {BLITZ_FIXES} }

        architect_ui::showcase::Showcase {}
    }
}

pub fn panel_defs() -> [PanelDef; 2] {
    [
        PanelDef {
            id: "FTS_UI_NATIVE",
            title: "FTS UI Native",
            component: PanelComponent::from_fn_ptr(UiTestPanel as fn() -> _ as *const ()),
            default_dock: DockPosition::Floating,
            renderer: PanelRenderer::Native,
            default_size: (900.0, 700.0),
            toggle_action: Some("fts-ui-native"),
        },
        PanelDef {
            id: "FTS_UI_DESKTOP",
            title: "FTS UI Desktop",
            component: PanelComponent::from_fn_ptr(UiTestPanel as fn() -> _ as *const ()),
            default_dock: DockPosition::Floating,
            renderer: PanelRenderer::Desktop,
            default_size: (900.0, 700.0),
            toggle_action: Some("fts-ui-desktop"),
        },
    ]
}

pub fn action_defs() -> [ActionDef; 2] {
    [
        ActionDef::new("fts-ui-native", "FTS: UI Native", || {
            daw::reaper_ui::dock::toggle_panel("FTS_UI_NATIVE");
        })
        .in_menu(),
        ActionDef::new("fts-ui-desktop", "FTS: UI Desktop", || {
            daw::reaper_ui::dock::toggle_panel("FTS_UI_DESKTOP");
        })
        .in_menu(),
    ]
}
