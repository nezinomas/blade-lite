//! Copies the view models into the window, and turns the window's callbacks
//! into view-model commands.

use std::rc::{Rc, Weak};

use razer_proto::Rgb;
use slint::{Model, ModelRc, SharedString, VecModel};
use ui::{DisplayView, Header, KeyboardView, PerformanceView, PresetOption};

use crate::shell::{AppOn, Parts, Shell};
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
    window.set_power(text(header.power.label()));
    window.set_health(text(&header.health));
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
    sliders(window, view);
    let limit = view.charge_limit;
    window.set_charge_limit_shown(limit.is_some());
    window.set_charge_limit_on(limit.is_some_and(|switch| switch.on));
    window.set_charge_limit_note(match limit {
        Some(switch) if switch.on => format!("Stops charging at {}%", switch.percent).into(),
        Some(switch) => format!("Off. Turn on to stop at {}%", switch.percent).into(),
        None => "".into(),
    });
}

/// Shows the Custom slider rows. A row that changed is updated, not made
/// again, so a focused slider keeps the focus. A drag or arrow key not yet
/// sent stays, unless the row changed. Only a change in the
/// number of rows makes them again.
fn sliders(window: &MainWindow, view: &PerformanceView) {
    update_sliders(window, view, same_row);
}

/// Puts each Custom slider back at the laptop's stop at once, in place, so
/// a drag the window lost is dropped and a focused slider keeps the focus.
pub fn put_back_sliders(window: &MainWindow, view: &PerformanceView) {
    update_sliders(window, view, |_, _| false);
}

/// Sets each row that is not `same` as the one shown.
fn update_sliders(
    window: &MainWindow,
    view: &PerformanceView,
    same: impl Fn(&SliderRow, &SliderRow) -> bool,
) {
    let rows = window.get_sliders();
    if rows.row_count() != view.sliders.len() {
        return window.set_sliders(model(slider_rows(view)));
    }
    for (i, row) in slider_rows(view).enumerate() {
        if !rows.row_data(i).is_some_and(|shown| same(&shown, &row)) {
            rows.set_row_data(i, row);
        }
    }
}

/// Rows equal by value, but for where the slider is: `ModelRc` equals only
/// itself, so the texts are compared one by one.
fn same_row(a: &SliderRow, b: &SliderRow) -> bool {
    a.label == b.label
        && a.stops == b.stops
        && a.position == b.position
        && a.text == b.text
        && a.texts.iter().eq(b.texts.iter())
}

fn slider_rows(view: &PerformanceView) -> impl Iterator<Item = SliderRow> + '_ {
    view.sliders.iter().map(|slider| SliderRow {
        label: text(slider.label),
        stops: int(slider.stops()),
        position: int(slider.position),
        value: stop(slider.position),
        text: text(&slider.text),
        texts: model(slider.texts.iter().map(|t| text(t))),
    })
}

pub fn keyboard(window: &MainWindow, view: &KeyboardView) {
    window.set_brightness(i32::from(view.brightness));
    window.set_colours_enabled(view.colours_enabled);
    window.set_swatches(model(view.swatches.iter().map(|swatch| SwatchItem {
        colour: colour(swatch.colour),
        selected: swatch.selected,
    })));
    window.set_custom_selected(view.custom_selected);
    let Rgb { red, green, blue } = view.custom_colour;
    window.set_custom_hex(format!("#{red:02X}{green:02X}{blue:02X}").into());
    window.set_logo_shown(view.logo_on.is_some());
    window.set_logo_on(view.logo_on == Some(true));
    window.set_vapor_light_shown(view.vapor_light_on.is_some());
    window.set_vapor_light_on(view.vapor_light_on == Some(true));
}

