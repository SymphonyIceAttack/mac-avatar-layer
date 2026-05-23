use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=SYPHON_FRAMEWORK_DIR");
    println!("cargo:rerun-if-changed=public/Syphon.framework");

    if env::var_os("CARGO_FEATURE_SYPHON_OUTPUT").is_none() {
        return;
    }

    let framework_dir = env::var_os("SYPHON_FRAMEWORK_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"))
                .join("public")
                .join("Syphon.framework")
        });
    let framework_binary = framework_dir.join("Syphon");
    if !framework_binary.is_file() {
        panic!(
            "Syphon.framework not found.\n\nSet SYPHON_FRAMEWORK_DIR=/path/to/Syphon.framework or place it at public/Syphon.framework."
        );
    }

    let Some(parent) = framework_dir.parent() else {
        panic!(
            "SYPHON_FRAMEWORK_DIR must point to a Syphon.framework directory, got {}",
            framework_dir.display()
        );
    };

    println!("cargo:rustc-link-search=framework={}", parent.display());
    println!("cargo:rustc-link-lib=framework=Syphon");
}
