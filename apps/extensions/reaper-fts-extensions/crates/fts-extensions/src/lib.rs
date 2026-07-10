//! FTS Extensions — unified REAPER extension.
//!
//! Loads all FTS modules in-process: launcher, session,
//! DAW sync, and input. Each module implements `daw::DawModule` and
//! registers its own actions and event subscriptions.
//!
//! This file is the host — it collects modules, initializes them, and
//! registers their actions with REAPER. The modules own their logic.

use std::cell::RefCell;
use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, OnceLock};

use crossbeam_channel::{Receiver, Sender};
use daw::module::{self, DawModule, ModuleContext};
use daw::rpc::Daw;
use daw::service::ActionRegistration;
use fragile::Fragile;
use reaper_high::{MainTaskMiddleware, MainThreadTask, Reaper as HighReaper, TaskSupport};
use reaper_low::PluginContext;
use reaper_macros::reaper_extension_plugin;
use reaper_medium::ReaperSession;
use tracing::{info, warn};

// ── Global State ─────────────────────────────────────────────────────────────

pub static GLOBAL: OnceLock<Global> = OnceLock::new();

pub struct Global {
    pub task_support: TaskSupport,
    pub task_sender: Sender<MainThreadTask>,
    task_receiver: Receiver<MainThreadTask>,
    pub daw: Daw,
    pub tokio_runtime: Arc<tokio::runtime::Runtime>,
    _log_guard: tracing_appender::non_blocking::WorkerGuard,
}

/// Audio-sync handles (single-project cell + multi-project registry).
/// Held in its own static so the timer callback can borrow it without
/// reaching through the [`Global`] singleton; `None` until the post-
/// session registration in `plugin_main` completes.
pub static AUDIO_SYNC: OnceLock<Option<daw_reaper::extension_setup::AudioSyncHandle>> =
    OnceLock::new();

impl Global {
    pub fn get() -> &'static Global {
        GLOBAL.get().expect("Global not initialized")
    }

    pub fn try_daw() -> Option<&'static Daw> {
        GLOBAL.get().map(|g| &g.daw)
    }
}

// ── App ──────────────────────────────────────────────────────────────────────

struct App {
    session: RefCell<ReaperSession>,
    task_middleware: RefCell<MainTaskMiddleware>,
    /// REAPER-action → handler map. Wrapped in `RefCell` so the
    /// late-registration path (e.g. reaper-input discovering a brand
    /// new workflow `.styx` file at runtime) can push new entries
    /// without rebuilding `App`. REAPER's main thread is the only
    /// touchpoint, so a `RefCell` is sufficient.
    action_handlers: RefCell<HashMap<String, Arc<dyn Fn() + Send + Sync>>>,
}

impl App {
    fn process_tasks(&self) {
        self.task_middleware.borrow_mut().run();
    }

    fn dispatch_action(&self, command_name: &str) {
        let handler = self.action_handlers.borrow().get(command_name).cloned();
        if let Some(handler) = handler {
            handler();
        } else {
            tracing::debug!("Unhandled action: {command_name}");
        }
    }

    /// Register a [`DynActionDef`] discovered after startup. Adds the
    /// handler to the dispatch map and registers the command name with
    /// REAPER's action list so `named_command_lookup` resolves it the
    /// next time a binding fires it. No-op if the command is already
    /// registered (REAPER's registry handles the duplicate case).
    fn register_late_action(
        &self,
        def: reaper_input::infrastructure::action_registry::DynActionDef,
    ) {
        let cmd_id = daw_reaper::action_registry::register_action_main_thread(
            &def.command_id,
            &def.display_name,
            def.appears_in_menu,
            def.toggle_state.is_some(),
        );
        if cmd_id > 0 {
            info!(
                command_id = %def.command_id,
                cmd_id,
                "Late-registered action"
            );
        } else {
            warn!(
                command_id = %def.command_id,
                "Failed to late-register action with REAPER"
            );
        }
        self.action_handlers
            .borrow_mut()
            .insert(def.command_id.clone(), def.handler);
    }
}

