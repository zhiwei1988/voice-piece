use std::ffi::{c_char, CStr};
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use koe_core::ffi::{SPCallbacks, SPSessionContext, SPSessionMode};

pub const WM_APP_SESSION_READY: u32 = WM_APP + 1;
pub const WM_APP_SESSION_ERROR: u32 = WM_APP + 2;
pub const WM_APP_SESSION_WARNING: u32 = WM_APP + 3;
pub const WM_APP_FINAL_TEXT: u32 = WM_APP + 4;
pub const WM_APP_STATE_CHANGED: u32 = WM_APP + 5;
pub const WM_APP_INTERIM_TEXT: u32 = WM_APP + 6;
pub const WM_APP_TRAY: u32 = WM_APP + 100;

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

fn get_hwnd() -> HWND {
    HWND(MAIN_HWND.load(Ordering::Relaxed) as *mut _)
}

fn post_string_message(msg: u32, text: &str) {
    let boxed = Box::new(text.to_string());
    let ptr = Box::into_raw(boxed) as isize;
    unsafe {
        let _ = PostMessageW(get_hwnd(), msg, WPARAM(0), LPARAM(ptr));
    }
}

extern "C" fn on_session_ready() {
    unsafe {
        let _ = PostMessageW(get_hwnd(), WM_APP_SESSION_READY, WPARAM(0), LPARAM(0));
    }
}

extern "C" fn on_session_error(message: *const c_char) {
    let msg = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .to_string();
    post_string_message(WM_APP_SESSION_ERROR, &msg);
}

extern "C" fn on_session_warning(message: *const c_char) {
    let msg = unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .to_string();
    post_string_message(WM_APP_SESSION_WARNING, &msg);
}

extern "C" fn on_final_text_ready(text: *const c_char) {
    let msg = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .to_string();
    post_string_message(WM_APP_FINAL_TEXT, &msg);
}

extern "C" fn on_log_event(level: std::ffi::c_int, message: *const c_char) {
    let msg = unsafe { CStr::from_ptr(message) }
        .to_string_lossy();
    match level {
        0 => log::error!("[core] {msg}"),
        1 => log::warn!("[core] {msg}"),
        2 => log::info!("[core] {msg}"),
        _ => log::debug!("[core] {msg}"),
    }
}

extern "C" fn on_state_changed(state: *const c_char) {
    let msg = unsafe { CStr::from_ptr(state) }
        .to_string_lossy()
        .to_string();
    post_string_message(WM_APP_STATE_CHANGED, &msg);
}

extern "C" fn on_interim_text(text: *const c_char) {
    let msg = unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .to_string();
    post_string_message(WM_APP_INTERIM_TEXT, &msg);
}

pub fn init(hwnd: HWND) {
    MAIN_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

    let callbacks = SPCallbacks {
        on_session_ready: Some(on_session_ready),
        on_session_error: Some(on_session_error),
        on_session_warning: Some(on_session_warning),
        on_final_text_ready: Some(on_final_text_ready),
        on_log_event: Some(on_log_event),
        on_state_changed: Some(on_state_changed),
        on_interim_text: Some(on_interim_text),
    };
    koe_core::sp_core_register_callbacks(callbacks);
}

pub fn begin_session(mode: SPSessionMode) {
    let context = SPSessionContext {
        mode,
        frontmost_bundle_id: std::ptr::null(),
        frontmost_pid: 0,
    };
    koe_core::sp_core_session_begin(context);
}

pub fn push_audio(data: &[u8], timestamp: u64) {
    koe_core::sp_core_push_audio(data.as_ptr(), data.len() as u32, timestamp);
}

pub fn end_session() {
    koe_core::sp_core_session_end();
}

pub fn cancel_session() {
    koe_core::sp_core_session_cancel();
}

fn recover_string(lparam: LPARAM) -> Option<String> {
    let ptr = lparam.0 as *mut String;
    if ptr.is_null() {
        return None;
    }
    Some(*unsafe { Box::from_raw(ptr) })
}

pub fn handle_message(hwnd: HWND, msg: u32, _wparam: WPARAM, lparam: LPARAM) {
    match msg {
        WM_APP_SESSION_READY => {
            log::info!("session ready (ASR connected)");
        }
        WM_APP_SESSION_ERROR => {
            if let Some(text) = recover_string(lparam) {
                log::error!("session error: {text}");
                super::overlay::update_state("error");
                super::tray::update_tooltip("Koe - Error");
                unsafe {
                    SetTimer(hwnd, 200, 2000, None);
                }
            }
        }
        WM_APP_SESSION_WARNING => {
            if let Some(text) = recover_string(lparam) {
                log::warn!("session warning: {text}");
            }
        }
        WM_APP_FINAL_TEXT => {
            if let Some(text) = recover_string(lparam) {
                log::info!("final text: {} chars", text.len());
                super::overlay::update_state("pasting");
                super::tray::update_tooltip("Koe - Pasting");
                super::clipboard::backup();
                super::clipboard::write_text(&text);
                unsafe {
                    SetTimer(hwnd, 100, 50, None);
                }
            }
        }
        WM_APP_STATE_CHANGED => {
            if let Some(state) = recover_string(lparam) {
                log::debug!("state: {state}");
                super::overlay::update_state(&state);
                let tooltip = match state.as_str() {
                    "recording_hold" | "recording_toggle" => "Koe - Recording...",
                    "finalizing_asr" | "correcting" => "Koe - Processing...",
                    "pasting" | "preparing_paste" => "Koe - Pasting",
                    "idle" | "completed" => "Koe - Idle",
                    "failed" => "Koe - Error",
                    _ => "Koe",
                };
                super::tray::update_tooltip(tooltip);
            }
        }
        WM_APP_INTERIM_TEXT => {
            if let Some(text) = recover_string(lparam) {
                super::overlay::update_interim_text(&text);
            }
        }
        _ => {}
    }
}
