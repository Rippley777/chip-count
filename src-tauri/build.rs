fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("native/source_access.m")
            .flag("-fobjc-arc")
            .compile("chip_source_access");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=UniformTypeIdentifiers");
        println!("cargo:rerun-if-changed=native/source_access.m");
    } else if std::env::var_os("CARGO_FEATURE_APP_STORE").is_some() {
        panic!("The app-store feature requires macOS");
    }
    tauri_build::build()
}