static APP: OnceLock<Fragile<App>> = OnceLock::new();

// ── Existing modules (not yet DawModule) ─────────────────────────────────────

mod actions;
mod continuous_action;
mod error;
mod item_actions;
mod menu;
#[cfg(all(feature = "mod-session", feature = "mod-input"))]
mod mode_input;
#[cfg(feature = "mod-session")]
mod mode_selector;
#[cfg(feature = "mod-session")]
mod mode_toolbars;
mod reaper_utils;
mod sync_settings;
mod tempo;
#[cfg(feature = "ui-dock")]
mod ui_test_panel;

// ── Timer callback ───────────────────────────────────────────────────────────

fn catch_panic(label: &str, f: impl FnOnce() + std::panic::UnwindSafe) {
    if let Err(e) = std::panic::catch_unwind(f) {
        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            format!("{e:?}")
        };
        warn!("timer_callback panicked in {label}: {msg}");
    }
}

#[cfg(feature = "mod-input")]
static INPUT_PREWARM_TICKS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// ~2s @ 30Hz: wait this long after plugin load before touching wgpu so
// REAPER's main HWND is fully realised. Below this tick count, the
// swapchain configure call returns "invalid surface".
#[cfg(feature = "mod-input")]
const INPUT_PREWARM_TICK_TARGET: u32 = 60;

// Tracks whether we've enqueued the prefix list yet. Set on the first tick
// >= TARGET; from then on we drain one overlay per tick until done.
#[cfg(feature = "mod-input")]
static INPUT_PREWARM_QUEUED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

extern "C" fn timer_callback() {
    if let Some(app_fragile) = APP.get() {
        let app = app_fragile.get();
        catch_panic(
            "process_tasks",
            std::panic::AssertUnwindSafe(|| app.process_tasks()),
        );
        // Deferred, paced prewarm: wait ~2s for REAPER's main HWND, then
        // enqueue the prefix list and build a handful of overlays per tick.
        // Each overlay build now only allocates its own surface + Vello
        // renderer because Instance/Adapter/Device are shared (warmed in
        // the background from plugin_main), so per-tick cost is much lower
        // and we can drain the queue faster without locking the host.
        #[cfg(feature = "mod-input")]
        {
            // Root-only prewarm: each overlay is ~80ms on the main thread.
            // 1 per tick = one missed frame per overlay, imperceptible. A
            // typical config has ~10 root prefixes so the queue drains in
            // ~10 ticks (~330ms at 30Hz) without any visible hitching.
            const OVERLAYS_PER_TICK: usize = 1;
            let tick = INPUT_PREWARM_TICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if tick >= INPUT_PREWARM_TICK_TARGET {
                if !INPUT_PREWARM_QUEUED.swap(true, std::sync::atomic::Ordering::AcqRel) {
                    catch_panic(
                        "enqueue_which_key_overlay_prewarm",
                        reaper_input::enqueue_which_key_overlay_prewarm,
                    );
                }
                catch_panic("prewarm_which_key_overlay_step", || {
                    for _ in 0..OVERLAYS_PER_TICK {
                        if !reaper_input::prewarm_which_key_overlay_step() {
                            break;
                        }
                    }
                });
            }
        }
        #[cfg(feature = "poll-broadcast")]
        {
            catch_panic(
                "poll_and_broadcast_items",
                daw_reaper::poll_and_broadcast_items,
            );
            catch_panic(
                "poll_and_broadcast_tempo_map",
                daw_reaper::poll_and_broadcast_tempo_map,
            );
            catch_panic(
                "poll_and_broadcast_transport",
                daw_reaper::poll_and_broadcast_transport,
            );
            catch_panic(
                "poll_and_broadcast_markers",
                daw_reaper::poll_and_broadcast_markers,
            );
            catch_panic(
                "poll_and_broadcast_regions",
                daw_reaper::poll_and_broadcast_regions,
            );
            catch_panic(
                "poll_and_broadcast_tracks",
                daw_reaper::poll_and_broadcast_tracks,
            );
        }
        // Keep the audio-sync multi-project slot table in sync with
        // REAPER's open project tabs. Cheap (one `enum_projects` loop)
        // and safe to call every tick. Skipped when registration
        // failed at startup.
        if let Some(Some(handle)) = AUDIO_SYNC.get() {
            let registry = handle.registry.clone();
            catch_panic("refresh_audio_sync_registry", move || {
                daw_reaper::extension_setup::refresh_audio_sync_registry(&registry);
            });
        }

        catch_panic("_fire_timer_callbacks", daw::_fire_timer_callbacks);
        catch_panic(
            "process_pending_actions",
            std::panic::AssertUnwindSafe(|| process_pending_actions(app)),
        );
        #[cfg(feature = "ui-dock")]
        catch_panic("update_panels", daw::ui::dock::update_panels);
    }
}

