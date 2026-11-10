//! The power-mode key: a Razer key that the keyboard reports on its own HID
//! collection, not as a normal key press.
//!
//! That collection sends input reports that list the Razer keys now held:
//! the report id, then one code per held key, then zeros. On the Blade 18,
//! Fn + numpad Enter gives `04 0A D3`: `0A` is Fn and `D3` the power-mode
//! key. Letting go of all keys gives `04 00`.

/// Where the model's power-mode key is reported, and its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerKey {
    /// The collection that sends the key reports.
    pub interface_number: i32,
    pub usage_page: u16,
    pub usage: u16,
    /// The first byte of each key report.
    pub report_id: u8,
    /// The size of each key report, with its id. A report of another size
    /// is not a key report.
    pub report_len: usize,
    /// The key's code in the list of held keys.
    pub code: u8,
    /// Fn's code in the list. With Num Lock on, Fn + numpad Enter sends
    /// only Fn here, and the Enter goes to Windows as a plain Enter.
    pub fn_code: u8,
}

impl PowerKey {
    /// Whether the key is held in this input report. `None` if the report
    /// is not a key report, so the caller ignores it.
    #[must_use]
    pub fn held(&self, report: &[u8]) -> Option<bool> {
        self.lists(report, self.code)
    }

    /// Whether Fn is held in this input report. `None` if the report is
    /// not a key report.
    #[must_use]
    pub fn fn_held(&self, report: &[u8]) -> Option<bool> {
        self.lists(report, self.fn_code)
    }

    /// Whether this key report lists `code` as held.
    fn lists(&self, report: &[u8], code: u8) -> Option<bool> {
        if report.len() != self.report_len {
            return None;
        }
        let (&id, held) = report.split_first()?;
        if id != self.report_id {
            return None;
        }
        // Code 0 ends the list; nothing after it counts.
        let mut listed = held.iter().take_while(|&&code| code != 0);
        Some(listed.any(|&held| held == code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: PowerKey = PowerKey {
        interface_number: 1,
        usage_page: 0x01,
        usage: 0x00,
        report_id: 0x04,
        report_len: 16,
        code: 0xD3,
        fn_code: 0x0A,
    };

    /// A 16-byte report as the laptop sends it.
    fn report(bytes: &[u8]) -> [u8; 16] {
        let mut report = [0; 16];
        report[..bytes.len()].copy_from_slice(bytes);
        report
    }

    #[test]
    fn fn_and_the_key_is_held() {
        assert_eq!(KEY.held(&report(&[0x04, 0x0A, 0xD3])), Some(true));
    }

    #[test]
    fn the_key_alone_is_held() {
        assert_eq!(KEY.held(&report(&[0x04, 0xD3])), Some(true));
    }

    #[test]
    fn fn_alone_is_not_the_key() {
        assert_eq!(KEY.held(&report(&[0x04, 0x0A])), Some(false));
    }

    #[test]
    fn all_keys_up_is_not_held() {
        assert_eq!(KEY.held(&report(&[0x04])), Some(false));
    }

    #[test]
    fn another_razer_key_is_not_the_key() {
        // The touchpad key.
        assert_eq!(KEY.held(&report(&[0x04, 0xDD])), Some(false));
    }

    #[test]
    fn a_code_after_the_end_of_the_list_does_not_count() {
        assert_eq!(KEY.held(&report(&[0x04, 0x0A, 0x00, 0xD3])), Some(false));
    }

    #[test]
    fn fn_is_held_with_or_without_the_key() {
        assert_eq!(KEY.fn_held(&report(&[0x04, 0x0A])), Some(true));
        assert_eq!(KEY.fn_held(&report(&[0x04, 0x0A, 0xD3])), Some(true));
    }

    #[test]
    fn fn_is_not_held_once_the_keys_are_up() {
        assert_eq!(KEY.fn_held(&report(&[0x04])), Some(false));
        assert_eq!(KEY.fn_held(&report(&[0x04, 0xD3])), Some(false));
    }

    #[test]
    fn fn_in_another_report_is_ignored() {
        assert_eq!(KEY.fn_held(&report(&[0x05, 0x0A])), None);
        assert_eq!(KEY.fn_held(&[0x04, 0x0A]), None);
    }

    #[test]
    fn another_report_id_is_ignored() {
        assert_eq!(KEY.held(&report(&[0x05, 0xD3])), None);
    }

    #[test]
    fn a_report_of_another_size_is_ignored() {
        assert_eq!(KEY.held(&report(&[0x04, 0xD3])[..15]), None);
        let mut long = [0; 17];
        long[..2].copy_from_slice(&[0x04, 0xD3]);
        assert_eq!(KEY.held(&long), None);
        assert_eq!(KEY.held(&[0x04, 0xD3]), None);
        assert_eq!(KEY.held(&[]), None);
    }

    #[test]
    fn a_key_of_no_size_reads_nothing() {
        let none = PowerKey {
            report_len: 0,
            ..KEY
        };

        assert_eq!(none.held(&[]), None);
    }
}
