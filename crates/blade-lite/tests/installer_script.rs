//! The installer script `installer/blade-lite.iss` (stage 39, tickets 78
//! and 233). These tests only read the script, and compile it when Inno
//! Setup's compiler is found: nothing here runs the installer or the app.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;

use blade_core::{APP_NAME, RUN_KEY, RUN_VALUE, STILL_QUITTING, TRAY_FLAG, Task};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn script_path() -> PathBuf {
    root().join("installer/blade-lite.iss")
}

fn script() -> String {
    std::fs::read_to_string(script_path()).expect("installer/blade-lite.iss reads")
}

/// The lines of one `[Section]`, without comments and blank lines.
fn section(name: &str) -> Vec<String> {
    let header = format!("[{name}]");
    script()
        .lines()
        .map(str::trim)
        .skip_while(|line| !line.eq_ignore_ascii_case(&header))
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .map(str::to_string)
        .collect()
}

/// The value of a `[Setup]` directive.
fn directive(name: &str) -> Option<String> {
    section("Setup").iter().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

#[test]
fn installs_for_this_user_without_admin() {
    assert_eq!(directive("PrivilegesRequired").as_deref(), Some("lowest"));
    // No dialog that offers an install for all users (admin).
    assert_eq!(directive("PrivilegesRequiredOverridesAllowed"), None);
}

#[test]
fn never_closes_the_app_by_itself() {
    // Restart Manager's close hides the window, and its kill sends no quit
    // preset (ticket 142). The script quits the app with `--quit` instead.
    assert_eq!(directive("CloseApplications").as_deref(), Some("no"));
    assert_eq!(directive("RestartApplications").as_deref(), Some("no"));
}

#[test]
fn ships_the_app_and_its_notices_only() {
    let mut sources: Vec<String> = section("Files")
        .iter()
        .filter_map(|line| {
            let rest = line.strip_prefix("Source:")?.trim_start();
            let quoted = rest.strip_prefix('"')?;
            Some(quoted.split('"').next()?.to_string())
        })
        .collect();
    sources.sort();
    assert_eq!(
        sources,
        [
            r"..\LICENSE",
            r"..\NOTICE",
            r"..\THIRD_PARTY_LICENSES.md",
            "{#AppExe}",
        ]
    );
    // The release build, unless the compile test gives a stand-in.
    assert!(script().contains(r#"#define AppExe "..\target\release\blade-lite.exe""#));
    // `blade-dev` sends single commands with no guards for users.
    assert!(!script().to_lowercase().contains("blade-dev"));
}

#[test]
fn deletes_the_settings_only_on_an_explicit_yes() {
    let script = script();
    assert_eq!(script.matches("DelTree(").count(), 1);
    // The box's second line holds its buttons and the answer it needs.
    let answer = script
        .lines()
        .skip_while(|line| !line.contains("MsgBox('Delete your Blade Lite settings"))
        .nth(1)
        .expect("the delete box");
    assert!(answer.contains("MB_DEFBUTTON2") && answer.contains("= IDYES"));
    // Inno logs each box's answer; the uninstall log keeps it.
    assert_eq!(directive("UninstallLogging").as_deref(), Some("yes"));
}

#[test]
fn the_run_value_starts_the_installed_app_in_the_tray() {
    let line = format!(
        r#"Root: HKCU; Subkey: "{RUN_KEY}"; ValueType: string; ValueName: "{RUN_VALUE}"; ValueData: """{{app}}\blade-lite.exe"" {TRAY_FLAG}""#
    );
    let registry = section("Registry");
    assert!(
        registry.iter().any(|entry| entry.starts_with(&line)),
        "no Run value like {line} in {registry:?}"
    );
}

#[test]
fn the_code_uses_the_apps_names_and_flags() {
    let script = script();
    for text in [
        format!("'{}'", Task::Quit.flag()),
        format!("'{}'", Task::Reset.flag()),
        format!("'Global\\{APP_NAME}'"),
        format!("{{userappdata}}\\{APP_NAME}"),
        format!("'{RUN_KEY}'"),
        format!("'{RUN_VALUE}'"),
        "QuitDone = 0;".to_string(),
        "QuitFailed = 1;".to_string(),
        format!("QuitStillQuitting = {STILL_QUITTING};"),
    ] {
        assert!(script.contains(&text), "the script has no {text}");
    }
}

/// Inno Setup's compiler, if installed: on the PATH, in the user's or the
/// machine's Programs folder (GitHub's Windows runners have it there).
fn iscc() -> Option<PathBuf> {
    let mut places: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        places.push(Path::new(&dir).join(r"Programs\Inno Setup 6\ISCC.exe"));
    }
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(dir) = std::env::var_os(var) {
            places.push(Path::new(&dir).join(r"Inno Setup 6\ISCC.exe"));
        }
    }
    places.into_iter().find(|path| path.is_file())
}

/// Inno Setup's compiler, or `None` with a note where it is not installed.
/// GitHub's Windows runners have it: there a missing one is a fault.
fn iscc_or_skip() -> Option<PathBuf> {
    let found = iscc();
    if found.is_none() {
        assert!(
            std::env::var_os("CI").is_none(),
            "Inno Setup 6 not found on CI"
        );
        eprintln!("Inno Setup 6 not found: the installer script was not compiled");
    }
    found
}

/// Compiles the script with `exe` as the app into `out`.
fn compile(iscc: &Path, exe: &Path, out: &Path) -> std::process::Output {
    Command::new(iscc)
        .arg("/Q")
        .arg(format!("/DAppExe={}", exe.display()))
        .arg(format!("/O{}", out.display()))
        .arg(script_path())
        .output()
        .expect("ISCC starts")
}

/// Compiles the script with the test build of the app (never run), so the
/// `[Code]` part is checked too, and the setup takes the exe's version
/// (ticket 249). Skipped where Inno Setup is not installed.
#[test]
fn the_script_compiles() {
    let Some(iscc) = iscc_or_skip() else { return };
    let out = tempfile::tempdir().expect("a temp folder");
    let output = compile(
        &iscc,
        Path::new(env!("CARGO_BIN_EXE_blade-lite")),
        out.path(),
    );
    assert!(
        output.status.success(),
        "ISCC failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let setup = format!("blade-lite-setup-{}.exe", env!("CARGO_PKG_VERSION"));
    assert!(
        out.path().join(&setup).is_file(),
        "{setup} is made: the setup has the crate's version"
    );
}

/// An exe with no version makes no setup (ticket 249).
#[test]
fn an_exe_with_no_version_makes_no_setup() {
    let Some(iscc) = iscc_or_skip() else { return };
    let out = tempfile::tempdir().expect("a temp folder");
    let exe = out.path().join("blade-lite.exe");
    std::fs::write(&exe, b"stand-in, never run").expect("the stand-in writes");
    let output = compile(&iscc, &exe, out.path());
    assert!(!output.status.success(), "ISCC made a setup");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("has no version")
            || String::from_utf8_lossy(&output.stderr).contains("has no version")
    );
}
