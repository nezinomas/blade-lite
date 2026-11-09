fn main() {
    // Debug info lets the window tests find controls by their labels. Debug
    // builds only, so the release exe stays as it was.
    let debug = std::env::var("PROFILE").is_ok_and(|profile| profile == "debug");
    let config = slint_build::CompilerConfiguration::new().with_debug_info(debug);
    slint_build::compile_with_config("ui/window.slint", config)
        .expect("the window markup compiles");
}
