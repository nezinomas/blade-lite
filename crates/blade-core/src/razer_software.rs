//! Razer's own software, such as Synapse or a Razer driver from Windows
//! Update. It would talk to the controller beside the app, so the app warns.
//! It only looks: nothing sent changes.

use std::io;

use platform::Programs;

/// Words that mark a program or service name as Razer's, in lower case.
const MARKS: [&str; 2] = ["razer", "synapse"];

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
    MARKS.iter().any(|mark| name.contains(mark))
}

/// The warning for what [`razer_software`] found, or `None` if nothing.
#[must_use]
pub fn razer_warning(found: &[String]) -> Option<String> {
    (!found.is_empty()).then(|| {
        format!(
            "Razer software runs ({}). It can talk to the controller too, \
             and the two can clash: quit or remove it.",
            found.join(", ")
        )
    })
}
