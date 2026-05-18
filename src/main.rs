#[cfg(target_os = "macos")]
mod macos_app;

#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = macos_app::run() {
        eprintln!("vtube-studio-rs failed to start: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("vtube-studio-rs currently targets macOS because the first milestone uses AppKit.");
    std::process::exit(1);
}
