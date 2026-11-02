//! Key presses sent to Windows with `SendInput`.

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        KEYEVENTF_KEYUP, VK_CONTROL, VK_F24, VK_LWIN,
    };

    use super::*;
    use crate::Key;

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
