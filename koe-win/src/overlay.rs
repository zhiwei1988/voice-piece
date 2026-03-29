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
const BG_COLOR: u32 = 0x00302020; // dark brownish (BGR)
const TEXT_COLOR: u32 = 0x00FFFFFF; // white (BGR)

static OVERLAY_HWND: Mutex<Option<HWND>> = Mutex::new(None);
static DISPLAY_TEXT: Mutex<String> = Mutex::new(String::new());
static IS_VISIBLE: Mutex<bool> = Mutex::new(false);

pub fn init() {
    let class_name = w!("KoeOverlay");

    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(overlay_wnd_proc),
            hInstance: hinstance.into(),
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
            None,
            None,
            Some(hinstance.into()),
            None,
        )
        .unwrap();

        SetLayeredWindowAttributes(hwnd, COLORREF(0), 220, LWA_ALPHA).unwrap();

        *OVERLAY_HWND.lock().unwrap() = Some(hwnd);
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
    if let Some(hwnd) = *OVERLAY_HWND.lock().unwrap() {
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, true);
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
    if let Some(hwnd) = *OVERLAY_HWND.lock().unwrap() {
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
    if let Some(hwnd) = *OVERLAY_HWND.lock().unwrap() {
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
        }
    }
}

fn reposition(hwnd: HWND, text: &str) {
    unsafe {
        // Measure text width
        let hdc = GetDC(Some(hwnd));
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
        GetTextExtentPoint32W(hdc, &wide, &mut size).ok();

        SelectObject(hdc, old_font);
        DeleteObject(font).ok();
        ReleaseDC(Some(hwnd), hdc);

        let text_width = size.cx.min(MAX_WIDTH - PADDING_H * 2);
        let window_width = text_width + PADDING_H * 2;
        let window_height = PILL_HEIGHT;

        // Get work area (screen minus taskbar)
        let mut work_area = RECT::default();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut work_area as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).ok();

        let screen_width = work_area.right - work_area.left;
        let x = work_area.left + (screen_width - window_width) / 2;
        let y = work_area.bottom - window_height - 20; // 20px above taskbar

        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x, y, window_width, window_height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        ).ok();
    }
}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect).ok();

            // Dark rounded background
            let brush = CreateSolidBrush(COLORREF(BG_COLOR));
            let pen = CreatePen(PS_NULL, 0, COLORREF(0));
            let old_brush = SelectObject(hdc, brush);
            let old_pen = SelectObject(hdc, pen);
            RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, CORNER_RADIUS, CORNER_RADIUS).ok();
            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
            DeleteObject(brush).ok();
            DeleteObject(pen).ok();

            // Text
            let text = DISPLAY_TEXT.lock().unwrap().clone();
            let wide: Vec<u16> = text.encode_utf16().collect();

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
            let mut wide_buf = wide.clone();
            DrawTextW(hdc, &mut wide_buf, &mut text_rect, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);

            SelectObject(hdc, old_font);
            DeleteObject(font).ok();

            EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
