//! Continuous Action Framework
//!
//! Provides support for "perform until shortcut released" style actions.
//! Based on SWS BR_ContinuousActions.cpp pattern.
//!
//! Usage:
//! 1. Register a continuous action with `register_continuous_action()`
//! 2. When the action is triggered, call `start_continuous_action(command_id)`
//! 3. The action callback will be called repeatedly until key is released
//! 4. Key release is detected via pre-hook accelerator (like SWS on macOS)

use crate::error::{lock_mut, lock_read};
use reaper_high::Reaper;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tracing::{debug, info, warn};

/// Special flag value passed to action during timer execution (matches SWS ACTION_FLAG)
pub const ACTION_FLAG: i32 = -666;

/// Windows message constants
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYUP: u32 = 0x0105;

/// Continuous action definition
pub struct ContinuousAction {
    /// Unique command ID for this action
    pub command_id: &'static str,

    /// Initialize/cleanup callback
    /// Called with `true` when action starts, `false` when it ends
    /// Returns `true` if initialization succeeded
    pub init: fn(init: bool) -> bool,

    /// Main action callback - called repeatedly while key is held
    pub action: fn(),

    /// Undo callback - called when action ends
    /// Returns undo flags for REAPER's undo system
    pub do_undo: fn() -> u32,
}

/// State for an active continuous action
struct ActiveAction {
    command_id: &'static str,
    action: fn(),
    do_undo: fn() -> u32,
}

/// Global registry of continuous actions
static REGISTERED_ACTIONS: OnceLock<Mutex<HashMap<&'static str, ContinuousAction>>> =
    OnceLock::new();

/// Currently active continuous action (if any)
static ACTIVE_ACTION: OnceLock<Mutex<Option<ActiveAction>>> = OnceLock::new();

/// Whether the timer is currently registered
static TIMER_REGISTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the accelerator is currently registered
static ACCEL_REGISTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn get_registry() -> &'static Mutex<HashMap<&'static str, ContinuousAction>> {
    REGISTERED_ACTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_active() -> &'static Mutex<Option<ActiveAction>> {
    ACTIVE_ACTION.get_or_init(|| Mutex::new(None))
}

/// Register a continuous action
///
/// Call this during extension initialization to register actions that support
/// "perform until shortcut released" behavior.
pub fn register_continuous_action(action: ContinuousAction) {
    let command_id = action.command_id;
    lock_mut(get_registry(), |registry| {
        registry.insert(action.command_id, action);
    });
    info!("Registering continuous action: {}", command_id);
}

