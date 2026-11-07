use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use blade_core::{LOG_FILES, LOG_LINE_MAX, LOG_MAX_BYTES, Log, log_line};

/// 2026-11-07 10:39:38 UTC.
fn at() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_794_047_978)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

fn old(path: &Path, n: usize) -> PathBuf {
    path.with_extension(format!("log.{n}"))
}

#[test]
fn the_log_is_beside_the_config() {
    let path = Log::path_in(Path::new("C:\\Users\\a\\AppData\\Roaming"));
    assert_eq!(
        path,
        Path::new("C:\\Users\\a\\AppData\\Roaming\\blade-lite\\blade-lite.log")
    );
}

#[test]
fn the_plan_caps_it_at_five_files_of_one_megabyte() {
    assert_eq!(LOG_FILES, 5);
    assert_eq!(LOG_MAX_BYTES, 1024 * 1024);
}

#[test]
fn a_line_starts_with_the_utc_time() {
    assert_eq!(log_line(at(), "no Mica"), "2026-11-07 10:39:38Z no Mica\n");
}

#[test]
fn a_line_before_1970_shows_the_epoch() {
    let before = SystemTime::UNIX_EPOCH - Duration::from_secs(5);
    assert_eq!(log_line(before, "x"), "1970-01-01 00:00:00Z x\n");
}

#[test]
fn a_leap_day_is_dated_right() {
    // 2028-02-29 23:59:59 UTC.
    let leap = SystemTime::UNIX_EPOCH + Duration::from_secs(1_835_481_599);
    assert_eq!(log_line(leap, "x"), "2028-02-29 23:59:59Z x\n");
}

#[test]
fn control_characters_cannot_break_a_line() {
    let line = log_line(at(), "one\ntwo\r\tthree\u{1b}[31m");
    assert_eq!(line, "2026-11-07 10:39:38Z one two  three [31m\n");
}

#[test]
fn a_long_text_is_cut() {
    let text = "é".repeat(LOG_LINE_MAX * 2);
    let line = log_line(at(), &text);
    let body = line.strip_prefix("2026-11-07 10:39:38Z ").unwrap();
    assert_eq!(body.trim_end().chars().count(), LOG_LINE_MAX);
    assert!(line.ends_with('\n'));
}

#[test]
fn writing_makes_the_folder_and_appends() {
    let dir = tempfile::tempdir().unwrap();
    let path = Log::path_in(dir.path());
    let log = Log::new(path.clone());
    log.write(at(), "first").unwrap();
    log.write(at(), "second").unwrap();
    assert_eq!(
        read(&path),
        "2026-11-07 10:39:38Z first\n2026-11-07 10:39:38Z second\n"
    );
}

#[test]
fn a_full_file_moves_to_dot_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blade-lite.log");
    let line_len = log_line(at(), "aaaa").len() as u64;
    let log = Log::with_limits(path.clone(), line_len * 2, 5);
    log.write(at(), "aaaa").unwrap();
    log.write(at(), "bbbb").unwrap();
    log.write(at(), "cccc").unwrap();
    assert!(read(&old(&path, 1)).contains("bbbb"));
    assert!(read(&old(&path, 1)).contains("aaaa"));
    assert_eq!(read(&path), log_line(at(), "cccc"));
}

#[test]
fn only_the_newest_files_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blade-lite.log");
    let line_len = log_line(at(), "0").len() as u64;
    let log = Log::with_limits(path.clone(), line_len, 3);
    for n in 0..6 {
        log.write(at(), &n.to_string()).unwrap();
    }
    assert_eq!(read(&path), log_line(at(), "5"));
    assert_eq!(read(&old(&path, 1)), log_line(at(), "4"));
    assert_eq!(read(&old(&path, 2)), log_line(at(), "3"));
    assert!(!old(&path, 3).exists());
}

#[test]
fn the_folder_never_holds_more_than_the_cap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blade-lite.log");
    let max = 200;
    let log = Log::with_limits(path, max, LOG_FILES);
    for n in 0..200 {
        log.write(at(), &format!("line {n}")).unwrap();
    }
    let entries: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(entries.len(), LOG_FILES);
    for entry in entries {
        assert!(entry.unwrap().metadata().unwrap().len() <= max);
    }
}

#[test]
fn limits_too_small_are_raised_to_one_line_and_one_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blade-lite.log");
    let log = Log::with_limits(path.clone(), 0, 0);
    log.write(at(), "a").unwrap();
    log.write(at(), "b").unwrap();
    assert_eq!(read(&path), log_line(at(), "b"));
    assert!(!old(&path, 1).exists());
}

#[test]
fn a_path_that_is_a_folder_gives_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let log = Log::new(dir.path().to_path_buf());
    assert!(log.write(at(), "x").is_err());
}

#[test]
fn a_century_that_is_not_a_leap_year_is_dated_right() {
    // 2100-03-01 00:00:00 UTC: 2100 has no 29 February.
    let march = SystemTime::UNIX_EPOCH + Duration::from_secs(4_107_542_400);
    assert_eq!(log_line(march, "x"), "2100-03-01 00:00:00Z x\n");
}

#[test]
fn the_last_second_of_a_year_is_dated_right() {
    // 2026-12-31 23:59:59 UTC.
    let end = SystemTime::UNIX_EPOCH + Duration::from_secs(1_798_761_599);
    assert_eq!(log_line(end, "x"), "2026-12-31 23:59:59Z x\n");
}

#[test]
fn line_and_direction_marks_cannot_break_a_line() {
    let line = log_line(at(), "a\u{2028}b\u{2029}c\u{202E}d\u{2066}e");
    assert_eq!(line, "2026-11-07 10:39:38Z a b c d e\n");
}

#[test]
fn a_roll_that_fails_starts_the_file_again_with_a_note() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blade-lite.log");
    // A folder where the oldest file goes, so the roll can't remove it.
    fs::create_dir(old(&path, 1)).unwrap();
    let line_len = log_line(at(), "aaaa").len() as u64;
    let log = Log::with_limits(path.clone(), line_len, 2);
    log.write(at(), "aaaa").unwrap();
    log.write(at(), "bbbb").unwrap();
    let text = read(&path);
    assert!(!text.contains("aaaa"), "{text}");
    assert!(text.contains("the log could not be rolled"), "{text}");
    assert!(text.ends_with(&log_line(at(), "bbbb")), "{text}");
}

#[test]
fn a_roll_that_keeps_failing_keeps_the_file_small() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("blade-lite.log");
    fs::create_dir(old(&path, 1)).unwrap();
    let max = 300;
    let log = Log::with_limits(path.clone(), max, 2);
    for n in 0..100 {
        log.write(at(), &format!("line {n}")).unwrap();
    }
    assert!(fs::metadata(&path).unwrap().len() <= max);
}
