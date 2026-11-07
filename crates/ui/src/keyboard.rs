//! The Keyboard and touchpad group: brightness, static colour, the touchpad
//! and the lid logo.

use razer_proto::{Brightness, Rgb};

/// The brightness slider's highest stop, in percent.
const BRIGHTNESS_MAX: u8 = 100;
/// The brightness text at 0.
const OFF: &str = "Off";
/// The colours offered as swatches, from the mockup.
const SWATCHES: [Rgb; 8] = [
    Rgb::new(0xFF, 0xFF, 0xFF),
    Rgb::new(0xE8, 0x11, 0x23),
    Rgb::new(0xFF, 0x8C, 0x00),
    Rgb::new(0xFF, 0xD4, 0x00),
    Rgb::new(0x44, 0xD6, 0x2C),
    Rgb::new(0x00, 0xB7, 0xC3),
    Rgb::new(0x00, 0x78, 0xD4),
    Rgb::new(0x8E, 0x44, 0xFF),
];

/// What the Keyboard and touchpad group is built from.
#[derive(Debug, Clone, Copy)]
pub struct KeyboardState {
    pub brightness: Brightness,
    pub colour: Rgb,
    pub touchpad_on: bool,
    /// `None` if the model has no logo or its state is unknown.
    pub logo_on: Option<bool>,
}

/// A change the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardCommand {
    SetBrightness(Brightness),
    SetColour(Rgb),
    SetTouchpad(bool),
    SetLogo(bool),
}

/// One colour swatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Swatch {
    pub colour: Rgb,
    pub selected: bool,
}

/// The Keyboard and touchpad group, ready to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardView {
    /// The slider runs from 0 to this, in percent.
    pub brightness_max: u8,
    pub brightness: u8,
    pub brightness_text: String,
    /// `false` while the lights are off.
    pub colours_enabled: bool,
    pub swatches: Vec<Swatch>,
    /// The colour the custom picker opens on.
    pub custom_colour: Rgb,
    /// True when the colour is none of the swatches.
    pub custom_selected: bool,
    pub touchpad_on: bool,
    /// `None` hides the lid logo switch: no logo, or its state is unknown.
    pub logo_on: Option<bool>,
}

impl KeyboardView {
    /// Builds the group from the state.
    #[must_use]
    pub fn new(state: &KeyboardState) -> Self {
        let percent = state.brightness.percent();
        let swatches = swatches(state.colour);
        Self {
            brightness_max: BRIGHTNESS_MAX,
            brightness: percent,
            brightness_text: brightness_text(percent),
            colours_enabled: percent > 0,
            custom_selected: !swatches.iter().any(|swatch| swatch.selected),
            swatches,
            custom_colour: state.colour,
            touchpad_on: state.touchpad_on,
            logo_on: state.logo_on,
        }
    }

    /// The command for moving the brightness slider to `percent`, unless
    /// the move changes nothing or is off the slider.
    #[must_use]
    pub fn move_brightness(&self, percent: u8) -> Option<KeyboardCommand> {
        if percent == self.brightness {
            return None;
        }
        let brightness = Brightness::new(percent).ok()?;
        Some(KeyboardCommand::SetBrightness(brightness))
    }

    /// The command for the swatch at `index`, unless it is already chosen.
    #[must_use]
    pub fn pick_swatch(&self, index: usize) -> Option<KeyboardCommand> {
        let swatch = self.swatches.get(index).filter(|s| !s.selected)?;
        self.set_colour(swatch.colour)
    }

    /// The command for the text typed in the custom colour field. Only
    /// `#RRGGBB` counts, with outer spaces ignored. Other text, or the
    /// current colour, gives no command, so the last good colour stays.
    #[must_use]
    pub fn pick_custom(&self, text: &str) -> Option<KeyboardCommand> {
        let colour = text.trim().parse::<Rgb>().ok()?;
        if colour == self.custom_colour {
            return None;
        }
        self.set_colour(colour)
    }

    /// The command that flips the touchpad switch.
    #[must_use]
    pub fn toggle_touchpad(&self) -> KeyboardCommand {
        KeyboardCommand::SetTouchpad(!self.touchpad_on)
    }

    /// The command that flips the lid logo switch, unless it is hidden.
    #[must_use]
    pub fn toggle_logo(&self) -> Option<KeyboardCommand> {
        self.logo_on.map(|on| KeyboardCommand::SetLogo(!on))
    }

    fn set_colour(&self, colour: Rgb) -> Option<KeyboardCommand> {
        self.colours_enabled
            .then_some(KeyboardCommand::SetColour(colour))
    }
}

fn swatches(current: Rgb) -> Vec<Swatch> {
    SWATCHES
        .iter()
        .map(|&colour| Swatch {
            colour,
            selected: colour == current,
        })
        .collect()
}

fn brightness_text(percent: u8) -> String {
    if percent == 0 {
        OFF.to_owned()
    } else {
        format!("{percent}%")
    }
}
