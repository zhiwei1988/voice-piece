use std::sync::atomic::{AtomicIsize, AtomicU16, AtomicU32, Ordering};
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use koe_core::ffi::SPSessionMode;

// Custom messages posted from the hook callback to the main thread
pub const WM_APP_HOTKEY_HOLD_START: u32 = WM_APP + 50;
pub const WM_APP_HOTKEY_HOLD_END: u32 = WM_APP + 51;
pub const WM_APP_HOTKEY_TAP_START: u32 = WM_APP + 52;
pub const WM_APP_HOTKEY_TAP_END: u32 = WM_APP + 53;
pub const WM_APP_HOTKEY_CANCEL: u32 = WM_APP + 54;

const HOLD_TIMER_ID: usize = 300;
const HOLD_THRESHOLD_MS: u32 = 180;
const TRAILING_AUDIO_TIMER_ID: usize = 301;
const TRAILING_AUDIO_MS: u32 = 300;

// Global state for the keyboard hook
static HOOK: AtomicIsize = AtomicIsize::new(0);
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static TRIGGER_VK: AtomicU16 = AtomicU16::new(0);
static CANCEL_VK: AtomicU16 = AtomicU16::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HotkeyState {
    Idle,
    Pending,         // trigger key down, waiting for hold threshold
    RecordingHold,   // hold confirmed
    RecordingToggle, // tap confirmed, recording
}

static STATE: AtomicU32 = AtomicU32::new(0); // HotkeyState encoded as u32

fn get_state() -> HotkeyState {
    match STATE.load(Ordering::SeqCst) {
        0 => HotkeyState::Idle,
        1 => HotkeyState::Pending,
        2 => HotkeyState::RecordingHold,
        3 => HotkeyState::RecordingToggle,
        _ => HotkeyState::Idle,
    }
}

fn set_state(s: HotkeyState) {
    STATE.store(s as u32, Ordering::SeqCst);
}

pub fn init(hwnd: HWND) {
    MAIN_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

    // Read hotkey config from koe-core
    let config = koe_core::sp_core_get_hotkey_config();
    TRIGGER_VK.store(config.trigger_key_code, Ordering::Relaxed);
    CANCEL_VK.store(config.cancel_key_code, Ordering::Relaxed);

    log::info!(
        "hotkey init: trigger=0x{:X}, cancel=0x{:X}",
        config.trigger_key_code,
        config.cancel_key_code
    );

    unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0)
            .expect("failed to install keyboard hook");
        HOOK.store(hook.0 as isize, Ordering::Relaxed);
    }
}

pub fn cleanup() {
    let hook_val = HOOK.swap(0, Ordering::Relaxed);
    if hook_val != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(hook_val as *mut _));
        }
    }
}

