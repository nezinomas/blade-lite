//! Static keyboard colour, painted key by key as Synapse paints it.

use std::str::FromStr;

use crate::{Command, ModelProfile};

/// Set the keyboard effect. Args `[05, 00]` pick the custom frame.
const SET_EFFECT: u16 = 0x030A;
const CUSTOM_FRAME: [u8; 2] = [0x05, 0x00];
/// Paint one row of the frame. Args are `[FF, row, first column, last column, RGB...]`.
const SET_FRAME_ROW: u16 = 0x030B;
const FRAME_ROW_MARKER: u8 = 0xFF;
const FIRST_COLUMN: u8 = 0;

const BLACK: Rgb = Rgb::new(0, 0, 0);

/// A colour, one byte per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    #[must_use]
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

/// The text is not a colour in `#RRGGBB` form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseRgbError;

impl FromStr for Rgb {
    type Err = ParseRgbError;

    /// Reads `#RRGGBB`, in upper or lower case hex.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let hex = text
            .strip_prefix('#')
            .filter(|hex| hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or(ParseRgbError)?;
        let channel = |start: usize| {
            u8::from_str_radix(&hex[start..start + 2], 16).map_err(|_| ParseRgbError)
        };
        Ok(Self::new(channel(0)?, channel(2)?, channel(4)?))
    }
}

/// The commands that paint every key in one colour: the custom frame effect,
/// then one frame row per row of the model's key grid.
#[must_use]
pub fn static_colour(colour: Rgb, model: &ModelProfile) -> Vec<Command> {
    let effect = Command {
        id: SET_EFFECT,
        args: CUSTOM_FRAME.to_vec(),
    };
    let rows = (0..model.keys.rows).map(|row| frame_row(row, colour, model));
    std::iter::once(effect).chain(rows).collect()
}

/// One frame row: the colour on each key, black where there is no key.
fn frame_row(row: u8, colour: Rgb, model: &ModelProfile) -> Command {
    let grid = &model.keys;
    let mut args = vec![FRAME_ROW_MARKER, row, FIRST_COLUMN, grid.columns - 1];
    for column in 0..grid.columns {
        let rgb = if grid.has_key(row, column) {
            colour
        } else {
            BLACK
        };
        args.extend([rgb.red, rgb.green, rgb.blue]);
    }
    Command {
        id: SET_FRAME_ROW,
        args,
    }
}
