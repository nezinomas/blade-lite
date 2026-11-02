//! Throw-away spike: an empty Slint window with Mica and the accent colour.

use std::time::Duration;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::UI::ViewManagement::{UIColorType, UISettings};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DwmExtendFrameIntoClientArea,
    DwmSetWindowAttribute,
};
use windows::Win32::UI::Controls::MARGINS;

slint::slint! {
    import { VerticalBox, Button, Slider } from "std-widgets.slint";

    export component Spike inherits Window {
        in property <color> accent;
        title: "Mica spike";
        preferred-width: 480px;
        preferred-height: 320px;
        background: transparent;

        VerticalBox {
            Text {
                text: "Mica should show behind this text.";
                font-size: 16px;
            }
            Rectangle {
                height: 48px;
                border-radius: 6px;
                background: root.accent;
                Text { text: "Accent colour"; color: white; }
            }
            Slider { value: 40; }
            Button { text: "Primary button"; primary: true; }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .with_winit_window_attributes_hook(|attributes| attributes.with_transparent(true))
        .select()?;

    let ui = Spike::new()?;
    let accent = accent_colour()?;
    println!("accent colour: {accent:?}");
    ui.set_accent(accent);

    // The handle exists only after the event loop has run once.
    let weak = ui.as_weak();
    slint::Timer::single_shot(Duration::from_millis(100), move || {
        let ui = weak.unwrap();
        if std::env::var_os("MICA_OFF").is_some() {
            println!("Mica off");
            return;
        }
        match apply_mica(&ui) {
            Ok(()) => println!("Mica set"),
            Err(error) => println!("Mica failed: {error}"),
        }
    });

    ui.run()?;
    Ok(())
}

fn accent_colour() -> windows::core::Result<slint::Color> {
    let colour = UISettings::new()?.GetColorValue(UIColorType::Accent)?;
    Ok(slint::Color::from_argb_u8(colour.A, colour.R, colour.G, colour.B))
}

fn apply_mica(ui: &Spike) -> Result<(), Box<dyn std::error::Error>> {
    let handle = ui.window().window_handle();
    let RawWindowHandle::Win32(win32) = handle.window_handle()?.as_raw() else {
        return Err("not a Win32 window".into());
    };
    let hwnd = HWND(win32.hwnd.get() as *mut _);
    let backdrop = DWMSBT_MAINWINDOW;
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    // SAFETY: hwnd is this process's live window, and the value outlives the call.
    unsafe {
        DwmExtendFrameIntoClientArea(hwnd, &margins)?;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&raw const backdrop).cast(),
            size_of_val(&backdrop) as u32,
        )?;
    }
    Ok(())
}
