# fts-extensions

FastTrackStudio's DAW-host extensions — one shared library per host,
loaded by the DAW itself. REAPER is the first host.

```
apps/extensions/reaper-fts-extensions/   the REAPER cdylib
                              actions/   the action registry
                                xtask/   the in-REAPER test harness
```

Split out of the FastTrackStudio shell in August 2026. It carries real
domain logic rather than registration glue:

| area | what it does |
|---|---|
| `src/tempo/` | tempo-map editing — grid moves, envelopes, transient snapping, time signatures |
| `src/volume_balancer.rs` | balances item/track volumes across a selection |
| `src/mirror.rs` | mirrors edits across paired tracks |
| `src/expression_mouse.rs` | the expression-editor mouse layer |
| `src/continuous_action.rs` | held-key / repeating action framework |

## Where the rest lives

The REAPER *platform* — the FFI bindings, the project model, theming,
keybinds, the extension bootstrap — is in
[daw](https://github.com/FastTrackStudios/daw) (~111k lines across
`daw-reaper`, `reaper-input`, `reaper-config`, `fts-themer`). This repo
is what binds it into a loadable extension and adds the editing tools.

Signal does **not** come through here: it reaches REAPER as a CLAP
plugin, so this extension never links the audio engine.

```
daw -> session -> signal -> { fts-extensions, FastTrackStudio, ... }
```

## Build

```bash
nix develop
cargo check --workspace
```

The REAPER integration suite (13 tests, in-process) needs a licensed
REAPER and runs self-hosted:

```bash
cargo run -p fts-extensions-xtask
```

## Licence

GPL-3.0-or-later.