/// Start a continuous action
///
/// Called when the action's shortcut is pressed. This:
/// 1. Checks if this action is already running (if so, returns true - key repeat)
/// 2. Stops any different running continuous action
/// 3. Calls the action's init callback with `true`
/// 4. Registers a timer and pre-hook accelerator to repeatedly call the action
///
/// Returns `true` if the action was started successfully
pub fn start_continuous_action(command_id: &str) -> bool {
    // Check if this same action is already running (key repeat - just ignore)
    {
        let active = get_active().lock().unwrap();
        if let Some(ref active_action) = *active {
            if active_action.command_id == command_id {
                debug!("Ignoring key repeat for: {}", command_id);
                return true; // Already running, don't restart
            }
        }
    }

    // Stop any different action that might be running
    stop_all_continuous_actions();

    let registry = get_registry().lock().unwrap();
    let Some(action) = registry.get(command_id) else {
        warn!("Continuous action not found: {}", command_id);
        return false;
    };

    // Call init callback
    if !(action.init)(true) {
        warn!("Continuous action init failed: {}", command_id);
        return false;
    }

    // Store active action state
    {
        let mut active = get_active().lock().unwrap();
        *active = Some(ActiveAction {
            command_id: action.command_id,
            action: action.action,
            do_undo: action.do_undo,
        });
    }

    // Register pre-hook accelerator for key release detection
    if !ACCEL_REGISTERED.load(std::sync::atomic::Ordering::Relaxed) {
        register_accelerator();
        ACCEL_REGISTERED.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    // Register timer if not already registered
    if !TIMER_REGISTERED.load(std::sync::atomic::Ordering::Relaxed) {
        if let Err(e) = register_timer() {
            warn!("Failed to register continuous action timer: {}", e);
            // Clean up
            deregister_accelerator();
            ACCEL_REGISTERED.store(false, std::sync::atomic::Ordering::Relaxed);

            let mut active = get_active().lock().unwrap();
            if let Some(ref active_action) = *active {
                // Find and call init(false) to clean up
                if let Some(action) = registry.get(active_action.command_id) {
                    (action.init)(false);
                }
            }
            *active = None;
            return false;
        }
        TIMER_REGISTERED.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    info!("Started continuous action: {}", command_id);
    true
}

/// Stop all continuous actions
///
/// Called when the key is released (detected via pre-hook accelerator).
/// This:
/// 1. Calls the action's do_undo callback to create an undo point
/// 2. Calls the action's init callback with `false` to clean up
/// 3. Deregisters the timer and accelerator
pub fn stop_all_continuous_actions() {
    let mut active = get_active().lock().unwrap();

    if let Some(ref active_action) = *active {
        debug!("Stopping continuous action: {}", active_action.command_id);

        // Call do_undo to get undo flags and create undo point
        let undo_flags = (active_action.do_undo)();
        if undo_flags != 0 {
            debug!(
                "Continuous action undo flags: {} for {}",
                undo_flags, active_action.command_id
            );
        }

        // Call init(false) to clean up
        let registry = get_registry().lock().unwrap();
        if let Some(action) = registry.get(active_action.command_id) {
            (action.init)(false);
        }
    }

    *active = None;

    // Deregister accelerator
    if ACCEL_REGISTERED.load(std::sync::atomic::Ordering::Relaxed) {
        deregister_accelerator();
        ACCEL_REGISTERED.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    // Deregister timer
    if TIMER_REGISTERED.load(std::sync::atomic::Ordering::Relaxed) {
        deregister_timer();
        TIMER_REGISTERED.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Check if a continuous action is currently active
pub fn is_action_active() -> bool {
    lock_read(get_active(), |active| active.is_some()).unwrap_or(false)
}

/// Get the command ID of the currently active action (if any)
pub fn get_active_command_id() -> Option<&'static str> {
    lock_read(get_active(), |active| active.as_ref().map(|a| a.command_id)).flatten()
}

/// Timer callback - called repeatedly while action is active
///
/// This is the extern "C" function registered with REAPER's timer system.
extern "C" fn continuous_action_timer() {
    // Execute the action - get the function pointer while holding the lock
    let action_fn = lock_read(get_active(), |active| active.as_ref().map(|a| a.action));

    // Call the action outside the lock
    if let Some(Some(action)) = action_fn {
        action();
    }
}

/// Accelerator callback for key release detection
///
/// This is registered as a pre-hook accelerator ("<accelerator") with REAPER.
/// It intercepts WM_KEYUP messages before normal accelerator processing.
/// Based on SWS BR_ContinuousActions.cpp TranslateAccel().
extern "C" fn translate_accel(
    msg: *mut reaper_low::raw::MSG,
    _ctx: *mut reaper_low::raw::accelerator_register_t,
) -> std::ffi::c_int {
    if msg.is_null() {
        return 0;
    }

    let message = unsafe { (*msg).message };

    // Check for key release
    if message == WM_KEYUP || message == WM_SYSKEYUP {
        debug!("Key release detected - stopping continuous action");
        // We can't call stop_all_continuous_actions directly here due to lock ordering
        // Instead, set a flag that the timer will check
        // Actually, SWS does call it directly, let's try
        stop_all_continuous_actions();
    }

    // Return 0 to let normal accelerator processing continue
    0
}

/// Static accelerator register struct
/// Must be static because REAPER holds a pointer to it
static mut ACCELERATOR_REGISTER: reaper_low::raw::accelerator_register_t =
    reaper_low::raw::accelerator_register_t {
        translateAccel: None,
        isLocal: true, // true = pre-hook, receives events before main window
        user: std::ptr::null_mut(),
    };

/// Register the pre-hook accelerator with REAPER
fn register_accelerator() {
    let reaper = Reaper::get();

    unsafe {
        // Set up the accelerator register struct
        ACCELERATOR_REGISTER.translateAccel = Some(translate_accel);
        ACCELERATOR_REGISTER.isLocal = true;

        // Register as pre-hook accelerator using "<accelerator" (like SWS on macOS)
        reaper.medium_reaper().low().plugin_register(
            c"<accelerator".as_ptr(),
            std::ptr::addr_of_mut!(ACCELERATOR_REGISTER).cast(),
        );
    }
    debug!("Registered pre-hook accelerator for key release detection");
}

/// Deregister the pre-hook accelerator from REAPER
fn deregister_accelerator() {
    let reaper = Reaper::get();

    unsafe {
        // Deregister using "-accelerator" (not "-<accelerator")
        reaper.medium_reaper().low().plugin_register(
            c"-accelerator".as_ptr(),
            std::ptr::addr_of_mut!(ACCELERATOR_REGISTER).cast(),
        );
    }
    debug!("Deregistered pre-hook accelerator");
}

/// Register the timer with REAPER
fn register_timer() -> Result<(), Box<dyn std::error::Error>> {
    let reaper = Reaper::get();
    let mut session = reaper.medium_session();

    session.plugin_register_add_timer(continuous_action_timer)?;
    debug!("Registered continuous action timer");
    Ok(())
}

/// Deregister the timer from REAPER
fn deregister_timer() {
    let reaper = Reaper::get();

    // Use low-level API to deregister timer
    // plugin_register("-timer", callback) deregisters
    unsafe {
        reaper
            .medium_reaper()
            .low()
            .plugin_register(c"-timer".as_ptr(), continuous_action_timer as *mut _);
    }
    debug!("Deregistered continuous action timer");
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test helper - mock init callback
    fn mock_init(_init: bool) -> bool {
        true
    }

    // Test helper - mock action callback
    fn mock_action() {}

    // Test helper - mock undo callback
    fn mock_undo() -> u32 {
        0
    }

    #[test]
    fn test_action_registration() {
        let action = ContinuousAction {
            command_id: "TEST_ACTION",
            init: mock_init,
            action: mock_action,
            do_undo: mock_undo,
        };

        register_continuous_action(action);

        let registry = get_registry().lock().unwrap();
        assert!(registry.contains_key("TEST_ACTION"));
    }
}