/// Low-level keyboard hook callback.
/// Runs on the main thread (which has the message loop).
/// Must return quickly — only update atomic state and PostMessage.
unsafe extern "system" fn keyboard_hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code >= 0 {
        let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = kbd.vkCode as u16;
        let trigger = TRIGGER_VK.load(Ordering::Relaxed);
        let cancel = CANCEL_VK.load(Ordering::Relaxed);
        let hwnd = HWND(MAIN_HWND.load(Ordering::Relaxed) as *mut _);

        let is_key_down = wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize;
        let is_key_up = wparam.0 == WM_KEYUP as usize || wparam.0 == WM_SYSKEYUP as usize;

        if vk == trigger {
            let state = get_state();
            if is_key_down && state == HotkeyState::Idle {
                set_state(HotkeyState::Pending);
                // Start hold threshold timer
                SetTimer(Some(hwnd), HOLD_TIMER_ID, HOLD_THRESHOLD_MS, None);
            } else if is_key_up && state == HotkeyState::Pending {
                // Key released before threshold — tap mode
                KillTimer(Some(hwnd), HOLD_TIMER_ID).ok();
                set_state(HotkeyState::RecordingToggle);
                let _ = PostMessageW(hwnd, WM_APP_HOTKEY_TAP_START, WPARAM(0), LPARAM(0));
            } else if is_key_up && state == HotkeyState::RecordingHold {
                // Hold ended
                set_state(HotkeyState::Idle);
                let _ = PostMessageW(hwnd, WM_APP_HOTKEY_HOLD_END, WPARAM(0), LPARAM(0));
            } else if is_key_down && state == HotkeyState::RecordingToggle {
                // Second tap — end toggle recording
                set_state(HotkeyState::Idle);
                let _ = PostMessageW(hwnd, WM_APP_HOTKEY_TAP_END, WPARAM(0), LPARAM(0));
            }
        } else if vk == cancel && is_key_down {
            let state = get_state();
            if state == HotkeyState::RecordingHold || state == HotkeyState::RecordingToggle {
                set_state(HotkeyState::Idle);
                let _ = PostMessageW(hwnd, WM_APP_HOTKEY_CANCEL, WPARAM(0), LPARAM(0));
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

/// Handle the hold threshold timer firing.
pub fn handle_timer(hwnd: HWND, wparam: WPARAM) {
    match wparam.0 {
        HOLD_TIMER_ID => {
            unsafe { KillTimer(Some(hwnd), HOLD_TIMER_ID).ok(); }
            if get_state() == HotkeyState::Pending {
                set_state(HotkeyState::RecordingHold);
                handle_message(hwnd, WM_APP_HOTKEY_HOLD_START);
            }
        }
        TRAILING_AUDIO_TIMER_ID => {
            // Trailing audio capture period ended
            unsafe { KillTimer(Some(hwnd), TRAILING_AUDIO_TIMER_ID).ok(); }
            crate::audio::stop_capture();
            crate::bridge::end_session();
        }
        100 => {
            // Paste timer (from bridge final_text handler)
            unsafe { KillTimer(Some(hwnd), 100).ok(); }
            crate::paste::simulate_paste();
            // Schedule clipboard restore
            unsafe { SetTimer(Some(hwnd), 101, 1500, None); }
        }
        101 => {
            // Clipboard restore timer
            unsafe { KillTimer(Some(hwnd), 101).ok(); }
            crate::clipboard::restore();
            crate::overlay::update_state("idle");
            crate::tray::update_tooltip("Koe - Idle");
        }
        200 => {
            // Error display timeout — back to idle
            unsafe { KillTimer(Some(hwnd), 200).ok(); }
            crate::overlay::update_state("idle");
            crate::tray::update_tooltip("Koe - Idle");
        }
        _ => {}
    }
}

/// Handle hotkey events posted from the hook callback.
pub fn handle_message(hwnd: HWND, msg: u32) {
    match msg {
        WM_APP_HOTKEY_HOLD_START => {
            log::info!("hold start");
            crate::overlay::update_state("recording");
            crate::tray::update_tooltip("Koe - Recording...");
            crate::bridge::begin_session(SPSessionMode::Hold);
            crate::audio::start_capture();
        }
        WM_APP_HOTKEY_HOLD_END => {
            log::info!("hold end");
            // Delay to capture trailing speech
            unsafe { SetTimer(Some(hwnd), TRAILING_AUDIO_TIMER_ID, TRAILING_AUDIO_MS, None); }
        }
        WM_APP_HOTKEY_TAP_START => {
            log::info!("tap start");
            crate::overlay::update_state("recording");
            crate::tray::update_tooltip("Koe - Recording...");
            crate::bridge::begin_session(SPSessionMode::Toggle);
            crate::audio::start_capture();
        }
        WM_APP_HOTKEY_TAP_END => {
            log::info!("tap end");
            // Delay to capture trailing speech
            unsafe { SetTimer(Some(hwnd), TRAILING_AUDIO_TIMER_ID, TRAILING_AUDIO_MS, None); }
        }
        WM_APP_HOTKEY_CANCEL => {
            log::info!("cancel");
            crate::audio::stop_capture();
            crate::bridge::cancel_session();
            crate::overlay::update_state("idle");
            crate::tray::update_tooltip("Koe - Idle");
        }
        _ => {}
    }
}
