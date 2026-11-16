//! Ticket 226 probe: the app's three windows with no controller, no tray and
//! no sensors, so nothing is sent to the laptop. Prints each step with its
//! time; a sampler outside reads the private memory. Same renderer as the
//! app: the build has only the software one.
#![allow(clippy::print_stdout, clippy::unwrap_used, clippy::type_complexity)]

use std::time::Duration;

use slint::{ComponentHandle, Timer, TimerMode};

slint::include_modules!();

/// How long each step holds, so the sampler reads it a few times.
const HOLD: Duration = Duration::from_secs(4);

fn say(step: &str) {
    println!(
        "{} step {step}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
}

fn main() {
    say("start");
    std::thread::sleep(HOLD);
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(|a| a.with_transparent(true).with_active(false))
        .select()
        .unwrap();
    say("backend");
    std::thread::sleep(HOLD);
    let window = MainWindow::new().unwrap();
    let flyout = FlyoutWindow::new().unwrap();
    let popup = ModePopup::new().unwrap();
    say("made");
    std::thread::sleep(HOLD);
    let steps: Vec<(&str, Box<dyn Fn()>)> = vec![
        ("loop", Box::new(|| {})),
        (
            "window shown",
            Box::new({
                let w = window.as_weak();
                move || w.unwrap().show().unwrap()
            }),
        ),
        (
            "window hidden",
            Box::new({
                let w = window.as_weak();
                move || w.unwrap().hide().unwrap()
            }),
        ),
        (
            "window shown 2",
            Box::new({
                let w = window.as_weak();
                move || w.unwrap().show().unwrap()
            }),
        ),
        (
            "window hidden 2",
            Box::new({
                let w = window.as_weak();
                move || w.unwrap().hide().unwrap()
            }),
        ),
        (
            "flyout shown",
            Box::new({
                let w = flyout.as_weak();
                move || w.unwrap().show().unwrap()
            }),
        ),
        (
            "flyout hidden",
            Box::new({
                let w = flyout.as_weak();
                move || w.unwrap().hide().unwrap()
            }),
        ),
        (
            "popup shown",
            Box::new({
                let w = popup.as_weak();
                move || w.unwrap().show().unwrap()
            }),
        ),
        (
            "popup hidden",
            Box::new({
                let w = popup.as_weak();
                move || w.unwrap().hide().unwrap()
            }),
        ),
        (
            "end",
            Box::new(|| {
                slint::quit_event_loop().unwrap();
            }),
        ),
    ];
    let next = std::cell::Cell::new(0);
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, HOLD, move || {
        let i = next.get();
        if let Some((name, step)) = steps.get(i) {
            say(name);
            step();
            next.set(i + 1);
        }
    });
    slint::run_event_loop_until_quit().unwrap();
}
