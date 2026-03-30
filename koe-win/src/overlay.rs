use std::sync::Mutex;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

const MAX_WIDTH: i32 = 600;
const PILL_HEIGHT: i32 = 36;
const PADDING_H: i32 = 16;
const PADDING_V: i32 = 8;
const CORNER_RADIUS: i32 = 18;
const BG_COLOR: u32 = 0x00302020;
const TEXT_COLOR: u32 = 0x00FFFFFF;

// HWND contains *mut c_void which is !Send, but we only access from the main thread.
// Store as isize (pointer value) to satisfy Send requirement for Mutex.
static OVERLAY_HWND: Mutex<Option<isize>> = Mutex::new(None);
static DISPLAY_TEXT: Mutex<String> = Mutex::new(String::new());
static IS_VISIBLE: Mutex<bool> = Mutex::new(false);

pub fn init() {
    let class_name = w!("KoeOverlay");

    unsafe {
        let hmodule = GetModuleHandleW(None).unwrap();
        let hinstance = HINSTANCE(hmodule.0);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(overlay_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name,
            hbrBackground: CreateSolidBrush(COLORREF(BG_COLOR)),
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            class_name,
            w!(""),
            WS_POPUP,
            0, 0, 200, PILL_HEIGHT,
            HWND::default(),
            None,
            hinstance,
            None,
        )
        .unwrap();

        SetLayeredWindowAttributes(hwnd, COLORREF(0), 220, LWA_ALPHA).unwrap();

        *OVERLAY_HWND.lock().unwrap() = Some(hwnd.0 as isize);
    }
}

pub fn update_state(state: &str) {
    match state {
        "recording_hold" | "recording_toggle" | "recording" => {
            set_text("Recording...");
            show();
        }
        "finalizing_asr" => {
            set_text("Finalizing...");
        }
        "correcting" => {
            set_text("Correcting...");
        }
        "pasting" | "preparing_paste" => {
            set_text("Pasting...");
        }
        "error" | "failed" => {
            set_text("Error");
        }
        "idle" | "completed" | "cancelled" => {
            hide();
        }
        _ => {}
    }
}

pub fn update_interim_text(text: &str) {
    if !text.is_empty() {
        set_text(text);
    }
}

fn set_text(text: &str) {
    *DISPLAY_TEXT.lock().unwrap() = text.to_string();
    if let Some(val) = *OVERLAY_HWND.lock().unwrap() {
        let hwnd = HWND(val as *mut _);
        unsafe {
            let _ = InvalidateRect(hwnd, None, BOOL(1));
            let _ = UpdateWindow(hwnd);
        }
        reposition(hwnd, text);
    }
}

fn show() {
    let mut visible = IS_VISIBLE.lock().unwrap();
    if *visible {
        return;
    }
    *visible = true;
    if let Some(val) = *OVERLAY_HWND.lock().unwrap() {
        let hwnd = HWND(val as *mut _);
        unsafe {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

fn hide() {
    let mut visible = IS_VISIBLE.lock().unwrap();
    if !*visible {
        return;
    }
    *visible = false;
    if let Some(val) = *OVERLAY_HWND.lock().unwrap() {
        let hwnd = HWND(val as *mut _);
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
        }
    }
}

fn reposition(hwnd: HWND, text: &str) {
    unsafe {
        let hdc = GetDC(hwnd);
        let font = CreateFontW(
            16, 0, 0, 0,
            FW_NORMAL.0 as i32,
            0, 0, 0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            DEFAULT_PITCH.0 as u32 | (FF_SWISS.0 as u32),
            w!("Segoe UI"),
        );
        let old_font = SelectObject(hdc, font);

        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE::default();
        let _ = GetTextExtentPoint32W(hdc, &wide, &mut size);

        SelectObject(hdc, old_font);
        let _ = DeleteObject(font);
        ReleaseDC(hwnd, hdc);

        let text_width = size.cx.min(MAX_WIDTH - PADDING_H * 2);
        let window_width = text_width + PADDING_H * 2;
        let window_height = PILL_HEIGHT;

        let mut work_area = RECT::default();
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut work_area as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));

        let screen_width = work_area.right - work_area.left;
        let x = work_area.left + (screen_width - window_width) / 2;
        let y = work_area.bottom - window_height - 20;

        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x, y, window_width, window_height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    log::debug!("overlay msg=0x{msg:04X}");
    match msg {
        WM_PAINT => {
            // Avoid BeginPaint/EndPaint — it triggers WM_NCPAINT internally
            // which causes ACCESS_VIOLATION in user32.dll on Windows 11
            // for layered+transparent popup windows.
            let _ = ValidateRect(hwnd, None);

            let hdc = GetDC(hwnd);
            if hdc.is_invalid() {
                return LRESULT(0);
            }

            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);

            let brush = CreateSolidBrush(COLORREF(BG_COLOR));
            let pen = CreatePen(PS_NULL, 0, COLORREF(0));
            let old_brush = SelectObject(hdc, brush);
            let old_pen = SelectObject(hdc, pen);
            let _ = RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, CORNER_RADIUS, CORNER_RADIUS);
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
            let _ = DeleteObject(brush);
            let _ = DeleteObject(pen);

            let text = DISPLAY_TEXT.lock().unwrap().clone();
            let mut wide: Vec<u16> = text.encode_utf16().collect();

            let font = CreateFontW(
                16, 0, 0, 0,
                FW_NORMAL.0 as i32,
                0, 0, 0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                CLEARTYPE_QUALITY.0 as u32,
                DEFAULT_PITCH.0 as u32 | (FF_SWISS.0 as u32),
                w!("Segoe UI"),
            );
            let old_font = SelectObject(hdc, font);
            SetTextColor(hdc, COLORREF(TEXT_COLOR));
            SetBkMode(hdc, TRANSPARENT);

            let mut text_rect = RECT {
                left: rect.left + PADDING_H,
                top: rect.top + PADDING_V,
                right: rect.right - PADDING_H,
                bottom: rect.bottom - PADDING_V,
            };
            DrawTextW(hdc, &mut wide, &mut text_rect, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);

            SelectObject(hdc, old_font);
            let _ = DeleteObject(font);

            ReleaseDC(hwnd, hdc);
            LRESULT(0)
        }
        // Borderless layered window — skip non-client painting to avoid
        // ACCESS_VIOLATION in DefWindowProcW on Windows 11.
        WM_NCPAINT => LRESULT(0),
        WM_ERASEBKGND => LRESULT(1),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
