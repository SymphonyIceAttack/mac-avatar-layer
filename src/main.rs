#[cfg(target_os = "macos")]
mod cubism;
#[cfg(target_os = "macos")]
mod live2d_model;
#[cfg(target_os = "macos")]
mod macos_app;
#[cfg(all(target_os = "macos", feature = "metal-renderer"))]
mod metal_renderer;
#[cfg(target_os = "macos")]
mod motion;
#[cfg(all(
    target_os = "macos",
    feature = "cubism-core",
    not(feature = "metal-renderer")
))]
mod software_renderer;

#[cfg(target_os = "macos")]
fn main() {
    let model_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "public/model/0.model3.json".to_string());

    if let Err(error) = macos_app::run(&model_path) {
        eprintln!("vtube-studio-rs failed to start: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("vtube-studio-rs currently targets macOS because the first milestone uses AppKit.");
    std::process::exit(1);
}
