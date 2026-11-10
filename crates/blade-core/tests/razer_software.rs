use std::io;

use blade_core::{razer_software, razer_warning};
use platform::Programs;

struct FakePrograms(io::Result<Vec<&'static str>>);

impl Programs for FakePrograms {
    fn running(&self) -> io::Result<Vec<String>> {
        match &self.0 {
            Ok(names) => Ok(names.iter().map(|&name| name.to_owned()).collect()),
            Err(error) => Err(io::Error::new(error.kind(), error.to_string())),
        }
    }
}

fn found(names: Vec<&'static str>) -> Vec<String> {
    razer_software(&FakePrograms(Ok(names))).expect("the names were read")
}

#[test]
fn finds_synapse_and_razer_names_in_any_case() {
    let names = vec![
        "explorer.exe",
        "Razer Synapse Service",
        "RazerAppEngine.exe",
        "SYNAPSE3.EXE",
        "Razer 02c7 Service",
    ];
    assert_eq!(
        found(names),
        [
            "Razer 02c7 Service",
            "Razer Synapse Service",
            "RazerAppEngine.exe",
            "SYNAPSE3.EXE",
        ]
    );
}

#[test]
fn says_each_name_once() {
    let names = vec!["RazerAppEngine.exe", "RazerAppEngine.exe"];
    assert_eq!(found(names), ["RazerAppEngine.exe"]);
}

#[test]
fn leaves_other_names_alone() {
    // The laptop's THX audio service runs without Synapse.
    let names = vec![
        "thx_rznb11_svc.exe",
        "THX Spatial Audio Service",
        "blade-lite.exe",
    ];
    assert_eq!(found(names), Vec::<String>::new());
}

#[test]
fn passes_on_a_failed_read() {
    let programs = FakePrograms(Err(io::Error::other("refused")));
    assert!(razer_software(&programs).is_err());
}

#[test]
fn warns_only_when_something_was_found() {
    assert_eq!(razer_warning(&[]), None);
    let warning = razer_warning(&["Razer Synapse Service".to_owned(), "RzDev".to_owned()])
        .expect("a warning");
    assert!(
        warning.contains("Razer Synapse Service, RzDev"),
        "{warning}"
    );
}

#[test]
fn finds_razer_driver_services_by_their_rz_names() {
    let names = vec!["RzDev_02c7", "rzdevu_02c7_mou", "RzCommon"];
    assert_eq!(found(names), ["RzCommon", "RzDev_02c7", "rzdevu_02c7_mou"]);
}

#[test]
fn leaves_this_repo_s_own_test_programs_alone() {
    let names = vec!["razer_device-1a2b3c.exe", "razer_proto-4d5e6f.exe"];
    assert_eq!(found(names), Vec::<String>::new());
}
