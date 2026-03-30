use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;

use std::sync::Mutex;

// CF_UNICODETEXT = 13
const CF_UNICODETEXT_ID: u32 = 13;

static BACKUP: Mutex<Option<Vec<u16>>> = Mutex::new(None);
static BACKUP_SEQ: Mutex<u32> = Mutex::new(0);

pub fn backup() {
    unsafe {
        if OpenClipboard(HWND::default()).is_err() {
            log::warn!("clipboard: backup failed to open");
            return;
        }

        let handle = GetClipboardData(CF_UNICODETEXT_ID);
        let data = if let Ok(h) = handle {
            if !h.0.is_null() {
                let ptr = GlobalLock(HGLOBAL(h.0)) as *const u16;
                if !ptr.is_null() {
                    let mut len = 0;
                    while *ptr.add(len) != 0 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(ptr, len + 1);
                    let vec = slice.to_vec();
                    let _ = GlobalUnlock(HGLOBAL(h.0));
                    Some(vec)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let _ = CloseClipboard();

        *BACKUP.lock().unwrap() = data;
    }
}

pub fn write_text(text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_size = wide.len() * 2;

    unsafe {
        if OpenClipboard(HWND::default()).is_err() {
            log::warn!("clipboard: write failed to open");
            return;
        }

        let _ = EmptyClipboard();

        let hmem = GlobalAlloc(GMEM_MOVEABLE, byte_size);
        if let Ok(hmem) = hmem {
            let ptr = GlobalLock(hmem) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                let _ = GlobalUnlock(hmem);
                let _ = SetClipboardData(CF_UNICODETEXT_ID, HANDLE(hmem.0));
            }
        }

        let _ = CloseClipboard();

        *BACKUP_SEQ.lock().unwrap() = GetClipboardSequenceNumber();
    }
}

pub fn restore() {
    let backup_data = BACKUP.lock().unwrap().take();
    let Some(data) = backup_data else {
        return;
    };

    unsafe {
        let current_seq = GetClipboardSequenceNumber();
        let our_seq = *BACKUP_SEQ.lock().unwrap();
        if current_seq != our_seq {
            log::debug!("clipboard: skip restore, clipboard was modified by another app");
            return;
        }

        if OpenClipboard(HWND::default()).is_err() {
            log::warn!("clipboard: restore failed to open");
            return;
        }

        let _ = EmptyClipboard();

        let byte_size = data.len() * 2;
        let hmem = GlobalAlloc(GMEM_MOVEABLE, byte_size);
        if let Ok(hmem) = hmem {
            let ptr = GlobalLock(hmem) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
                let _ = GlobalUnlock(hmem);
                let _ = SetClipboardData(CF_UNICODETEXT_ID, HANDLE(hmem.0));
            }
        }

        let _ = CloseClipboard();
    }
}