static ACTION_CHANNEL: OnceLock<(Sender<String>, Receiver<String>)> = OnceLock::new();

fn action_channel() -> &'static (Sender<String>, Receiver<String>) {
    ACTION_CHANNEL.get_or_init(|| crossbeam_channel::unbounded())
}

fn process_pending_actions(app: &App) {
    let (_, rx) = action_channel();
    while let Ok(command_name) = rx.try_recv() {
        app.dispatch_action(&command_name);
    }
}

// ── Initialisation ───────────────────────────────────────────────────────────

fn initialize_daw(tokio_runtime: &tokio::runtime::Runtime) -> eyre::Result<Daw> {
    use daw_reaper::socket_publisher::publish_extension_socket;
    use daw_reaper::{build_extension_daw_with, create_daw_handler};

    tokio_runtime
        .block_on(async {
            // Session owns DAW-adjacent session services such as MIDI chart
            // hydration. Those services resolve the global DAW lazily at call
            // time so startup only creates one in-process service graph.
            let handler = create_daw_handler();
            #[cfg(feature = "mod-session")]
            let handler = {
                let h = session::daw_services::layer_services_with_daw(handler, daw_reaper::Reaper);
                // Mount the FTS session control surfaces (mode /
                // take-ranking / record control) so external Vox
                // peers — CLI, desktop, mobile — can call them.
                session::daw_services::layer_control_surfaces(h)
            };

            // Publish the same service router on a Unix socket so
            // external apps (CLI, desktop, mobile, audio-sync peers)
            // can call into the in-process surface. The publisher
            // clones the router internally (Arc-backed), so the local
            // build below sees the same dispatchers.
            publish_extension_socket(handler.clone());

            build_extension_daw_with(handler).await
        })
        .map_err(|e| eyre::eyre!("Failed to initialise in-process DAW: {e}"))
}

