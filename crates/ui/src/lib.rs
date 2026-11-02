//! View models that turn the app's state into what the window and flyout show.

mod display;
mod flyout;
mod header;
mod keyboard;
mod performance;

pub use display::{DisplayCommand, DisplayState, DisplayView, NativeOption, RateSegment};
pub use flyout::FlyoutView;
pub use header::{Header, HeaderState, MISSING, PowerPill, Stat};
pub use keyboard::{KeyboardCommand, KeyboardState, KeyboardView, Swatch};
pub use performance::{
    PerformanceCommand, PerformanceState, PerformanceView, PresetOption, Slider, SliderKind,
};
