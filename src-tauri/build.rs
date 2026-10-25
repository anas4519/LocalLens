fn main() {
    // screencapturekit ships a small Swift bridge; point the loader at the
    // system Swift runtime so binaries and test executables can find it.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    tauri_build::build()
}
