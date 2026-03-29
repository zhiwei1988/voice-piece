use windows::Win32::Foundation::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

use crate::bridge::WM_APP_TRAY;

const TRAY_ICON_ID: u32 = 1;

// Menu item IDs
const IDM_STATUS: u32 = 1000;
const IDM_OPEN_CONFIG: u32 = 1001;
const IDM_RELOAD_CONFIG: u32 = 1002;
const IDM_QUIT: u32 = 1003;

static mut TRAY_DATA: Option<NOTIFYICONDATAW> = None;

pub fn init(hwnd: HWND) {
    unsafe {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NOTIFY_ICON_DATA_FLAGS(NIF_ICON.0 | NIF_MESSAGE.0 | NIF_TIP.0),
            uCallbackMessage: WM_APP_TRAY,
            hIcon: LoadIconW(None, IDI_APPLICATION).unwrap(),
            ..Default::default()
        };
        set_tooltip(&mut nid, "Koe - Idle");
        Shell_NotifyIconW(NIM_ADD, &nid);
        TRAY_DATA = Some(nid);
    }
}

pub fn cleanup(hwnd: HWND) {
    unsafe {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            ..Default::default()
        };
        Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

pub fn update_tooltip(text: &str) {
    unsafe {
        if let Some(ref mut nid) = TRAY_DATA {
            set_tooltip(nid, text);
            Shell_NotifyIconW(NIM_MODIFY, nid);
        }
    }
}

fn set_tooltip(nid: &mut NOTIFYICONDATAW, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let len = wide.len().min(nid.szTip.len());
    nid.szTip[..len].copy_from_slice(&wide[..len]);
    if len < nid.szTip.len() {
        nid.szTip[len - 1] = 0; // null terminate
    }
}

pub fn handle_message(hwnd: HWND, _msg: u32, _wparam: WPARAM, lparam: LPARAM) {
    let event = (lparam.0 & 0xFFFF) as u32;
    match event {
        WM_RBUTTONUP | WM_CONTEXTMENU => {
            show_context_menu(hwnd);
        }
        _ => {}
    }
}

fn show_context_menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu().unwrap();

        // Status (disabled)
        let status = w!("Koe - Voice Input");
        AppendMenuW(menu, MF_STRING | MF_GRAYED, IDM_STATUS as usize, status).unwrap();

        AppendMenuW(menu, MF_SEPARATOR, 0, None).unwrap();

        AppendMenuW(menu, MF_STRING, IDM_OPEN_CONFIG as usize, w!("Open Config Folder")).unwrap();
        AppendMenuW(menu, MF_STRING, IDM_RELOAD_CONFIG as usize, w!("Reload Config")).unwrap();

        AppendMenuW(menu, MF_SEPARATOR, 0, None).unwrap();

        AppendMenuW(menu, MF_STRING, IDM_QUIT as usize, w!("Quit")).unwrap();

        // Required for the menu to dismiss when clicking outside
        SetForegroundWindow(hwnd);

        let mut pt = POINT::default();
        GetCursorPos(&mut pt).unwrap();

        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            0,
            hwnd,
            None,
        );

        let _ = DestroyMenu(menu);

        if cmd.as_bool() {
            handle_menu_command(cmd.0 as u32);
        }
    }
}

fn handle_menu_command(cmd: u32) {
    match cmd {
        IDM_OPEN_CONFIG => {
            let config_dir = koe_core::config::config_dir();
            let dir_str = config_dir.to_string_lossy().to_string();
            let wide: Vec<u16> = dir_str.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                windows::Win32::UI::Shell::ShellExecuteW(
                    None,
                    w!("explore"),
                    PCWSTR(wide.as_ptr()),
                    None,
                    None,
                    windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                );
            }
        }
        IDM_RELOAD_CONFIG => {
            koe_core::sp_core_reload_config();
            log::info!("config reloaded");
        }
        IDM_QUIT => {
            unsafe {
                PostQuitMessage(0);
            }
        }
        _ => {}
    }
}
