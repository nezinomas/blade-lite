//! Copies the view models into the window, and turns the window's callbacks
//! into view-model commands.

use std::rc::{Rc, Weak};

use razer_proto::Rgb;
use slint::{ModelRc, SharedString, VecModel};
use ui::{DisplayView, FlyoutView, Header, KeyboardView, PerformanceView, PresetOption};

use crate::shell::{RealApp, Shell};
use crate::{Choice, FlyoutWindow, MainWindow, SliderRow, Stat, SwatchItem};

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
        enabled: true,
    }
}

fn preset_choices(presets: &[PresetOption]) -> ModelRc<Choice> {
    model(presets.iter().map(|p| Choice {
        enabled: p.enabled,
        ..choice(p.label, p.selected)
    }))
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
    window.set_presets(preset_choices(&view.presets));
    window.set_sliders(model(view.sliders.iter().map(|slider| SliderRow {
        label: text(slider.label),
        stops: int(slider.stops),
        position: int(slider.position),
        text: text(&slider.text),
    })));
    let limit = view.charge_limit;
    window.set_charge_limit_shown(limit.is_some());
    window.set_charge_limit_on(limit.is_some_and(|switch| switch.on));
    window.set_charge_limit_note(match limit {
        Some(switch) if switch.on => format!("Stops charging at {}%", switch.percent).into(),
        Some(switch) => format!("Off. Turn on to stop at {}%", switch.percent).into(),
        None => "".into(),
    });
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
    window.set_logo_shown(view.logo_on.is_some());
    window.set_logo_on(view.logo_on == Some(true));
}

