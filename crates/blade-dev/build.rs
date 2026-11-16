fn main() {
    // The DLLs the exe imports load before `main`, so `limit_dll_search`
    // can't cover them: look for them only in System32 (ticket 224).
    println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
}