/// Register all actions synchronously on the main thread.
///
/// The registry service in `daw-reaper` owns the REAPER command IDs and
/// toggle state. We register through that service here, then forward action
/// trigger events into the existing dispatch channel.
fn register_actions_sync(
    defs: &actions::ActionDefs,
    modules: Vec<Box<dyn DawModule>>,
    panels: Vec<module::PanelDef>,
) {
    let g = Global::get();
    let runtime = g.tokio_runtime.clone();
    let defs = defs.clone();
    let action_count = defs.len();
    let panel_count = panels.len();
    let runtime_for_module_subscriptions = runtime.clone();
    let task_support = &g.task_support;

    for (command_id, display_name, _handler, show_in_menu, toggleable) in &defs {
        let cmd_id = daw_reaper::action_registry::register_action_main_thread(
            command_id,
            display_name,
            *show_in_menu,
            *toggleable,
        );

        if cmd_id > 0 {
            info!(command_id = %command_id, cmd_id, "Registered action");
        } else {
            warn!("Failed to register action: {command_id}");
        }
    }
    info!(actions = action_count, "Action registration completed");

    #[cfg(feature = "mod-input")]
    for (command_id, _, _, _, _) in &defs {
        if matches!(
            command_id.as_str(),
            "FTS_INPUT_TOGGLE"
                | "FTS_INPUT_TOGGLE_PASSTHROUGH"
                | "FTS_INPUT_TOGGLE_DEBUG_LOGGING"
                | "FTS_INPUT_PROFILE_SELECTOR"
                | "FTS_INPUT_WORKFLOW_SELECTOR"
                | "FTS_INPUT_TOGGLE_ACTIONS_PANEL"
                | "FTS_INPUT_TOGGLE_KEYBOARD_PANEL"
                | "FTS_INPUT_TOGGLE_STATUS_PANEL"
        ) {
            if daw_reaper::Reaper.is_in_action_list(command_id) {
                info!(
                    command_id = %command_id,
                    "Input action list probe: present immediately after registration"
                );
            } else {
                warn!(
                    command_id = %command_id,
                    "Input action list probe: missing immediately after registration"
                );
            }
        }
    }

    #[cfg(feature = "ui-dock")]
    if let Err(err) = task_support.do_later_in_main_thread_asap(move || {
        info!(panels = panels.len(), "Panel definitions collected");
        for panel in &panels {
            daw::ui::dock::register_panel_from_service(panel);
        }
        daw::ui::dock::restore_dock_state();
        info!(panels = panels.len(), "Panel registration completed");
    }) {
        warn!("Failed to schedule panel registration: {err}");
    }
    #[cfg(not(feature = "ui-dock"))]
    let _ = (task_support, panels);

    let module_ctx = ModuleContext::new(runtime_for_module_subscriptions);
    for module in &modules {
        tracing::info!(
            module = module.name(),
            "Subscribing {}",
            module.display_name()
        );
        module.subscribe(&module_ctx);
    }
    info!(modules = modules.len(), "All modules subscribed");
    info!(
        modules = modules.len(),
        actions = action_count,
        panels = panel_count,
        "FTS Extensions ready"
    );

    let (tx, _) = action_channel();
    let tx = tx.clone();
    runtime.spawn(async move {
        let mut rx = daw_reaper::action_registry::subscribe_action_broadcasts();
        info!("Subscribed to action trigger events");
        loop {
            match rx.recv().await {
                Ok(command_name) => {
                    let _ = tx.send(command_name);
                }
                Err(e) => {
                    warn!("Action trigger stream error: {e:?}");
                    break;
                }
            }
        }
    });
}

// ── Entry point ──────────────────────────────────────────────────────────────

