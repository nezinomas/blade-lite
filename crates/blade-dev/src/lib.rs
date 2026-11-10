//! The commands of `blade-dev`.

/// What `blade-dev` was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Find the controller and read its firmware version.
    Probe,
    /// Print the Razer key reports, and the keyboard and vapor light
    /// levels as they change, to find which keys change what.
    Keys,
}

impl Command {
    /// Reads the command from the args after the program name.
    pub fn parse<'a>(args: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let mut args = args.into_iter();
        match (args.next(), args.next()) {
            (Some("probe"), None) => Some(Self::Probe),
            (Some("keys"), None) => Some(Self::Keys),
            _ => None,
        }
    }
}
