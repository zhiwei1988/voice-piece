use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU16, AtomicU32, Ordering};
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use koe_core::ffi::SPSessionMode;

pub const WM_APP_HOTKEY_HOLD_START: u32 = WM_APP + 50;
pub const WM_APP_HOTKEY_HOLD_END: u32 = WM_APP + 51;
pub const WM_APP_HOTKEY_TAP_START: u32 = WM_APP + 52;
pub const WM_APP_HOTKEY_TAP_END: u32 = WM_APP + 53;
pub const WM_APP_HOTKEY_CANCEL: u32 = WM_APP + 54;

const HOLD_TIMER_ID: usize = 300;
const HOLD_THRESHOLD_MS: u32 = 500;
const TRAILING_AUDIO_TIMER_ID: usize = 301;
const TRAILING_AUDIO_MS: u32 = 300;

static HOOK: AtomicIsize = AtomicIsize::new(0);
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static TRIGGER_VK: AtomicU16 = AtomicU16::new(0);
static CANCEL_VK: AtomicU16 = AtomicU16::new(0);
static TRIGGER_DOWN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HotkeyState {
    Idle,
    Pending,
    RecordingHold,
    RecordingToggle,
}

static STATE: AtomicU32 = AtomicU32::new(0);

fn get_state() -> HotkeyState {
    match STATE.load(Ordering::SeqCst) {
        1 => HotkeyState::Pending,
        2 => HotkeyState::RecordingHold,
        3 => HotkeyState::RecordingToggle,
        _ => HotkeyState::Idle,
    }
}

fn set_state(s: HotkeyState) {
    STATE.store(s as u32, Ordering::SeqCst);
}

fn load_hwnd() -> HWND {
    HWND(MAIN_HWND.load(Ordering::Relaxed) as *mut _)
}

pub fn init(hwnd: HWND) {
    MAIN_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

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
        let hwnd = load_hwnd();

        let is_key_down = wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize;
        let is_key_up = wparam.0 == WM_KEYUP as usize || wparam.0 == WM_SYSKEYUP as usize;

        if vk == trigger {
            let state = get_state();
            if is_key_down {
                let was_down = TRIGGER_DOWN.swap(true, Ordering::SeqCst);
                if !was_down && state == HotkeyState::Idle {
                    set_state(HotkeyState::Pending);
                    SetTimer(hwnd, HOLD_TIMER_ID, HOLD_THRESHOLD_MS, None);
                } else if state == HotkeyState::RecordingToggle {
                    set_state(HotkeyState::Idle);
                    let _ = PostMessageW(hwnd, WM_APP_HOTKEY_TAP_END, WPARAM(0), LPARAM(0));
                }
            } else if is_key_up {
                TRIGGER_DOWN.store(false, Ordering::SeqCst);
                if state == HotkeyState::Pending {
                    let _ = KillTimer(hwnd, HOLD_TIMER_ID);
                    set_state(HotkeyState::RecordingToggle);
                    let _ = PostMessageW(hwnd, WM_APP_HOTKEY_TAP_START, WPARAM(0), LPARAM(0));
                } else if state == HotkeyState::RecordingHold {
                    set_state(HotkeyState::Idle);
                    let _ = PostMessageW(hwnd, WM_APP_HOTKEY_HOLD_END, WPARAM(0), LPARAM(0));
                }
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

pub fn handle_timer(hwnd: HWND, wparam: WPARAM) {
    match wparam.0 {
        HOLD_TIMER_ID => {
            unsafe { let _ = KillTimer(hwnd, HOLD_TIMER_ID); }
            if get_state() == HotkeyState::Pending {
                set_state(HotkeyState::RecordingHold);
                handle_message(hwnd, WM_APP_HOTKEY_HOLD_START);
            }
        }
        TRAILING_AUDIO_TIMER_ID => {
            unsafe { let _ = KillTimer(hwnd, TRAILING_AUDIO_TIMER_ID); }
            crate::audio::stop_capture();
            crate::bridge::end_session();
        }
        100 => {
            unsafe { let _ = KillTimer(hwnd, 100); }
            crate::paste::simulate_paste();
            unsafe { SetTimer(hwnd, 101, 1500, None); }
        }
        101 => {
            unsafe { let _ = KillTimer(hwnd, 101); }
            crate::clipboard::restore();
            crate::overlay::update_state("idle");
            crate::tray::update_tooltip("Koe - Idle");
        }
        200 => {
            unsafe { let _ = KillTimer(hwnd, 200); }
            crate::overlay::update_state("idle");
            crate::tray::update_tooltip("Koe - Idle");
        }
        _ => {}
    }
}

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
            unsafe { SetTimer(hwnd, TRAILING_AUDIO_TIMER_ID, TRAILING_AUDIO_MS, None); }
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
            unsafe { SetTimer(hwnd, TRAILING_AUDIO_TIMER_ID, TRAILING_AUDIO_MS, None); }
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
