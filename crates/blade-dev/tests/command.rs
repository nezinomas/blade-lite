use blade_dev::Command;

#[test]
fn probe_is_a_command() {
    assert_eq!(Command::parse(["probe"]), Some(Command::Probe));
}

#[test]
fn no_command_or_an_unknown_one_is_none() {
    assert_eq!(Command::parse([]), None);
    assert_eq!(Command::parse(["fly"]), None);
}

#[test]
fn keys_is_a_command() {
    assert_eq!(Command::parse(["keys"]), Some(Command::Keys));
    assert_eq!(Command::parse(["keys", "more"]), None);
}
