#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod bridge;
#[cfg(target_os = "windows")]
mod tray;
#[cfg(target_os = "windows")]
mod hotkey;
#[cfg(target_os = "windows")]
mod audio;
#[cfg(target_os = "windows")]
mod clipboard;
#[cfg(target_os = "windows")]
mod paste;
#[cfg(target_os = "windows")]
mod overlay;

#[cfg(target_os = "windows")]
fn main() {
    use windows::Win32::Foundation::*;
    use windows::Win32::System::Com::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::*;

    env_logger::init();
    log::info!("Koe for Windows starting...");

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        SetProcessDPIAware();
    }

    let config_path = std::ffi::CString::new("").unwrap();
    let ret = koe_core::sp_core_create(config_path.as_ptr());
    if ret != 0 {
        log::error!("sp_core_create failed: {ret}");
        std::process::exit(1);
    }

    let class_name = w!("KoeMessageWindow");
    let hwnd = unsafe {
        let hmodule = GetModuleHandleW(None).unwrap();
        let hinstance = HINSTANCE(hmodule.0);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Koe"),
            WINDOW_STYLE::default(),
            0, 0, 0, 0,
            HWND_MESSAGE,
            None,
            hinstance,
            None,
        )
        .unwrap()
    };

    bridge::init(hwnd);
    tray::init(hwnd);
    overlay::init();
    hotkey::init(hwnd);

    log::info!("Koe ready — hotkey monitor active");

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    hotkey::cleanup();
    tray::cleanup(hwnd);
    koe_core::sp_core_destroy();
    unsafe { CoUninitialize() };
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::UI::WindowsAndMessaging::*;

    match msg {
        m if m >= bridge::WM_APP_SESSION_READY && m <= bridge::WM_APP_INTERIM_TEXT => {
            bridge::handle_message(hwnd, msg, wparam, lparam);
            LRESULT(0)
        }

        bridge::WM_APP_TRAY => {
            tray::handle_message(hwnd, msg, wparam, lparam);
            LRESULT(0)
        }

        WM_TIMER => {
            hotkey::handle_timer(hwnd, wparam);
            LRESULT(0)
        }

        m if m >= hotkey::WM_APP_HOTKEY_HOLD_START && m <= hotkey::WM_APP_HOTKEY_CANCEL => {
            hotkey::handle_message(hwnd, msg);
            LRESULT(0)
        }

        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("koe-win is only supported on Windows. Use KoeApp on macOS.");
    std::process::exit(1);
}
