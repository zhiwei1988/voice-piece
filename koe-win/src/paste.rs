use windows::Win32::UI::Input::KeyboardAndMouse::*;

fn key_event(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Simulate Ctrl+V paste by sending keyboard input events.
pub fn simulate_paste() {
    // Scan codes: Ctrl=0x1D, V=0x2F
    let inputs = [
        key_event(VK_CONTROL, 0x1D, KEYBD_EVENT_FLAGS(0)),
        key_event(VK_V, 0x2F, KEYBD_EVENT_FLAGS(0)),
        key_event(VK_V, 0x2F, KEYEVENTF_KEYUP),
        key_event(VK_CONTROL, 0x1D, KEYEVENTF_KEYUP),
    ];

    unsafe {
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        log::info!("paste: SendInput sent {sent}/4 events");
    }
}
