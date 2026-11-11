//! Static keyboard colour, painted key by key as Synapse paints it.

use std::str::FromStr;

use crate::{Command, ModelProfile, RedKey, RedKeys, TOUCHPAD_OFF};

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
            let pair = hex.get(start..start + 2).ok_or(ParseRgbError)?;
            u8::from_str_radix(pair, 16).map_err(|_| ParseRgbError)
        };
        Ok(Self::new(channel(0)?, channel(2)?, channel(4)?))
    }
}

/// The commands that paint every key in one colour: the custom frame effect,
/// then one frame row per row of the model's key grid.
#[must_use]
pub fn static_colour(colour: Rgb, model: &ModelProfile) -> Vec<Command> {
    frame(colour, &[], model)
}

/// As [`static_colour`], with the model's touchpad key red, as it shows
/// while the touchpad is off.
#[must_use]
pub fn static_colour_touchpad_off(colour: Rgb, model: &ModelProfile) -> Vec<Command> {
    let red = RedKeys {
        touchpad: true,
        mic: false,
    };
    static_colour_red(colour, model, red)
}

/// As [`static_colour`], with the model's keys in `red` painted
/// [`TOUCHPAD_OFF`]. A key the model does not have, or that is not a key
/// of its grid, keeps the colour.
#[must_use]
pub fn static_colour_red(colour: Rgb, model: &ModelProfile, red: RedKeys) -> Vec<Command> {
    let keys = [(red.touchpad, model.touchpad_key), (red.mic, model.mic_key)];
    let red: Vec<(u8, u8)> = keys
        .into_iter()
        .filter_map(|(on, key)| key.filter(|_| on))
        .map(|RedKey { row, column, .. }| (row, column))
        .filter(|&(row, column)| {
            row < model.keys.rows && column < model.keys.columns && model.keys.has_key(row, column)
        })
        .collect();
    frame(colour, &red, model)
}

/// The custom frame effect, then one frame row per row of the key grid,
/// with the keys in `red` painted [`TOUCHPAD_OFF`]. Nothing for a grid with
/// no rows or no columns: the effect alone would show an empty frame.
fn frame(colour: Rgb, red: &[(u8, u8)], model: &ModelProfile) -> Vec<Command> {
    let Some(last_column) = model.keys.columns.checked_sub(1) else {
        return Vec::new();
    };
    if model.keys.rows == 0 {
        return Vec::new();
    }
    let effect = Command {
        id: SET_EFFECT,
        args: CUSTOM_FRAME.to_vec(),
    };
    let rows = (0..model.keys.rows).map(|row| frame_row(row, last_column, colour, red, model));
    std::iter::once(effect).chain(rows).collect()
}

/// One frame row: the colour on each key, black where there is no key.
fn frame_row(
    row: u8,
    last_column: u8,
    colour: Rgb,
    red: &[(u8, u8)],
    model: &ModelProfile,
) -> Command {
    let grid = &model.keys;
    let mut args = vec![FRAME_ROW_MARKER, row, FIRST_COLUMN, last_column];
    for column in 0..grid.columns {
        let rgb = if red.contains(&(row, column)) {
            TOUCHPAD_OFF
        } else if grid.has_key(row, column) {
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