fn int(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn index(value: i32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

pub fn flyout(window: &FlyoutWindow, view: &FlyoutView, header: &Header) {
    window.set_device(text(&header.title));
    window.set_power(text(header.power.label()));
    window.set_presets(preset_choices(&view.presets));
    window.set_rates(model(
        view.rates.iter().map(|r| choice(&r.label, r.selected)),
    ));
    window.set_rates_enabled(view.rates_enabled);
    window.set_sixty_hz_on_battery(view.sixty_hz_on_battery);
    window.set_brightness(i32::from(view.brightness));
    window.set_brightness_text(text(&view.brightness_text));
    window.set_touchpad_on(view.touchpad_on);
    window.set_reset_enabled(view.reset_enabled);
}

/// Runs `act` with the shell, while it still exists.
fn on(shell: &Weak<Shell>, act: impl FnOnce(&Shell)) {
    if let Some(shell) = shell.upgrade() {
        act(&shell);
    }
}

fn pick_preset(shell: &Shell, i: i32) {
    shell.act(|app| match app.performance_view().pick_preset(index(i)) {
        Some(command) => app.on_performance(command),
        None => Ok(()),
    });
}

fn pick_rate(shell: &Shell, i: i32) {
    shell.act(|app| match app.display_view()?.pick_rate(index(i)) {
        Some(command) => app.on_display(command),
        None => Ok(()),
    });
}

fn move_brightness(shell: &Shell, percent: i32) {
    let percent = u8::try_from(percent).unwrap_or(u8::MAX);
    shell.act(|app| match app.keyboard_view()?.move_brightness(percent) {
        Some(command) => app.on_keyboard(command),
        None => Ok(()),
    });
}

fn toggle_sixty_hz(shell: &Shell) {
    shell.act(RealApp::toggle_sixty_hz_on_battery);
}

fn toggle_touchpad(shell: &Shell) {
    shell.act(|app| {
        let command = app.keyboard_view()?.toggle_touchpad();
        app.on_keyboard(command)
    });
}

/// Each callback builds the group's view again and asks it for the command,
/// so a click that changes nothing sends nothing.
pub fn callbacks(shell: &Rc<Shell>) {
    window_callbacks(shell);
    flyout_callbacks(shell);
}

fn flyout_callbacks(shell: &Rc<Shell>) {
    let flyout = shell.flyout();
    let s = Rc::downgrade(shell);
    flyout.on_pick_preset(move |i| on(&s, |shell| pick_preset(shell, i)));
    let s = Rc::downgrade(shell);
    flyout.on_pick_rate(move |i| on(&s, |shell| pick_rate(shell, i)));
    let s = Rc::downgrade(shell);
    flyout.on_toggle_sixty_hz(move || on(&s, toggle_sixty_hz));
    let s = Rc::downgrade(shell);
    flyout.on_move_brightness(move |percent| on(&s, |shell| move_brightness(shell, percent)));
    let s = Rc::downgrade(shell);
    flyout.on_toggle_touchpad(move || on(&s, toggle_touchpad));
    let s = Rc::downgrade(shell);
    flyout.on_reset(move || {
        on(&s, |shell| {
            shell.act(|app| {
                let view = app.performance_view();
                let reset = ui::FlyoutView::new(&view, &app.display_view()?, &app.keyboard_view()?);
                match reset.reset_to_balanced() {
                    Some(command) => app.on_performance(command),
                    None => Ok(()),
                }
            });
        });
    });
    let s = Rc::downgrade(shell);
    flyout.on_open_window(move || {
        if let Some(shell) = s.upgrade() {
            shell.show_window();
        }
    });
}

fn window_callbacks(shell: &Rc<Shell>) {
    let window = shell.window();
    let s = Rc::downgrade(shell);
    window.on_pick_preset(move |i| on(&s, |shell| pick_preset(shell, i)));
    let s = Rc::downgrade(shell);
    window.on_move_slider(move |i, position| {
        on(&s, |shell| {
            shell.act(|app| {
                let view = app.performance_view();
                match view.move_slider(index(i), index(position)) {
                    Some(command) => app.on_performance(command),
                    None => Ok(()),
                }
            });
        });
    });
    let s = Rc::downgrade(shell);
    window.on_toggle_charge_limit(move || {
        on(&s, |shell| {
            shell.act(|app| match app.performance_view().toggle_charge_limit() {
                Some(command) => app.on_performance(command),
                None => Ok(()),
            });
        });
    });
    let s = Rc::downgrade(shell);
    window.on_pick_rate(move |i| on(&s, |shell| pick_rate(shell, i)));
    let s = Rc::downgrade(shell);
    window.on_toggle_sixty_hz(move || on(&s, toggle_sixty_hz));
    let s = Rc::downgrade(shell);
    window.on_pick_native(move |i| {
        on(&s, |shell| {
            shell.act(|app| match app.display_view()?.pick_native_mode(index(i)) {
                Some(command) => app.on_display(command),
                None => Ok(()),
            });
        });
    });
    let s = Rc::downgrade(shell);
    window.on_move_brightness(move |percent| on(&s, |shell| move_brightness(shell, percent)));
    let s = Rc::downgrade(shell);
    window.on_pick_swatch(move |i| {
        on(&s, |shell| {
            shell.act(|app| match app.keyboard_view()?.pick_swatch(index(i)) {
                Some(command) => app.on_keyboard(command),
                None => Ok(()),
            });
        });
    });
    let s = Rc::downgrade(shell);
    window.on_pick_custom(move |hex| {
        on(&s, |shell| {
            let Ok(rgb) = hex.trim().parse::<Rgb>() else {
                shell.show_error(&format!("{hex} is not a colour. Use #RRGGBB."));
                return;
            };
            shell.act(|app| match app.keyboard_view()?.pick_custom(rgb) {
                Some(command) => app.on_keyboard(command),
                None => Ok(()),
            });
        });
    });
    let s = Rc::downgrade(shell);
    window.on_toggle_touchpad(move || on(&s, toggle_touchpad));
    let s = Rc::downgrade(shell);
    window.on_toggle_logo(move || {
        on(&s, |shell| {
            shell.act(|app| match app.keyboard_view()?.toggle_logo() {
                Some(command) => app.on_keyboard(command),
                None => Ok(()),
            });
        });
    });
}
