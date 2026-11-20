//! Ticket 303 probe: how long the main window takes to open when it is
//! hidden (as now) and when it is made again, and whether a window made
//! again gets the old place and size. Only Slint windows: no controller, no
//! tray and no sensors, so nothing is sent to the laptop. Same backend,
//! renderer and attributes as the app. Prints each step with its time (for
//! the memory sampler of ticket 226) and what it measured. Ends by itself.
#![allow(clippy::print_stdout, clippy::unwrap_used)]

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize, Timer, TimerMode};

slint::include_modules!();

/// How long each step holds, so the window paints and the sampler reads it.
const HOLD: Duration = Duration::from_secs(3);

/// A place and size as a user drag could leave them, in pixels.
const MOVED: PhysicalPosition = PhysicalPosition { x: 160, y: 120 };
const RESIZED: PhysicalSize = PhysicalSize {
    width: 1400,
    height: 1500,
};

thread_local! {
    /// How many times the attributes hook ran: once for each window made.
    static HOOKS: Cell<u32> = const { Cell::new(0) };
}

/// One step of the probe, run by the timer.
type Step = Box<dyn Fn(&Rc<Probe>)>;

fn say(step: &str) {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    println!("{ms} step {step}");
}

/// The window, and when its last show was asked, so its first paint after
/// it can be timed.
struct Probe {
    window: std::cell::RefCell<MainWindow>,
    shown_at: Cell<Option<Instant>>,
}

impl Probe {
    fn window(&self) -> MainWindow {
        self.window.borrow().clone_strong()
    }

    /// Times the next paint after a show.
    fn watch(self: &Rc<Self>) {
        let probe = Rc::downgrade(self);
        self.window()
            .window()
            .on_winit_window_event(move |_, event| {
                if matches!(event, winit::event::WindowEvent::RedrawRequested)
                    && let Some(probe) = probe.upgrade()
                    && let Some(at) = probe.shown_at.take()
                {
                    println!(
                        "measured first paint {} ms after show()",
                        at.elapsed().as_millis()
                    );
                }
                EventResult::Propagate
            });
    }

    fn show(&self, name: &str) {
        say(name);
        let at = Instant::now();
        self.shown_at.set(Some(at));
        self.window().show().unwrap();
        println!("measured show() {} ms", at.elapsed().as_millis());
    }

    fn place(&self, when: &str) {
        let window = self.window();
        let window = window.window();
        let position = window.position();
        let size = window.size();
        println!(
            "measured {when}: position {} {}, size {} x {}, hooks run {}",
            position.x,
            position.y,
            size.width,
            size.height,
            HOOKS.get()
        );
    }
}

fn main() {
    say("start");
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(|attributes| {
            HOOKS.set(HOOKS.get() + 1);
            attributes.with_transparent(true).with_active(false)
        })
        .select()
        .unwrap();
    let made = Instant::now();
    let probe = Rc::new(Probe {
        window: std::cell::RefCell::new(MainWindow::new().unwrap()),
        shown_at: Cell::new(None),
    });
    println!(
        "measured MainWindow::new() {} ms",
        made.elapsed().as_millis()
    );
    probe.watch();
    say("made");

    let steps: Vec<Step> = vec![
        Box::new(|probe| probe.show("shown first")),
        Box::new(|probe| {
            say("moved");
            let window = probe.window();
            window.window().set_position(MOVED);
            window.window().set_size(RESIZED);
        }),
        Box::new(|probe| {
            probe.place("before the hide");
            say("hidden");
            probe.window().hide().unwrap();
        }),
        Box::new(|probe| probe.show("shown again, kept")),
        Box::new(|probe| {
            say("hidden 2");
            probe.window().hide().unwrap();
        }),
        Box::new(|probe| {
            let old = probe.window();
            let position = old.window().position();
            let size = old.window().size();
            drop(old);
            let made = Instant::now();
            let new = MainWindow::new().unwrap();
            println!(
                "measured MainWindow::new() again {} ms",
                made.elapsed().as_millis()
            );
            new.window().set_position(position);
            new.window().set_size(size);
            // The old window is dropped here, with its picture.
            probe.window.replace(new);
            probe.watch();
            say("made again");
        }),
        Box::new(|probe| probe.show("shown, made again")),
        Box::new(|probe| {
            probe.place("after the show of the window made again");
            say("hidden 3");
            probe.window().hide().unwrap();
        }),
        Box::new(|_| {
            say("end");
            slint::quit_event_loop().unwrap();
        }),
    ];
    let next = Cell::new(0);
    let timer = Timer::default();
    let ticking = Rc::clone(&probe);
    timer.start(TimerMode::Repeated, HOLD, move || {
        let i = next.get();
        if let Some(step) = steps.get(i) {
            step(&ticking);
            next.set(i + 1);
        }
    });
    slint::run_event_loop_until_quit().unwrap();
}
