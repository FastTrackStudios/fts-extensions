//! Standalone — renders fts-ui Showcase in dioxus-native.

use dioxus_native::prelude::*;

const TAILWIND_CSS: &str = include_str!("../../fts-extensions/assets/tailwind.css");
const FTS_THEME_CSS: &str = include_str!("../../fts-extensions/assets/fts-theme.css");

const BLITZ_FIXES: &str = r#"
input, textarea, select, button { cursor: auto !important; }
input:disabled, textarea:disabled, button:disabled { cursor: not-allowed !important; }
:root { color-scheme: dark; }
"#;

#[component]
fn App() -> Element {
    rsx! {
        document::Style { {TAILWIND_CSS} }
        document::Style { {FTS_THEME_CSS} }
        document::Style { {BLITZ_FIXES} }

        fts_ui::showcase::Showcase {}
    }
}

fn main() {
    dioxus_native::launch(App);
}
