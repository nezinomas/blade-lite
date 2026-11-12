//! Razer's own software, such as Synapse or a Razer driver from Windows
//! Update. It would talk to the controller beside the app, so the app warns
//! and stops its background reads and fan guard; an updater stops every
//! send. The names are a guess: no Razer updater was seen on the laptop.

use std::io;

use platform::Programs;

/// Words that mark a program or service name as Razer's, in lower case.
const MARKS: [&str; 2] = ["razer", "synapse"];
/// How Razer's driver services start, such as `RzDev_02c7`. Only at the
/// start: the THX audio service `thx_rznb11_svc` has it inside.
const RZ: &str = "rz";
/// This repo's own test programs, such as `razer_device-<hash>.exe`.
const OWN_TESTS: [&str; 3] = ["razer_device-", "razer_proto-", "razer_software-"];

/// The running programs and services that look like Razer's, sorted, each
/// once.
///
/// # Errors
///
/// The Windows error if what runs could not be read.
pub fn razer_software(programs: &impl Programs) -> io::Result<Vec<String>> {
    let mut found: Vec<String> = programs
        .running()?
        .into_iter()
        .filter(|name| is_razer(name))
        .collect();
    found.sort_unstable();
    found.dedup();
    Ok(found)
}

fn is_razer(name: &str) -> bool {
    let name = name.to_lowercase();
    if OWN_TESTS.iter().any(|own| name.starts_with(own)) {
        return false;
    }
    name.starts_with(RZ) || MARKS.iter().any(|mark| name.contains(mark))
}

/// Words that mark a Razer program as an updater, in lower case. A
/// firmware update must never meet the app's sends.
const UPDATER_MARKS: [&str; 4] = ["update", "firmware", "flash", "bios"];

/// Whether what [`razer_software`] found holds an updater, such as a
/// firmware updater: the app then sends nothing at all.
#[must_use]
pub fn razer_updater(found: &[String]) -> bool {
    found.iter().any(|name| {
        let name = name.to_lowercase();
        UPDATER_MARKS.iter().any(|mark| name.contains(mark))
    })
}

/// The warning for what [`razer_software`] found, or `None` if nothing.
#[must_use]
pub fn razer_warning(found: &[String]) -> Option<String> {
    (!found.is_empty()).then(|| {
        if razer_updater(found) {
            return format!(
                "A Razer updater runs ({}). The app sends nothing to the \
                 controller until it ends, and the fan guard is off. A \
                 manual fan stays as set until then.",
                found.join(", ")
            );
        }
        format!(
            "Razer software runs ({}). It can talk to the controller too, \
             and the two can clash: quit or remove it. The fan guard is off \
             while it runs.",
            found.join(", ")
        )
    })
}
