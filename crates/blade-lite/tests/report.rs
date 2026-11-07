use std::fs;

use blade_core::Log;
use blade_lite::{Reporter, ShownErrors};

#[test]
fn a_report_goes_to_the_log_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = Log::path_in(dir.path());
    let reporter = Reporter::new(Some(Log::new(path.clone())));
    reporter.report("no Mica: refused");
    let text = fs::read_to_string(path).unwrap();
    assert!(text.ends_with("Z no Mica: refused\n"), "{text}");
}

#[test]
fn a_report_without_a_log_file_does_nothing_bad() {
    Reporter::new(None).report("no Mica");
}

#[test]
fn a_log_file_that_cannot_be_written_does_not_stop_the_app() {
    let dir = tempfile::tempdir().unwrap();
    // A folder in the file's place.
    let reporter = Reporter::new(Some(Log::new(dir.path().to_path_buf())));
    reporter.report("no Mica");
    reporter.report("still no Mica");
}

#[test]
fn shown_errors_are_logged_once_until_they_change() {
    let mut shown = ShownErrors::default();
    assert_eq!(shown.new_text("busy"), Some("busy"));
    assert_eq!(shown.new_text("busy"), None);
    assert_eq!(shown.new_text("busy\nno display"), Some("busy\nno display"));
}

#[test]
fn no_error_is_never_logged() {
    let mut shown = ShownErrors::default();
    assert_eq!(shown.new_text(""), None);
    shown.new_text("busy");
    assert_eq!(shown.new_text(""), None);
    // The same error after a clear spell is news again.
    assert_eq!(shown.new_text("busy"), Some("busy"));
}
