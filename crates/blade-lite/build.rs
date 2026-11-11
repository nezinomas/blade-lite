use std::path::Path;

// The tray's icon, drawn here too, for the exe's own icon.
#[allow(dead_code)]
#[path = "src/icon.rs"]
mod icon;

/// The exe icon's sizes: what Explorer and the taskbar ask for, up to 256.
const EXE_ICON_SIZES: [u16; 6] = [16, 24, 32, 48, 64, 256];

fn main() {
    // Debug info lets the window tests find controls by their labels. Debug
    // builds only, so the release exe stays as it was.
    let debug = std::env::var("PROFILE").is_ok_and(|profile| profile == "debug");
    let config = slint_build::CompilerConfiguration::new().with_debug_info(debug);
    slint_build::compile_with_config("ui/window.slint", config)
        .expect("the window markup compiles");
    exe_icon();
}

/// Builds the icon into the exe. The accent is read only when the app runs,
/// so the exe gets the Windows default blue.
fn exe_icon() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    let out = std::env::var("OUT_DIR").expect("Cargo sets OUT_DIR");
    let out = Path::new(&out);
    let ico = out.join("app.ico");
    std::fs::write(&ico, icon::ico(icon::DEFAULT_ACCENT, &EXE_ICON_SIZES))
        .expect("the icon file is written");
    // The resource compiler reads forward slashes; a backslash in quotes
    // would start an escape.
    let ico = ico.display().to_string().replace('\\', "/");
    let rc = out.join("app.rc");
    std::fs::write(&rc, format!("1 ICON \"{ico}\"\n")).expect("the resource file is written");
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_required()
        .expect("the icon is built into the exe");
}
