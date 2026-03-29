use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;

use std::sync::Mutex;

static BACKUP: Mutex<Option<Vec<u16>>> = Mutex::new(None);
static BACKUP_SEQ: Mutex<u32> = Mutex::new(0);

/// Backup current clipboard text content.
pub fn backup() {
    unsafe {
        if OpenClipboard(None).is_err() {
            log::warn!("clipboard: backup failed to open");
            return;
        }

        let handle = GetClipboardData(CF_UNICODETEXT.0 as u32);
        let data = if let Ok(h) = handle {
            if h.0 != std::ptr::null_mut() {
                let ptr = GlobalLock(HGLOBAL(h.0)) as *const u16;
                if !ptr.is_null() {
                    let mut len = 0;
                    while *ptr.add(len) != 0 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(ptr, len + 1); // include null
                    let vec = slice.to_vec();
                    GlobalUnlock(HGLOBAL(h.0)).ok();
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

/// Write text to the clipboard.
pub fn write_text(text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_size = wide.len() * 2;

    unsafe {
        if OpenClipboard(None).is_err() {
            log::warn!("clipboard: write failed to open");
            return;
        }

        let _ = EmptyClipboard();

        let hmem = GlobalAlloc(GMEM_MOVEABLE, byte_size);
        if let Ok(hmem) = hmem {
            let ptr = GlobalLock(hmem) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                GlobalUnlock(hmem).ok();
                let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(hmem.0));
            }
        }

        let _ = CloseClipboard();

        // Record sequence number for restore check
        *BACKUP_SEQ.lock().unwrap() = GetClipboardSequenceNumber();
    }
}

/// Restore the backed-up clipboard content.
/// Only restores if no other app has modified the clipboard since our write.
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

        if OpenClipboard(None).is_err() {
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
                GlobalUnlock(hmem).ok();
                let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(hmem.0));
            }
        }

        let _ = CloseClipboard();
    }
}
