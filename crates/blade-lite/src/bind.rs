//! Copies the view models into the window, and turns the window's callbacks
//! into view-model commands.

use std::rc::Rc;

use razer_proto::Rgb;
use slint::{ModelRc, SharedString, VecModel};
use ui::{DisplayView, Header, KeyboardView, PerformanceView};

use crate::{Choice, MainWindow, Shell, SliderRow, Stat, SwatchItem};

pub fn colour(rgb: Rgb) -> slint::Color {
    slint::Color::from_rgb_u8(rgb.red, rgb.green, rgb.blue)
}

fn model<T: Clone + 'static>(items: impl IntoIterator<Item = T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items.into_iter().collect::<Vec<_>>()))
}

fn choice(label: &str, selected: bool) -> Choice {
    Choice {
        label: label.into(),
        selected,
    }
}

fn text(value: &str) -> SharedString {
    value.into()
}

pub fn header(window: &MainWindow, header: &Header) {
    let stats: Vec<Stat> = header
        .stats
        .iter()
        .map(|stat| Stat {
            label: text(&stat.label),
            value: text(&stat.value),
        })
        .collect();
    // Two rows, as in the mockup.
    let (top, bottom) = stats.split_at(stats.len().div_ceil(2));
    window.set_device(text(&header.title));
    window.set_preset(text(header.preset));
    window.set_power(text(header.power.label()));
    window.set_stats_top(model(top.to_vec()));
    window.set_stats_bottom(model(bottom.to_vec()));
}

pub fn display(window: &MainWindow, view: &DisplayView) {
    window.set_rates(model(
        view.rates.iter().map(|r| choice(&r.label, r.selected)),
    ));
    window.set_rates_enabled(view.rates_enabled);
    window.set_rate_note(text(&view.rate_note));
    window.set_sixty_hz_on_battery(view.sixty_hz_on_battery);
    window.set_native_modes(model(
        view.native_modes
            .iter()
            .map(|m| choice(&m.label, m.selected)),
    ));
    window.set_native_note(text(&view.native_note));
    window.set_restart_infobar(view.restart_infobar);
}

pub fn performance(window: &MainWindow, view: &PerformanceView) {
    window.set_presets(model(
        view.presets.iter().map(|p| choice(p.label, p.selected)),
    ));
    window.set_sliders(model(view.sliders.iter().map(|slider| SliderRow {
        label: text(slider.label),
        stops: int(slider.stops),
        position: int(slider.position),
        text: text(&slider.text),
    })));
}

pub fn keyboard(window: &MainWindow, view: &KeyboardView) {
    window.set_brightness(i32::from(view.brightness));
    window.set_brightness_text(text(&view.brightness_text));
    window.set_colours_enabled(view.colours_enabled);
    window.set_swatches(model(view.swatches.iter().map(|swatch| SwatchItem {
        colour: colour(swatch.colour),
        selected: swatch.selected,
    })));
    window.set_custom_selected(view.custom_selected);
    let Rgb { red, green, blue } = view.custom_colour;
    window.set_custom_hex(format!("#{red:02X}{green:02X}{blue:02X}").into());
    window.set_touchpad_on(view.touchpad_on);
}

fn int(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn index(value: i32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// Each callback builds the group's view again and asks it for the command,
/// so a click that changes nothing sends nothing.
pub fn callbacks(window: &MainWindow, shell: &Rc<Shell>) {
    let s = Rc::clone(shell);
    window.on_pick_preset(move |i| {
        s.act(|app| match app.performance_view().pick_preset(index(i)) {
            Some(command) => app.on_performance(command),
            None => Ok(()),
        });
    });
    let s = Rc::clone(shell);
    window.on_move_slider(move |i, position| {
        s.act(|app| {
            let view = app.performance_view();
            match view.move_slider(index(i), index(position)) {
                Some(command) => app.on_performance(command),
                None => Ok(()),
            }
        });
    });
    let s = Rc::clone(shell);
    window.on_pick_rate(move |i| {
        s.act(|app| match app.display_view()?.pick_rate(index(i)) {
            Some(command) => app.on_display(command),
            None => Ok(()),
        });
    });
    let s = Rc::clone(shell);
    window.on_toggle_sixty_hz(move || {
        s.act(|app| {
            let command = app.display_view()?.toggle_sixty_hz_on_battery();
            app.on_display(command)
        });
    });
    let s = Rc::clone(shell);
    window.on_pick_native(move |i| {
        s.act(|app| match app.display_view()?.pick_native_mode(index(i)) {
            Some(command) => app.on_display(command),
            None => Ok(()),
        });
    });
    let s = Rc::clone(shell);
    window.on_move_brightness(move |percent| {
        let percent = u8::try_from(percent).unwrap_or(u8::MAX);
        s.act(|app| match app.keyboard_view()?.move_brightness(percent) {
            Some(command) => app.on_keyboard(command),
            None => Ok(()),
        });
    });
    let s = Rc::clone(shell);
    window.on_pick_swatch(move |i| {
        s.act(|app| match app.keyboard_view()?.pick_swatch(index(i)) {
            Some(command) => app.on_keyboard(command),
            None => Ok(()),
        });
    });
    let s = Rc::clone(shell);
    window.on_pick_custom(move |hex| {
        let Ok(rgb) = hex.trim().parse::<Rgb>() else {
            s.show_error(&format!("{hex} is not a colour. Use #RRGGBB."));
            return;
        };
        s.act(|app| match app.keyboard_view()?.pick_custom(rgb) {
            Some(command) => app.on_keyboard(command),
            None => Ok(()),
        });
    });
    let s = Rc::clone(shell);
    window.on_toggle_touchpad(move || {
        s.act(|app| {
            let command = app.keyboard_view()?.toggle_touchpad();
            app.on_keyboard(command)
        });
    });
}
