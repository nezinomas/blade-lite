use std::fs;

use blade_core::Log;
use blade_lite::{Reporter, ShownErrors, open_log, report, report_panics};

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

#[test]
fn reports_from_two_threads_stay_whole_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blade-lite.log");
    let reporter = Reporter::new(Some(Log::with_limits(path, 2000, 3)));
    std::thread::scope(|scope| {
        for name in ["left", "right"] {
            let reporter = &reporter;
            scope.spawn(move || {
                for n in 0..300 {
                    reporter.report(&format!("{name} {n}"));
                }
            });
        }
    });
    for entry in fs::read_dir(dir.path()).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        for line in text.lines() {
            let (_, body) = line.split_once("Z ").unwrap();
            let (name, n) = body.split_once(' ').unwrap();
            assert!(name == "left" || name == "right", "{line}");
            assert!(n.parse::<u32>().unwrap() < 300, "{line}");
        }
    }
}

/// The app's one reporter: only this test may touch it.
#[test]
fn the_app_log_opens_once_and_also_takes_panics() {
    let dir = tempfile::tempdir().unwrap();
    let path = Log::path_in(dir.path());
    assert!(open_log(Log::new(path.clone())));
    assert!(!open_log(Log::new(dir.path().join("other.log"))));
    report_panics();
    report("one");
    let panicked = std::thread::spawn(|| panic!("boom")).join();
    assert!(panicked.is_err());
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains("Z one\n"), "{text}");
    assert!(text.contains("panic") && text.contains("boom"), "{text}");
}
