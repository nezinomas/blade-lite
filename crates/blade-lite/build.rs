use std::path::Path;

// The tray's icon, drawn here too, for the exe's own icon.
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
    // The DLLs the exe imports load before `main`, so `limit_dll_search`
    // can't cover them: look for them only in System32 (ticket 224).
    println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
}

/// Builds the icon and the version into the exe. The accent is read only when the app runs,
/// so the exe gets the Windows default blue.
fn exe_icon() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    let out = std::env::var("OUT_DIR").expect("Cargo sets OUT_DIR");
    let out = Path::new(&out);
    std::fs::write(
        out.join("app.ico"),
        icon::ico(icon::DEFAULT_ACCENT, &EXE_ICON_SIZES),
    )
    .expect("the icon file is written");
    // A name, not a path: embed-resource gives the resource compiler OUT_DIR
    // to look in, so no folder name (any letters) goes in the file.
    let rc = out.join("app.rc");
    let resources = format!("1 ICON \"app.ico\"\n{}", version_info());
    std::fs::write(&rc, resources).expect("the resource file is written");
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_required()
        .expect("the icon and the version are built into the exe");
}

/// The exe's version resource, from the crate's version. The installer
/// reads its product version (ticket 249), so the two never differ.
fn version_info() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let numbers = format!(
        "{},{},{},0",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    format!(
        "1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
BEGIN
  BLOCK \"StringFileInfo\"
  BEGIN
    BLOCK \"040904B0\"
    BEGIN
      VALUE \"FileVersion\", \"{version}\"
      VALUE \"ProductVersion\", \"{version}\"
    END
  END
  BLOCK \"VarFileInfo\"
  BEGIN
    VALUE \"Translation\", 0x409, 1200
  END
END
"
    )
}
