//! Key presses sent to Windows with `SendInput`, and the Num Lock state.

use std::io;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    SendInput, VIRTUAL_KEY, VK_CONTROL, VK_F24, VK_LWIN, VK_NUMLOCK,
};

use crate::{Key, KeyInjector};

/// Whether Num Lock is on now, as the calling thread sees it. Call it from
/// a thread that runs a message loop, such as the UI thread's poll. A
/// process without focus sees the changes (checked on the laptop
/// 2026-11-09).
#[must_use]
pub fn num_lock_on() -> bool {
    // SAFETY: GetKeyState takes a key code by value and touches no memory
    // of ours.
    let state = unsafe { GetKeyState(i32::from(VK_NUMLOCK)) };
    // The low bit is the toggle.
    state & 1 == 1
}

/// Key presses that reach Windows as if typed on the keyboard.
pub struct WindowsKeys;

impl KeyInjector for WindowsKeys {
    fn press(&mut self, keys: &[Key]) -> io::Result<()> {
        let inputs: Vec<INPUT> = events(keys)
            .into_iter()
            .map(|(key, flags)| input(key, flags))
            .collect();
        let count = u32::try_from(inputs.len()).unwrap_or(u32::MAX);
        let size = i32::try_from(size_of::<INPUT>()).unwrap_or(i32::MAX);
        // SAFETY: the pointer and count describe the vector, which lives
        // past the call, and each entry is a keyboard input.
        let sent = unsafe { SendInput(count, inputs.as_ptr(), size) };
        if sent != count {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Each key down in order, then each key up in reverse order.
fn events(keys: &[Key]) -> Vec<(VIRTUAL_KEY, KEYBD_EVENT_FLAGS)> {
    let down = keys.iter().map(|&key| (virtual_key(key), 0));
    let up = keys
        .iter()
        .rev()
        .map(|&key| (virtual_key(key), KEYEVENTF_KEYUP));
    down.chain(up).collect()
}

fn virtual_key(key: Key) -> VIRTUAL_KEY {
    match key {
        Key::Win => VK_LWIN,
        Key::Ctrl => VK_CONTROL,
        Key::F24 => VK_F24,
    }
}

fn input(key: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_go_down_in_order_and_up_in_reverse() {
        let down = 0;
        let up = KEYEVENTF_KEYUP;

        assert_eq!(
            events(&[Key::Win, Key::Ctrl, Key::F24]),
            [
                (VK_LWIN, down),
                (VK_CONTROL, down),
                (VK_F24, down),
                (VK_F24, up),
                (VK_CONTROL, up),
                (VK_LWIN, up),
            ]
        );
    }
}
