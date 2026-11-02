//! Commands with their args, ready to wrap in a [`crate::Request`].

/// One command for the controller. The high byte of `id` is the command class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub id: u16,
    pub args: Vec<u8>,
}