fn int(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// A slider stop as the slider's value. Stops are few, so the cap is never
/// reached.
fn stop(value: usize) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

fn index(value: i32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// The flyout's header, presets and reset, which need no read that can
/// fail.
pub fn flyout_performance(window: &FlyoutWindow, view: &PerformanceView, header: &Header) {
    window.set_device(text(&header.title));
    window.set_power(text(header.power.label()));
    window.set_presets(preset_choices(&view.presets));
    window.set_reset_enabled(view.reset_enabled());
}

/// The flyout's rates, as the Display group shows them. The 60 Hz switch
/// is set from the config on each refresh.
pub fn flyout_display(window: &FlyoutWindow, view: &DisplayView) {
    window.set_rates(model(
        view.rates.iter().map(|r| choice(&r.label, r.selected)),
    ));
    window.set_rates_enabled(view.rates_enabled);
}

/// The flyout's brightness, as the Keyboard group shows it.
pub fn flyout_keyboard(window: &FlyoutWindow, view: &KeyboardView) {
    window.set_brightness(i32::from(view.brightness));
}

/// Runs `act` with the shell, while it still exists.
fn on<P: Parts>(shell: &Weak<Shell<P>>, act: impl FnOnce(&Rc<Shell<P>>)) {
    if let Some(shell) = shell.upgrade() {
        act(&shell);
    }
}

fn pick_preset<P: Parts>(shell: &Rc<Shell<P>>, i: i32) {
    shell.user_act(|app| match app.performance_view().pick_preset(index(i)) {
        Some(command) => app.on_performance(command),
        None => Ok(()),
    });
}

fn pick_rate<P: Parts>(shell: &Rc<Shell<P>>, i: i32) {
    // The rates come from Windows: no controller read for them.
    shell.user_act(|app| match app.display_view_unread()?.pick_rate(index(i)) {
        Some(command) => app.on_display(command),
        None => Ok(()),
    });
}

fn move_brightness<P: Parts>(shell: &Rc<Shell<P>>, percent: i32) {
    let percent = u8::try_from(percent).unwrap_or(u8::MAX);
    shell.user_act(|app| match app.keyboard_view()?.move_brightness(percent) {
        Some(command) => app.on_keyboard(command),
        None => Ok(()),
    });
}

fn toggle_sixty_hz<P: Parts>(shell: &Rc<Shell<P>>) {
    shell.user_act(AppOn::<P>::toggle_sixty_hz_on_battery);
}

fn toggle_touchpad<P: Parts>(shell: &Rc<Shell<P>>) {
    shell.user_act(|app| match app.keyboard_view()?.toggle_touchpad() {
        Some(command) => app.on_keyboard(command),
        None => Ok(()),
    });
}

/// Each callback builds the group's view again and asks it for the command,
/// so a click that changes nothing sends nothing.
pub fn callbacks<P: Parts>(shell: &Rc<Shell<P>>) {
    window_callbacks(shell);
    flyout_callbacks(shell);
}

fn flyout_callbacks<P: Parts>(shell: &Rc<Shell<P>>) {
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
            shell.user_act(|app| match app.performance_view().reset_to_balanced() {
                Some(command) => app.on_performance(command),
                None => Ok(()),
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

fn window_callbacks<P: Parts>(shell: &Rc<Shell<P>>) {
    let window = shell.window();
    let s = Rc::downgrade(shell);
    window.on_pick_preset(move |i| on(&s, |shell| pick_preset(shell, i)));
    let s = Rc::downgrade(shell);
    window.on_move_slider(move |i, position| {
        on(&s, |shell| {
            shell.user_act(|app| {
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
            shell.user_act(|app| match app.performance_view().toggle_charge_limit() {
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
            shell.user_act(|app| match app.display_view()?.pick_native_mode(index(i)) {
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
            shell.user_act(|app| match app.keyboard_view()?.pick_swatch(index(i)) {
                Some(command) => app.on_keyboard(command),
                None => Ok(()),
            });
        });
    });
    let s = Rc::downgrade(shell);
    window.on_pick_custom(move |hex| {
        on(&s, |shell| {
            shell.user_act(|app| match app.keyboard_view()?.pick_custom(&hex) {
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
            shell.user_act(|app| match app.keyboard_view()?.toggle_logo() {
                Some(command) => app.on_keyboard(command),
                None => Ok(()),
            });
        });
    });
    let s = Rc::downgrade(shell);
    window.on_export_settings(move || on(&s, Shell::export_settings));
    let s = Rc::downgrade(shell);
    window.on_import_settings(move || on(&s, Shell::import_settings));
    let s = Rc::downgrade(shell);
    window.on_reset_settings(move || on(&s, |shell| shell.user_act(AppOn::<P>::reset_settings)));
    let s = Rc::downgrade(shell);
    window.on_toggle_vapor_light(move || {
        on(&s, |shell| {
            shell.user_act(|app| match app.keyboard_view()?.toggle_vapor_light() {
                Some(command) => app.on_keyboard(command),
                None => Ok(()),
            });
        });
    });
}
