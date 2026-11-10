//! Looks for Razer's own software, such as Synapse or a Razer driver from
//! Windows Update, which would talk to the controller beside the app. It
//! only warns: nothing sent changes.

use blade_core::{razer_software, razer_warning};
use blade_lite::report;

use super::{Parts, Shell};

impl<P: Parts> Shell<P> {
    /// Looks for Razer software. The next refresh shows the warning while
    /// it runs, and drops it once it is gone. Called at start and after
    /// each resume, before the refresh. True only the first time something
    /// is found in this run, so the caller opens the window once.
    pub fn look_for_razer(&self) -> bool {
        let found = razer_software(&self.programs).unwrap_or_else(|error| {
            report(&format!("can't look for Razer software: {error}"));
            Vec::new()
        });
        let warning = razer_warning(&found);
        let first = warning.is_some() && !self.razer_opened.replace(true);
        self.razer_warning.replace(warning);
        first
    }

    #[cfg(test)]
    pub fn programs(&self) -> &P::Programs {
        &self.programs
    }
}