#[reaper_extension_plugin]
fn plugin_main(context: PluginContext) -> Result<(), Box<dyn Error>> {
    // Tracing -> rolling daily log file in $XDG_STATE_HOME/fasttrackstudio/
    let log_dir = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
                .join(".local/state")
        })
        .join("fasttrackstudio");
    std::fs::create_dir_all(&log_dir).ok();
    let file_appender = tracing_appender::rolling::daily(&log_dir, "reaper-fts-extensions.log");
    let (non_blocking, log_guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into())
                .add_directive("cranelift_jit=warn".parse()?)
                .add_directive("cranelift_codegen=warn".parse()?)
                .add_directive("wasmtime=warn".parse()?),
        )
        .init();

    info!("FTS Extensions starting…");

    // Kick off the wgpu Instance/Adapter/Device build on a worker thread
    // so the heavy GPU init runs in parallel with the rest of plugin_main
    // and with REAPER's own startup. No window/surface is touched here,
    // so this is safe to call before REAPER's main loop is running.
    #[cfg(feature = "mod-input")]
    reaper_input::prewarm_gpu_in_background();

    // Low-level REAPER and SWELL APIs must be available globally before
    // any Reaper::get() / Swell::get() calls (needed for menus, panels, etc.)
    let _ = reaper_low::Reaper::make_available_globally(reaper_low::Reaper::load(context));
    let _ = reaper_low::Swell::make_available_globally(reaper_low::Swell::load(context));

    daw_reaper::set_plugin_context(context);

    match HighReaper::load(context).setup() {
        Ok(_) => {
            info!("REAPER high-level API loaded");
            if let Err(e) = HighReaper::get().wake_up() {
                tracing::debug!("reaper wake_up: {e}");
            }
        }
        Err(_) => tracing::debug!("REAPER high-level API already loaded"),
    }

    // Seed the mode toolbar names in `reaper-menu.ini` so REAPER's
    // toolbar list shows `Organize 1`, `Write 1`, etc. on next launch.
    // Best-effort: a failure here just logs and continues.
    #[cfg(feature = "mod-session")]
    {
        let resource = HighReaper::get().resource_path();
        match mode_toolbars::rename_for_modes(resource.as_std_path()) {
            Ok(true) => info!(
                resource = %resource,
                "Mode toolbars renamed in reaper-menu.ini (restart REAPER to see them)"
            ),
            Ok(false) => tracing::debug!("Mode toolbars already named correctly"),
            Err(e) => warn!("Failed to rename mode toolbars: {e}"),
        }
    }

    let tokio_runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?,
    );

    let (task_sender, task_receiver) = crossbeam_channel::unbounded();
    let task_support = TaskSupport::new(task_sender.clone());

    let daw = initialize_daw(&tokio_runtime)?;

    GLOBAL
        .set(Global {
            task_support,
            task_sender: task_sender.clone(),
            task_receiver: task_receiver.clone(),
            daw: daw.clone(),
            tokio_runtime,
            _log_guard: log_guard,
        })
        .map_err(|_| "Global already set")?;

    let g = Global::get();
    let _ = daw::init_from_parts(g.daw.clone(), g.tokio_runtime.clone());
    daw_reaper::set_task_support(&g.task_support);

    // Spin up the per-domain push broadcasters (item / tempo / fx /
    // routing / take). Required for any external subscriber to receive
    // events; safe to call before module subscriptions hook in.
    daw_reaper::extension_setup::init_surviving_broadcasters();

    let task_middleware = MainTaskMiddleware::new(g.task_sender.clone(), g.task_receiver.clone());

    // ── Collect modules ──────────────────────────────────────────────────
    // Each library implements daw::DawModule and exports module(). Modules
    // are gated by per-module cargo features so we can bisect startup cost.
    let modules: Vec<Box<dyn DawModule>> = vec![
        #[cfg(feature = "mod-launcher")]
        fts_launcher::daw_module::module(),
        #[cfg(feature = "mod-session")]
        session::daw_module::module_with_daw(daw_reaper::Reaper),
        #[cfg(feature = "mod-sync")]
        daw_synchronization::daw_module::module(),
        #[cfg(feature = "mod-input")]
        reaper_input::daw_module::module(),
    ];
    let module_count = modules.len();

    // Initialize all modules before collecting actions.
    //
    // Several module actions derive toggle state or dynamic bindings from
    // configuration loaded during init, so registering them before init can
    // produce incomplete or stale action metadata.
    let module_ctx = ModuleContext::new(g.tokio_runtime.clone());
    for module in &modules {
        info!(
            module = module.name(),
            "Initializing {}",
            module.display_name()
        );
        module.init(&module_ctx);
    }
    info!(modules = module_count, "All modules initialized");

    // Restore the persisted session mode before installing the input
    // bridge. `set_mode` applies the window layout, toolbars, and fires
    // listeners — so doing this first means the bridge's initial
    // workflow activation (run inside `install`) already sees the
    // correct mode and there's no flash of the default Organize mode.
    #[cfg(feature = "mod-session")]
    {
        if let Some(mode) = session::mode_actions::restore_persisted_mode() {
            info!(mode = %mode, "Restored persisted session mode");
        } else {
            info!("No persisted session mode — staying on default");
        }
    }

    // Bridge session mode changes to reaper-input workflows. Only wired
    // when both modules are compiled in; otherwise this is a no-op build.
    #[cfg(all(feature = "mod-session", feature = "mod-input"))]
    {
        mode_input::install();
        info!("Mode → input workflow bridge installed");
    }

    // Collect actions from all modules after init has populated runtime state.
    let module_actions = module::collect_actions(&modules);

    // Also collect legacy (non-module) actions from this crate
    let legacy_defs = actions::build_action_defs();

    // Merge all actions
    let mut all_actions: HashMap<String, Arc<dyn Fn() + Send + Sync>> = HashMap::new();
    for (id, _, handler, _, _) in &legacy_defs {
        all_actions.insert(id.clone(), handler.clone());
    }
    for (id, _, handler, _, _) in &module_actions {
        all_actions.insert(id.clone(), handler.clone());
    }
    #[cfg(feature = "ui-dock")]
    for action in ui_test_panel::action_defs() {
        let (id, _, handler, _, _) = action.into_tuple();
        all_actions.insert(id, handler);
    }

    info!(
        legacy = legacy_defs.len(),
        modules = module_actions.len(),
        total = all_actions.len(),
        "Action definitions collected"
    );

    // Combine all defs for REAPER registration
    let mut all_defs: actions::ActionDefs = legacy_defs;
    all_defs.extend(module_actions.into_iter().map(
        |(id, display_name, handler, show_in_menu, toggleable)| {
            (id, display_name, handler, show_in_menu, toggleable)
        },
    ));
    #[cfg(feature = "ui-dock")]
    all_defs.extend(
        ui_test_panel::action_defs()
            .into_iter()
            .map(|a| a.into_tuple()),
    );

    #[allow(unused_mut)]
    let mut panels = module::collect_panels(&modules);
    #[cfg(feature = "ui-dock")]
    panels.extend(ui_test_panel::panel_defs());

    let session = ReaperSession::load(context);
    let app = App {
        session: RefCell::new(session),
        task_middleware: RefCell::new(task_middleware),
        action_handlers: RefCell::new(all_actions),
    };

    APP.set(Fragile::new(app)).map_err(|_| "App already set")?;

    // Install the late-registration callback so reaper-input can push
    // new actions through to REAPER + the dispatch map whenever it
    // discovers a new workflow `.styx` file at runtime.
    #[cfg(feature = "mod-input")]
    reaper_input::infrastructure::action_registry::set_action_registrar(Box::new(|def| {
        let Some(app_fragile) = APP.get() else {
            tracing::warn!(
                command_id = %def.command_id,
                "Late-action fired before App initialised; ignoring"
            );
            return;
        };
        app_fragile.get().register_late_action(def);
    }));

    #[cfg(feature = "ui-dock")]
    {
        daw::ui::dock::init_service();
        daw::ui::dock::init_dock(reaper_low::Reaper::get(), reaper_low::Swell::get());
    }

    register_actions_sync(&all_defs, modules, panels);

    let app = APP.get().unwrap().get();
    let mut session = app.session.borrow_mut();
    session.plugin_register_add_timer(timer_callback)?;
    #[cfg(feature = "host-hooks")]
    daw_reaper::register_project_importer(&mut session)?;

    // Register the FTS DAW control surface (honors FTS_CSURF_MODE +
    // FTS_CSURF_DISABLED env vars) so external read-only subscribers
    // — web UIs, MIDI/OSC surfaces — get the same callback stream the
    // bridge plugin used to provide.
    daw_reaper::extension_setup::register_control_surface(&mut session);

    // Register the per-buffer audio-thread snapshot hooks (single +
    // multi-project). Required for any clock-sync / drift-correction
    // consumer downstream; safe to leave in place even when no peer
    // discovery has been configured.
    let audio_sync_handle = daw_reaper::extension_setup::init_audio_sync(&mut session);
    let _ = AUDIO_SYNC.set(audio_sync_handle.clone());

    drop(session);

    // ClockSync — env-gated UDP peer discovery + offset estimation.
    // Opt in with `FTS_AUDIO_SYNC_PORT=<u16>` (and optionally
    // `FTS_AUDIO_SYNC_DRIFT=1`); off entirely otherwise. Uses the
    // host tokio runtime + task_support so the drift corrector can
    // call back into REAPER from the main thread.
    if let Some(handle) = audio_sync_handle.as_ref() {
        // Read persisted on/off state from REAPER ExtState. Defaults
        // ON for clock-sync (multicast peer discovery + position
        // broadcast — passive on the wire when no peers, safe to
        // leave on) and OFF for drift correction (auto-rate-changes
        // are intrusive; opt in deliberately).
        let cs_enabled = sync_settings::clock_sync_enabled();
        let drift_enabled = sync_settings::drift_enabled();
        if cs_enabled {
            let g = Global::get();
            // Use ClockSync's default port (7777). User can still
            // override per-run via FTS_AUDIO_SYNC_PORT env var if a
            // collision happens.
            const CLOCK_SYNC_PORT: u16 = 7777;
            daw_reaper::extension_setup::init_clock_sync(
                g.tokio_runtime.handle().clone(),
                handle.cell.clone(),
                CLOCK_SYNC_PORT,
                drift_enabled,
                |rate: f64| {
                    use reaper_medium::PlaybackSpeedFactor;
                    let ts = &Global::get().task_support;
                    let _ = ts.do_later_in_main_thread_asap(move || {
                        let reaper = reaper_high::Reaper::get();
                        let speed = PlaybackSpeedFactor::new(rate);
                        reaper.medium_reaper().csurf_on_play_rate_change(speed);
                    });
                },
            );
        } else {
            info!("Clock-sync disabled by persisted ExtState (toggle with FTS_CLOCK_SYNC_TOGGLE)");
        }
    }

    // Register the FTS_WINDOW_* nudge / grow actions outside the
    // session borrow — they take their own borrow internally for the
    // gaccel registration.
    daw_reaper::extension_setup::register_window_geometry_actions();

    // Register the cross-DAW project importer so REAPER's File > Open
    // (and Finder double-click, once the Info.plist association is in
    // place) accept .ptx / .als / .aaf / .dawproject and route them
    // through daw_reaper::project_import::* into a fresh project.
    match daw_reaper::register_project_importer(&mut HighReaper::get().medium_session()) {
        Ok(()) => info!("Project importer registered (.ptx, .als, .aaf, .dawproject)"),
        Err(e) => warn!("Project importer registration FAILED: {:?}", e),
    }

    #[cfg(feature = "host-hooks")]
    {
        // ── Extensions → FastTrackStudio menu ────────────────────────────
        // Collect menu entries from all action defs with show_in_menu=true.
        let menu_entries: Vec<(String, String)> = all_defs
            .iter()
            .filter(|(_, _, _, show_in_menu, _)| *show_in_menu)
            .map(|(id, display_name, _, _, _)| (id.clone(), display_name.clone()))
            .collect();
        info!(menu_entries = menu_entries.len(), "Menu entries collected");
        menu::set_menu_entries(menu_entries);

        info!("Registering Extensions menu hook...");
        HighReaper::get().medium_reaper().add_extensions_main_menu();
        match HighReaper::get()
            .medium_session()
            .plugin_register_add_hook_custom_menu::<menu::FtsMenuHook>()
        {
            Ok(()) => info!("Extensions menu hook registered successfully"),
            Err(e) => warn!("Extensions menu hook registration FAILED: {:?}", e),
        }
    }
    #[cfg(not(feature = "host-hooks"))]
    let _ = &all_defs;

    info!(modules = module_count, "FTS Extensions startup scheduled");
    Ok(())
}
