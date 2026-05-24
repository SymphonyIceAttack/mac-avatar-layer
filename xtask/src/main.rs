use std::{
    collections::HashMap,
    env,
    ffi::CStr,
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    os::raw::{c_char, c_uint, c_void},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use image::{Rgba, RgbaImage};
use serde::Deserialize;
use sysinfo::{Process, ProcessesToUpdate, Signal, System};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const DEVELOPMENT_CONFIG_PATH: &str = "vtube-studio-rs.dev.toml";
const DEVELOPMENT_EXAMPLE_CONFIG_PATH: &str = "vtube-studio-rs.dev.example.toml";
const BUILD_CONFIG_PATH: &str = "vtube-studio-rs.build.toml";
const BUILD_EXAMPLE_CONFIG_PATH: &str = "vtube-studio-rs.build.example.toml";
const DEV_CAMERA_BUNDLE_ID: &str = "rs.vtube-studio.dev";
const VIRTUAL_CAMERA_NAME: &str = "VTube Studio RS Camera";
const VIRTUAL_CAMERA_EXTENSION_BUNDLE_ID: &str = "rs.vtube-studio.dev.CameraExtension";
const VIRTUAL_CAMERA_MACH_SERVICE: &str = "rs.vtube-studio.dev.CameraExtension";
const VIRTUAL_CAMERA_APP_GROUP: &str = "group.rs.vtube-studio.dev";
const VIRTUAL_CAMERA_BUNDLE_NAME: &str = "VTube Studio RS Camera.systemextension";
const DEV_APP_BUNDLE_NAME: &str = "vtube-studio-rs Dev.app";
const CONTAINER_PROVISION_PROFILE_ENV: &str = "VTUBE_RS_CONTAINER_PROVISION_PROFILE";
const CAMERA_EXTENSION_PROVISION_PROFILE_ENV: &str = "VTUBE_RS_CAMERA_EXTENSION_PROVISION_PROFILE";
const DEFAULT_CONTAINER_PROVISION_PROFILE: &str =
    "public/provisioning/ContainerApp.provisionprofile";
const DEFAULT_CAMERA_EXTENSION_PROVISION_PROFILE: &str =
    "public/provisioning/CameraExtension.provisionprofile";
const APPLE_WWDR_G3_URL: &str = "https://www.apple.com/certificateauthority/AppleWWDRCAG3.cer";
const APPLE_WWDR_G3_SHA1: &str = "06EC06599F4ED0027CC58956B4D3AC1255114F35";

#[derive(Debug, Clone, Copy)]
enum ProvisioningProfileKind {
    ContainerApp,
    CameraExtension,
}

impl ProvisioningProfileKind {
    fn label(self) -> &'static str {
        match self {
            Self::ContainerApp => "container app",
            Self::CameraExtension => "Camera Extension",
        }
    }

    fn expected_bundle_id(self) -> &'static str {
        match self {
            Self::ContainerApp => DEV_CAMERA_BUNDLE_ID,
            Self::CameraExtension => VIRTUAL_CAMERA_EXTENSION_BUNDLE_ID,
        }
    }

    fn requires_system_extension_install(self) -> bool {
        matches!(self, Self::ContainerApp)
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("build-app") => build_app(args.collect()),
        Some("build-camera-extension") => build_camera_extension(args.collect()),
        Some("clean") => clean(args.collect()),
        Some("capture-mask-matrix") => capture_mask_matrix(args.collect()),
        Some("capture-offscreen-matrix") => capture_offscreen_matrix(args.collect()),
        Some("capture-full-matrix") => capture_full_matrix(args.collect()),
        Some("capture-metal") => capture_metal(args.collect()),
        Some("camera-extension-plan") => camera_extension_plan(args.collect()),
        Some("capture-quality-matrix") => capture_quality_matrix(args.collect()),
        Some("capture-risk-models") => capture_risk_models(args.collect()),
        Some("capture-rice-stress") => capture_rice_stress(args.collect()),
        Some("configure-internal-output") => configure_internal_output(args.collect()),
        Some("configure-obs-recording") => configure_obs_recording(args.collect()),
        Some("doctor") => doctor(args.collect()),
        Some("fix-wwdr-cert") => fix_wwdr_cert(args.collect()),
        Some("list-models") => list_models(args.collect()),
        Some("mao-mask-audit") => mao_mask_audit(args.collect()),
        Some("provision-camera-profiles") => provision_camera_profiles(args.collect()),
        Some("probe-risk-models") => probe_risk_models(args.collect()),
        Some("quality-visual-diff") => quality_visual_diff(args.collect()),
        Some("ren-visual-diff") => ren_visual_diff(args.collect()),
        Some("ren-offscreen-audit") => ren_offscreen_audit(args.collect()),
        Some("render-regression-report") => render_regression_report(args.collect()),
        Some("rice-stress-audit") => rice_stress_audit(args.collect()),
        Some("run-metal") => run_metal(args.collect()),
        Some("run-space-test") => run_space_test(args.collect()),
        Some("sample-compatibility-sweep") => sample_compatibility_sweep(args.collect()),
        Some("select-model") => select_model(args.collect()),
        Some("tune-input") => tune_input(args.collect()),
        Some("virtual-camera-readiness") => virtual_camera_readiness(args.collect()),
        Some("help") | Some("--help") | Some("-h") | None => {
            print_help();
            Ok(())
        }
        Some(command) => Err(format!("unknown xtask command: {command}").into()),
    }
}

fn print_help() {
    println!(
        "\
vtube-studio-rs xtask

Usage:
  cargo xtask build-app [--release]
  cargo xtask build-camera-extension [--dev|--build]
  cargo xtask clean [--generated|--all]
  cargo xtask camera-extension-plan [--dev|--build]
  cargo xtask capture-full-matrix
  cargo xtask capture-metal [MODEL_PATH]
  cargo xtask capture-mask-matrix [MODEL_PATH]
  cargo xtask capture-offscreen-matrix [MODEL_PATH]
  cargo xtask capture-quality-matrix [MODEL_PATH ...]
  cargo xtask capture-risk-models [MODEL_PATH ...]
  cargo xtask capture-rice-stress [MODEL_PATH]
  cargo xtask configure-internal-output [--dev|--build]
  cargo xtask configure-obs-recording [--dev|--build] [--desktop|--offscreen]
  cargo xtask doctor
  cargo xtask fix-wwdr-cert
  cargo xtask list-models [MODEL_OR_DIR ...]
  cargo xtask mao-mask-audit [MODEL_PATH]
  cargo xtask provision-camera-profiles [--from DIR] [--force]
  cargo xtask probe-risk-models [MODEL_OR_DIR ...]
  cargo xtask quality-visual-diff
  cargo xtask ren-visual-diff
  cargo xtask ren-offscreen-audit [MODEL_PATH]
  cargo xtask render-regression-report
  cargo xtask rice-stress-audit [MODEL_PATH]
  cargo xtask run-metal [--release] [MODEL_PATH]
  cargo xtask run-space-test [MODEL_PATH]
  cargo xtask sample-compatibility-sweep [SAMPLES_ROOT]
  cargo xtask select-model [--dev|--build] MODEL_PATH
  cargo xtask tune-input [--dev|--build] <mouse|mouth|camera> <soft|normal|expressive>
  cargo xtask virtual-camera-readiness [--dev|--build]

Commands:
  build-app          Build and sign the local macOS .app wrapper without launching it.
  build-camera-extension
                     Build the CoreMediaIO Camera Extension prototype bundle.
  clean              Remove generated target artifacts; --all also runs cargo clean.
  camera-extension-plan
                     Write CoreMediaIO Camera Extension prototype templates and plan.
  capture-full-matrix
                     Run the complete render regression capture and report matrix.
  capture-metal     Capture the Metal renderer window to target/render-regression.
  capture-mask-matrix
                     Capture shared/high-precision/no-mask screenshots for Mao.
  capture-offscreen-matrix
                     Capture shared/high-precision/no-mask screenshots for Ren.
  capture-quality-matrix
                     Capture mipmaps/anisotropy screenshots for default, Mao, and Ren.
  capture-risk-models
                     Capture baseline screenshots for default, Mao, and Ren.
  capture-rice-stress
                     Capture shared/high-precision/no-mask screenshots for Rice.
  configure-obs-recording
                     Write a transparent-window OBS Window Capture preset to dev/build config.
  configure-internal-output
                     Write the system camera source preset with IOSurface, preview, and activation enabled.
  doctor            Check local configs, selected models, settings, and Cubism Core SDK paths.
  fix-wwdr-cert     Install Apple's current WWDR G3 intermediate and re-check codesigning.
  list-models       List local .model3.json files and resource counts.
  mao-mask-audit     Generate target/render-regression/mao-mask-audit.md.
  provision-camera-profiles
                     Copy matching Apple provisioning profiles into public/provisioning.
  probe-risk-models  Generate target/render-regression/probe.txt through the Rust model probe.
  quality-visual-diff
                     Generate target/render-regression/quality-visual-diff.md.
  ren-visual-diff   Generate target/render-regression/ren-visual-diff.md.
  ren-offscreen-audit
                     Generate target/render-regression/ren-offscreen-audit.md.
  render-regression-report
                     Generate target/render-regression/report.md.
  rice-stress-audit Generate target/render-regression/rice-stress-audit.md.
  run-metal         Run the Metal renderer with local Cubism Core env; --release uses the build config.
  run-space-test    Run Space/display reliability test and write a Markdown report.
  sample-compatibility-sweep
                     Generate target/render-regression/compatibility-sweep.md.
  select-model      Write [model].path in the dev/build local config.
  tune-input        Write persistent mouse, mouth, or camera calibration preset values.
  virtual-camera-readiness
                     Write target/virtual-camera/readiness.md for the future in-project macOS virtual camera path.
"
    );
}

fn clean(args: Vec<String>) -> Result<()> {
    let mode = args.first().map(String::as_str).unwrap_or("--generated");
    if args.len() > 1 || (mode != "--generated" && mode != "--all") {
        return Err("usage: cargo xtask clean [--generated|--all]".into());
    }

    let root = project_root()?;
    terminate_app_processes(&root);

    let target = root.join("target");
    remove_path(target.join("render-regression"))?;
    remove_path(target.join("space-test"))?;
    remove_path(target.join("space-test-smoke"))?;
    remove_path(target.join("space-test-live.out"))?;
    remove_path(target.join("space-test-live.pid"))?;
    remove_path(target.join("space-test-smoke.out"))?;
    remove_path(target.join("camera-test"))?;
    remove_path(target.join("virtual-camera"))?;
    remove_path(target.join("codesign"))?;
    remove_path(target.join("dev-app"))?;
    remove_path(target.join("vtube-studio-rs.pid"))?;

    if mode == "--all" {
        run_status(
            Command::new("cargo")
                .arg("clean")
                .current_dir(&root)
                .stdin(Stdio::null()),
        )?;
    }

    println!("Cleaned target artifacts ({mode}).");
    Ok(())
}

fn capture_full_matrix(args: Vec<String>) -> Result<()> {
    if !args.is_empty() {
        return Err("usage: cargo xtask capture-full-matrix".into());
    }

    let root = project_root()?;
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression"));
    fs::create_dir_all(&output_dir)?;

    clean(vec!["--generated".to_string()])?;
    fs::create_dir_all(&output_dir)?;

    let options = CaptureRunOptions {
        clean_before: false,
        report_after: false,
    };
    let default_model = "public/model/0.model3.json".to_string();
    let mao_model = "public/CubismSdkForNative/Samples/Resources/Mao/Mao.model3.json".to_string();
    let ren_model = "public/CubismSdkForNative/Samples/Resources/Ren/Ren.model3.json".to_string();
    let rice_model =
        "public/CubismSdkForNative/Samples/Resources/Rice/Rice.model3.json".to_string();
    let rice_exists = model_exists(&root, &rice_model);

    run_full_step("Risk model sweep", || {
        capture_risk_models_to(
            &root,
            vec![default_model.clone(), mao_model.clone(), ren_model.clone()],
            output_dir.clone(),
            options,
        )
    })?;
    run_full_step("Mao mask matrix", || {
        capture_mask_mode_matrix_to(
            &root,
            &mao_model,
            output_dir.join("mask-matrix"),
            "mask",
            "Mask matrix screenshots",
            options,
        )
    })?;
    run_full_step("Ren offscreen matrix", || {
        capture_mask_mode_matrix_to(
            &root,
            &ren_model,
            output_dir.join("offscreen-matrix"),
            "offscreen",
            "Offscreen matrix screenshots",
            options,
        )
    })?;

    if rice_exists {
        run_full_step("Rice optional stress matrix", || {
            capture_rice_stress_matrix_to(
                &root,
                &rice_model,
                output_dir.join("rice-stress"),
                options,
            )
        })?;
    } else {
        println!("\n==> Rice optional stress matrix");
        println!("Skipping missing optional Rice sample model.");
    }

    run_full_step("Texture quality matrix", || {
        capture_quality_mode_matrix_to(
            &root,
            vec![default_model.clone(), mao_model.clone(), ren_model.clone()],
            output_dir.join("quality-matrix"),
            options,
        )
    })?;

    terminate_app_processes(&root);
    let mut probe_models = vec![default_model, mao_model, ren_model];
    if rice_exists {
        probe_models.push(rice_model.clone());
    }

    run_full_step("Combined model risk probe", || {
        let probe_path = output_dir.join("probe.txt");
        let existing_models = existing_models(&root, probe_models.clone());
        if existing_models.is_empty() {
            eprintln!("Skipping model probe because no configured models were found.");
            Ok(())
        } else {
            run_model_probe(&root, &existing_models, &probe_path)
        }
    })?;
    run_full_step("Mao mask audit", || mao_mask_audit(vec![]))?;
    run_full_step("Ren offscreen audit", || ren_offscreen_audit(vec![]))?;
    run_full_step("Ren visual diff", || ren_visual_diff(vec![]))?;
    if rice_exists {
        run_full_step("Rice optional stress audit", || rice_stress_audit(vec![]))?;
    }
    run_full_step("Quality visual diff", || quality_visual_diff(vec![]))?;

    println!();
    run_render_regression_report_safe(&root)?;
    Ok(())
}

fn capture_metal(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err("usage: cargo xtask capture-metal [MODEL_PATH]".into());
    }

    let root = project_root()?;
    let model_path = args
        .first()
        .map(String::as_str)
        .unwrap_or("public/model/0.model3.json");
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression"));
    let output_path = capture_metal_to(&root, model_path, &output_dir)?;
    println!("{}", output_path.display());
    Ok(())
}

fn capture_mask_matrix(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err("usage: cargo xtask capture-mask-matrix [MODEL_PATH]".into());
    }

    capture_mask_mode_matrix(
        args.first()
            .map(String::as_str)
            .unwrap_or("public/CubismSdkForNative/Samples/Resources/Mao/Mao.model3.json"),
        "target/render-regression/mask-matrix",
        "mask",
        "Mask matrix screenshots",
    )
}

fn capture_offscreen_matrix(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err("usage: cargo xtask capture-offscreen-matrix [MODEL_PATH]".into());
    }

    capture_mask_mode_matrix(
        args.first()
            .map(String::as_str)
            .unwrap_or("public/CubismSdkForNative/Samples/Resources/Ren/Ren.model3.json"),
        "target/render-regression/offscreen-matrix",
        "offscreen",
        "Offscreen matrix screenshots",
    )
}

fn capture_quality_matrix(args: Vec<String>) -> Result<()> {
    let models = if args.is_empty() {
        vec![
            "public/model/0.model3.json".to_string(),
            "public/CubismSdkForNative/Samples/Resources/Mao/Mao.model3.json".to_string(),
            "public/CubismSdkForNative/Samples/Resources/Ren/Ren.model3.json".to_string(),
        ]
    } else {
        args
    };

    capture_quality_mode_matrix(models)
}

fn capture_risk_models(args: Vec<String>) -> Result<()> {
    let models = if args.is_empty() {
        vec![
            "public/model/0.model3.json".to_string(),
            "public/CubismSdkForNative/Samples/Resources/Mao/Mao.model3.json".to_string(),
            "public/CubismSdkForNative/Samples/Resources/Ren/Ren.model3.json".to_string(),
        ]
    } else {
        args
    };

    let root = project_root()?;
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression"));
    capture_risk_models_to(&root, models, output_dir, capture_options_from_env())
}

fn capture_rice_stress(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err("usage: cargo xtask capture-rice-stress [MODEL_PATH]".into());
    }

    let model_path = args
        .first()
        .map(String::as_str)
        .unwrap_or("public/CubismSdkForNative/Samples/Resources/Rice/Rice.model3.json");
    capture_rice_stress_matrix(model_path)
}

fn doctor(args: Vec<String>) -> Result<()> {
    if !args.is_empty() {
        return Err("usage: cargo xtask doctor".into());
    }

    let root = project_root()?;
    println!("vtube-studio-rs doctor");
    println!("Project: {}", root.display());
    println!();

    let mut issues = 0usize;
    let development_check = check_local_config(&root, SelectModelTarget::Development)?;
    let build_check = check_local_config(&root, SelectModelTarget::Build)?;
    issues += development_check.issues + build_check.issues;
    issues += check_cubism_core_sdk(&root);

    println!();
    if issues == 0 {
        println!("Doctor result: ok");
        Ok(())
    } else {
        Err(format!("doctor found {issues} issue(s)").into())
    }
}

fn fix_wwdr_cert(args: Vec<String>) -> Result<()> {
    if !args.is_empty() {
        return Err("usage: cargo xtask fix-wwdr-cert".into());
    }
    if env::consts::OS != "macos" {
        return Err("cargo xtask fix-wwdr-cert is only available on macOS.".into());
    }

    let root = project_root()?;
    let cert_dir = root.join("target/codesign");
    fs::create_dir_all(&cert_dir)?;
    let cert_path = cert_dir.join("AppleWWDRCAG3.cer");

    println!("Downloading Apple WWDR G3 intermediate certificate...");
    run_status(
        Command::new("curl")
            .arg("-fsSL")
            .arg("-o")
            .arg(&cert_path)
            .arg(APPLE_WWDR_G3_URL)
            .current_dir(&root)
            .stdin(Stdio::null()),
    )?;

    println!(
        "Installing Apple WWDR G3 into login keychain ({})...",
        relative_display(&root, &cert_path)
    );
    add_certificate_to_login_keychain(&cert_path)?;

    println!("Checking local codesigning identities...");
    let identity_output = security_find_identity(&[])?;
    let codesign_output = security_find_identity(&["-v", "-p", "codesigning"])?;
    if let Some(identity) = detect_codesign_identity() {
        println!("Codesigning identity is ready: {identity}");
        println!("Next: cargo xtask build-app --release");
        return Ok(());
    }

    if find_untrusted_codesign_identity_line(&identity_output).is_some() {
        return Err(format!(
            "Apple codesigning identity is still not trusted after installing WWDR G3.\n\
Open Keychain Access, search for `Apple Worldwide Developer Relations Certification Authority`, \
remove the expired 2023 WWDR intermediate if present, keep the G3 certificate \
with SHA-1 {APPLE_WWDR_G3_SHA1}, and leave trust set to `Use System Defaults`.\n\n\
security find-identity:\n{identity_output}\n\
security find-identity -v -p codesigning:\n{codesign_output}"
        )
        .into());
    }

    Err(format!(
        "Apple WWDR G3 was installed, but no valid Apple codesigning identity was found.\n\
Create or download an Apple Development certificate in Xcode, then run this command again.\n\n\
security find-identity:\n{identity_output}\n\
security find-identity -v -p codesigning:\n{codesign_output}"
    )
    .into())
}

#[derive(Debug, Clone)]
struct ProvisionCameraProfilesOptions {
    source_dirs: Vec<PathBuf>,
    force: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ObsWindowPlacement {
    Desktop,
    Offscreen,
}

impl ObsWindowPlacement {
    fn origin(self) -> (f64, f64) {
        match self {
            Self::Desktop => (100.0, 140.0),
            Self::Offscreen => (-20000.0, 140.0),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Desktop => "transparent desktop window",
            Self::Offscreen => "transparent offscreen window",
        }
    }
}

fn provision_camera_profiles(args: Vec<String>) -> Result<()> {
    if env::consts::OS != "macos" {
        return Err("cargo xtask provision-camera-profiles is only available on macOS.".into());
    }
    let options = parse_provision_camera_profiles_args(args)?;
    let root = project_root()?;
    let target_dir = root.join("public/provisioning");
    fs::create_dir_all(&target_dir)?;

    println!("Scanning provisioning profiles in:");
    for source_dir in &options.source_dirs {
        println!("  - {}", source_dir.display());
    }
    let container_dest = root.join(DEFAULT_CONTAINER_PROVISION_PROFILE);
    let extension_dest = root.join(DEFAULT_CAMERA_EXTENSION_PROVISION_PROFILE);

    let container = ensure_camera_profile(
        &root,
        &options.source_dirs,
        &container_dest,
        ProvisioningProfileKind::ContainerApp,
        options.force,
    )?;
    let extension = ensure_camera_profile(
        &root,
        &options.source_dirs,
        &extension_dest,
        ProvisioningProfileKind::CameraExtension,
        options.force,
    )?;

    println!();
    println!("Provisioning profile setup complete.");
    println!("Container app: {}", relative_display(&root, &container));
    println!("Camera Extension: {}", relative_display(&root, &extension));
    println!("Next: cargo xtask build-app --release");
    Ok(())
}

fn parse_provision_camera_profiles_args(
    args: Vec<String>,
) -> Result<ProvisionCameraProfilesOptions> {
    let mut source_dirs = Vec::new();
    let mut force = false;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--from" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    return Err(
                        "usage: cargo xtask provision-camera-profiles [--from DIR] [--force]"
                            .into(),
                    );
                };
                source_dirs.push(PathBuf::from(value));
            }
            "--force" => {
                force = true;
            }
            value => {
                return Err(format!(
                    "unknown provision-camera-profiles option: {value}\nusage: cargo xtask provision-camera-profiles [--from DIR] [--force]"
                )
                .into());
            }
        }
        index += 1;
    }
    if source_dirs.is_empty() {
        source_dirs = default_provisioning_profile_source_dirs();
    }
    Ok(ProvisionCameraProfilesOptions { source_dirs, force })
}

fn default_provisioning_profile_source_dirs() -> Vec<PathBuf> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"));
    vec![
        home.join("Library/MobileDevice/Provisioning Profiles"),
        home.join("Library/Developer/Xcode/UserData/Provisioning Profiles"),
    ]
}

fn ensure_camera_profile(
    root: &Path,
    source_dirs: &[PathBuf],
    destination: &Path,
    kind: ProvisioningProfileKind,
    force: bool,
) -> Result<PathBuf> {
    if destination.is_file() && !force && validate_provisioning_profile(destination, kind).is_ok() {
        println!(
            "{} profile already exists and is valid: {}",
            kind.label(),
            relative_display(root, destination)
        );
        return Ok(destination.to_path_buf());
    }

    let candidate = find_matching_provisioning_profile(source_dirs, kind)?.ok_or_else(|| {
        let sources = source_dirs
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "no matching {} provisioning profile found in [{}].\n\
Create/download profiles for `{}` in Apple Developer/Xcode, or pass the download folder with `--from DIR`.",
            kind.label(),
            sources,
            kind.expected_bundle_id()
        )
    })?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&candidate, destination).map_err(|error| {
        format!(
            "failed to copy {} profile from {} to {}: {error}",
            kind.label(),
            candidate.display(),
            destination.display()
        )
    })?;
    let summary = validate_provisioning_profile(destination, kind)?;
    println!(
        "Copied {} profile: {} -> {}",
        kind.label(),
        candidate.display(),
        relative_display(root, destination)
    );
    println!(
        "Validated {} profile: name={} app_id={} team={}",
        kind.label(),
        summary.name.as_deref().unwrap_or("unknown"),
        summary.application_identifier,
        summary.team_identifier.as_deref().unwrap_or("unknown")
    );
    Ok(destination.to_path_buf())
}

fn find_matching_provisioning_profile(
    source_dirs: &[PathBuf],
    kind: ProvisioningProfileKind,
) -> Result<Option<PathBuf>> {
    let mut candidates = Vec::new();
    for source_dir in source_dirs {
        if source_dir.is_dir() {
            collect_provisioning_profile_paths(source_dir, &mut candidates)?;
        }
    }
    candidates.sort();
    for candidate in candidates {
        if validate_provisioning_profile(&candidate, kind).is_ok() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn collect_provisioning_profile_paths(dir: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_provisioning_profile_paths(&path, output)?;
            continue;
        }
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if matches!(extension, "provisionprofile" | "mobileprovision") {
            output.push(path);
        }
    }
    Ok(())
}

fn add_certificate_to_login_keychain(cert_path: &Path) -> Result<()> {
    let keychain = dirs_home_keychain();
    let output = Command::new("security")
        .arg("add-certificates")
        .arg("-k")
        .arg(&keychain)
        .arg(cert_path)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        println!("Installed Apple WWDR G3 certificate.");
        return Ok(());
    }
    let combined = format!("{stdout}{stderr}");
    if combined.contains("already in") {
        println!("Apple WWDR G3 certificate is already installed.");
        return Ok(());
    }
    Err(format!(
        "failed to install Apple WWDR G3 certificate into {} with status {}\nstdout:\n{}\nstderr:\n{}",
        keychain.display(),
        output.status,
        stdout,
        stderr
    )
    .into())
}

fn dirs_home_keychain() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library/Keychains/login.keychain-db")
}

fn security_find_identity(args: &[&str]) -> Result<String> {
    let output = Command::new("security")
        .arg("find-identity")
        .args(args)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        Ok(stdout.into_owned())
    } else {
        Err(format!(
            "security find-identity failed with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status, stdout, stderr
        )
        .into())
    }
}

fn list_models(args: Vec<String>) -> Result<()> {
    let root = project_root()?;
    let roots = if args.is_empty() {
        vec!["public".to_string()]
    } else {
        args
    };

    let mut model_paths = Vec::new();
    for item in &roots {
        collect_model3_paths(&root.join(item), &mut model_paths)?;
    }
    model_paths.sort();
    model_paths.dedup();

    if model_paths.is_empty() {
        return Err(format!("no .model3.json files found under: {}", roots.join(", ")).into());
    }

    println!("Found {} Live2D model(s):", model_paths.len());
    println!(
        "{:<18} {:>3} {:>3} {:>4} {:>4} {:>7} {}",
        "name", "tex", "mot", "expr", "phys", "display", "path"
    );

    let mut failures = 0usize;
    for path in model_paths {
        match ModelManifestSummary::load(&path) {
            Ok(summary) => {
                println!(
                    "{:<18} {:>3} {:>3} {:>4} {:>4} {:>7} {}",
                    summary.name,
                    summary.texture_count,
                    summary.motion_count,
                    summary.expression_count,
                    yes_no(summary.has_physics),
                    yes_no(summary.has_display_info),
                    relative_display(&root, &summary.path)
                );
            }
            Err(error) => {
                failures += 1;
                println!(
                    "{:<18} {:>3} {:>3} {:>4} {:>4} {:>7} {}",
                    model_name_from_path(&path.to_string_lossy()),
                    "-",
                    "-",
                    "-",
                    "-",
                    "-",
                    relative_display(&root, &path)
                );
                eprintln!("  failed to read {}: {error}", path.display());
            }
        }
    }

    if failures > 0 {
        return Err(format!("{failures} model manifest(s) could not be read").into());
    }

    Ok(())
}

fn mao_mask_audit(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err("usage: cargo xtask mao-mask-audit [MODEL_PATH]".into());
    }

    let root = project_root()?;
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression"));
    fs::create_dir_all(&output_dir)?;

    let model_path = args.first().cloned().unwrap_or_else(|| {
        "public/CubismSdkForNative/Samples/Resources/Mao/Mao.model3.json".to_string()
    });
    let probe_path = env::var_os("PROBE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("mao-mask-audit-probe.txt"));
    let report_path = env::var_os("REPORT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("mao-mask-audit.md"));

    run_model_probe(&root, std::slice::from_ref(&model_path), &probe_path)?;
    let probe = fs::read_to_string(&probe_path)?;
    let report = mao_mask_audit_report(&root, &output_dir, &model_path, &probe_path, &probe);
    fs::write(&report_path, report)?;

    println!("{}", report_path.display());
    Ok(())
}

fn probe_risk_models(args: Vec<String>) -> Result<()> {
    let root = project_root()?;
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression"));
    fs::create_dir_all(&output_dir)?;
    let probe_path = env::var_os("PROBE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("probe.txt"));

    let roots = if args.is_empty() {
        vec![
            "public/model".to_string(),
            "public/CubismSdkForNative/Samples/Resources/Mao".to_string(),
            "public/CubismSdkForNative/Samples/Resources/Ren".to_string(),
        ]
    } else {
        args
    };

    run_model_probe(&root, &roots, &probe_path)?;

    println!("{}", probe_path.display());
    Ok(())
}

fn quality_visual_diff(args: Vec<String>) -> Result<()> {
    if !args.is_empty() {
        return Err("usage: cargo xtask quality-visual-diff".into());
    }

    let root = project_root()?;
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression"));
    let matrix_dir = env::var_os("MATRIX_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("quality-matrix"));
    let diff_dir = env::var_os("DIFF_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("quality-visual-diff"));
    let report_path = env::var_os("REPORT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("quality-visual-diff.md"));

    fs::create_dir_all(&diff_dir)?;
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let report = quality_visual_diff_report(&root, &output_dir, &matrix_dir, &diff_dir)?;
    fs::write(&report_path, report)?;

    println!("{}", report_path.display());
    Ok(())
}

fn ren_visual_diff(args: Vec<String>) -> Result<()> {
    if !args.is_empty() {
        return Err("usage: cargo xtask ren-visual-diff".into());
    }

    let root = project_root()?;
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression"));
    let matrix_dir = env::var_os("MATRIX_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("offscreen-matrix"));
    let diff_dir = env::var_os("DIFF_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("ren-visual-diff"));
    let report_path = env::var_os("REPORT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("ren-visual-diff.md"));

    let shared_image = env::var_os("SHARED_IMAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|| matrix_dir.join("latest-Ren-shared.png"));
    let high_precision_image = env::var_os("HIGH_PRECISION_IMAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|| matrix_dir.join("latest-Ren-high-precision.png"));
    let no_mask_image = env::var_os("NO_MASK_IMAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|| matrix_dir.join("latest-Ren-no-mask.png"));

    require_file(
        &shared_image,
        "Missing Ren shared screenshot. Run cargo xtask capture-offscreen-matrix first.",
    )?;
    require_file(
        &high_precision_image,
        "Missing Ren high-precision screenshot. Run cargo xtask capture-offscreen-matrix first.",
    )?;
    require_file(
        &no_mask_image,
        "Missing Ren no-mask screenshot. Run cargo xtask capture-offscreen-matrix first.",
    )?;

    fs::create_dir_all(&diff_dir)?;
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let report = ren_visual_diff_report(
        &root,
        &output_dir,
        &diff_dir,
        &shared_image,
        &high_precision_image,
        &no_mask_image,
    )?;
    fs::write(&report_path, report)?;

    println!("{}", report_path.display());
    Ok(())
}

fn ren_offscreen_audit(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err("usage: cargo xtask ren-offscreen-audit [MODEL_PATH]".into());
    }

    let root = project_root()?;
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression"));
    fs::create_dir_all(&output_dir)?;

    let model_path = args.first().cloned().unwrap_or_else(|| {
        "public/CubismSdkForNative/Samples/Resources/Ren/Ren.model3.json".to_string()
    });
    let probe_path = env::var_os("PROBE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("ren-offscreen-audit-probe.txt"));
    let report_path = env::var_os("REPORT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("ren-offscreen-audit.md"));

    run_model_probe(&root, std::slice::from_ref(&model_path), &probe_path)?;
    let probe = fs::read_to_string(&probe_path)?;
    let report = ren_offscreen_audit_report(&root, &output_dir, &model_path, &probe_path, &probe);
    fs::write(&report_path, report)?;

    println!("{}", report_path.display());
    Ok(())
}

fn render_regression_report(args: Vec<String>) -> Result<()> {
    if !args.is_empty() {
        return Err("usage: cargo xtask render-regression-report".into());
    }

    let root = project_root()?;
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression"));
    fs::create_dir_all(&output_dir)?;
    let report_path = env::var_os("REPORT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("report.md"));

    let report = render_regression_report_markdown(&root, &output_dir);
    fs::write(&report_path, report)?;

    println!("{}", report_path.display());
    Ok(())
}

fn rice_stress_audit(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err("usage: cargo xtask rice-stress-audit [MODEL_PATH]".into());
    }

    let root = project_root()?;
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression"));
    fs::create_dir_all(&output_dir)?;

    let model_path = args.first().cloned().unwrap_or_else(|| {
        "public/CubismSdkForNative/Samples/Resources/Rice/Rice.model3.json".to_string()
    });
    let probe_path = env::var_os("PROBE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("rice-stress-audit-probe.txt"));
    let report_path = env::var_os("REPORT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("rice-stress-audit.md"));

    run_model_probe(&root, std::slice::from_ref(&model_path), &probe_path)?;
    let probe = fs::read_to_string(&probe_path)?;
    let report = rice_stress_audit_report(&root, &output_dir, &model_path, &probe_path, &probe);
    fs::write(&report_path, report)?;

    println!("{}", report_path.display());
    Ok(())
}

fn build_app(args: Vec<String>) -> Result<()> {
    let options = parse_build_app_args(args)?;
    let root = project_root()?;
    let (include_dir, lib_dir) = cubism_core_paths(&root)?;
    let config_path = if options.release {
        root.join(BUILD_CONFIG_PATH)
    } else {
        root.join(DEVELOPMENT_CONFIG_PATH)
    };
    let system_camera_source_requested = config_requests_system_camera_source(&config_path);
    if system_camera_source_requested && !camera_provisioning_profiles_available(&root) {
        return Err(system_camera_source_unavailable_message().into());
    }
    let executable = build_metal_executable(&root, options.release, &include_dir, &lib_dir)?;
    let bundle_dir = install_camera_app_wrapper(
        &root,
        &executable,
        options.release,
        system_camera_source_requested,
    )?;
    let launch_bundle_dir = if system_camera_source_requested {
        Some(install_app_bundle_to_applications(&root, &bundle_dir)?)
    } else {
        None
    };

    println!("App wrapper: {}", bundle_dir.display());
    if let Some(launch_bundle_dir) = &launch_bundle_dir {
        println!("Installed app wrapper: {}", launch_bundle_dir.display());
    }
    println!(
        "Profile: {}",
        if options.release {
            "release"
        } else {
            "development"
        }
    );
    println!("Config: {}", relative_display(&root, &config_path));
    println!("Bundle id: {DEV_CAMERA_BUNDLE_ID}");
    println!(
        "Embedded Camera Extension: {}",
        if system_camera_source_requested {
            relative_display(
                &root,
                &bundle_dir
                    .join("Contents/Library/SystemExtensions")
                    .join(VIRTUAL_CAMERA_BUNDLE_NAME),
            )
        } else {
            "disabled for desktop window output".to_string()
        }
    );
    println!(
        "Run it with: cargo xtask run-metal{}",
        if options.release { " --release" } else { "" }
    );
    Ok(())
}

fn run_metal(args: Vec<String>) -> Result<()> {
    let options = parse_run_metal_args(args)?;

    let root = project_root()?;
    let (include_dir, lib_dir) = cubism_core_paths(&root)?;
    let config_path = if options.release {
        root.join(BUILD_CONFIG_PATH)
    } else {
        root.join(DEVELOPMENT_CONFIG_PATH)
    };
    let system_camera_source_requested = config_requests_system_camera_source(&config_path);
    if system_camera_source_requested && !camera_provisioning_profiles_available(&root) {
        return Err(system_camera_source_unavailable_message().into());
    }

    if env::var("RUN_METAL_KILL_OLD").unwrap_or_else(|_| "1".to_string()) != "0" {
        terminate_app_processes(&root);
        let _ = fs::remove_file(root.join("target/vtube-studio-rs.pid"));
    }

    let executable = build_metal_executable(&root, options.release, &include_dir, &lib_dir)?;
    let bundle_dir = install_camera_app_wrapper(
        &root,
        &executable,
        options.release,
        system_camera_source_requested,
    )?;
    println!("App wrapper: {}", bundle_dir.display());
    let launch_bundle_dir = if system_camera_source_requested {
        let installed = install_app_bundle_to_applications(&root, &bundle_dir)?;
        println!("Installed app wrapper: {}", installed.display());
        installed
    } else {
        bundle_dir
    };

    let log_stem = if options.release {
        "run-metal-release"
    } else {
        "run-metal-dev"
    };
    launch_camera_app_wrapper(
        &root,
        &launch_bundle_dir,
        &include_dir,
        &lib_dir,
        &config_path,
        options.model_path.as_deref(),
        log_stem,
        &[],
    )
}

fn build_metal_executable(
    root: &Path,
    release: bool,
    include_dir: &Path,
    lib_dir: &Path,
) -> Result<PathBuf> {
    build_metal_executable_with_features(
        root,
        release,
        include_dir,
        lib_dir,
        "metal-renderer camera-tracking screen-capture-kit iosurface-output system-extension-activation",
    )
}

fn build_metal_executable_with_features(
    root: &Path,
    release: bool,
    include_dir: &Path,
    lib_dir: &Path,
    features: &str,
) -> Result<PathBuf> {
    let mut command = Command::new("cargo");
    command.arg("build");
    if release {
        command.arg("--release");
    }
    command.arg("--features").arg(features);
    command
        .current_dir(&root)
        .env("CUBISM_CORE_INCLUDE_DIR", &include_dir)
        .env("CUBISM_CORE_LIB_DIR", &lib_dir);
    let status = command.status()?;
    if !status.success() {
        let profile = if release { " --release" } else { "" };
        return Err(format!(
            "cargo build{profile} --features \"{features}\" failed with status {status}"
        )
        .into());
    }

    let profile_dir = if release { "release" } else { "debug" };
    Ok(root
        .join("target")
        .join(profile_dir)
        .join("vtube-studio-rs"))
}

fn build_camera_extension(args: Vec<String>) -> Result<()> {
    let target = parse_camera_extension_plan_args(args)?;
    let root = project_root()?;
    let release = matches!(target, SelectModelTarget::Build);
    let executable = build_camera_extension_executable(&root, release)?;
    let bundle_dir = install_camera_extension_bundle(&root, &executable)?;
    println!("Camera Extension prototype bundle built.");
    println!("Target: {}", target.label());
    println!("Bundle: {}", relative_display(&root, &bundle_dir));
    println!("Camera name: {VIRTUAL_CAMERA_NAME}");
    println!("Bundle id: {VIRTUAL_CAMERA_EXTENSION_BUNDLE_ID}");
    println!("Mach service: {VIRTUAL_CAMERA_MACH_SERVICE}");
    println!(
        "Next: run `cargo xtask build-app --release`, move the app to /Applications, then use the VT menu activation prototype."
    );
    println!("Embed with: cargo xtask build-app --release");
    Ok(())
}

fn build_camera_extension_executable(root: &Path, release: bool) -> Result<PathBuf> {
    let mut command = Command::new("cargo");
    command
        .arg("build")
        .arg("-p")
        .arg("vtube-studio-rs-camera-extension");
    if release {
        command.arg("--release");
    }
    command.current_dir(root).stdin(Stdio::null());
    run_status(&mut command)?;
    let profile_dir = if release { "release" } else { "debug" };
    Ok(root
        .join("target")
        .join(profile_dir)
        .join("CameraExtension"))
}

fn install_camera_extension_bundle(root: &Path, executable: &Path) -> Result<PathBuf> {
    let bundle_dir = root
        .join("target/virtual-camera")
        .join(VIRTUAL_CAMERA_BUNDLE_NAME);
    let contents_dir = bundle_dir.join("Contents");
    let macos_dir = contents_dir.join("MacOS");
    let resources_dir = contents_dir.join("Resources");
    let executable_path = macos_dir.join("CameraExtension");
    fs::create_dir_all(&macos_dir)?;
    fs::create_dir_all(&resources_dir)?;
    fs::write(
        contents_dir.join("Info.plist"),
        camera_extension_info_plist(),
    )?;
    fs::write(
        resources_dir.join("CameraExtension.entitlements"),
        camera_extension_entitlements(),
    )?;
    embed_provisioning_profile_if_available(
        root,
        &contents_dir,
        CAMERA_EXTENSION_PROVISION_PROFILE_ENV,
        DEFAULT_CAMERA_EXTENSION_PROVISION_PROFILE,
        ProvisioningProfileKind::CameraExtension,
    )?;
    let _ = fs::remove_file(&executable_path);
    fs::copy(executable, &executable_path)?;
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&executable_path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable_path, permissions)?;
    }
    sign_camera_extension_bundle(root, &bundle_dir)?;
    Ok(bundle_dir)
}

fn sign_camera_extension_bundle(root: &Path, bundle_dir: &Path) -> Result<()> {
    let identity = camera_codesign_identity_choice();
    let entitlements = bundle_dir
        .join("Contents/Resources")
        .join("CameraExtension.entitlements");
    let mut command = Command::new("codesign");
    command
        .arg("--force")
        .arg("--deep")
        .arg("--options")
        .arg("runtime")
        .arg("--entitlements")
        .arg(&entitlements)
        .arg("--sign")
        .arg(&identity.value)
        .arg("--identifier")
        .arg(VIRTUAL_CAMERA_EXTENSION_BUNDLE_ID)
        .arg(bundle_dir)
        .current_dir(root)
        .stdin(Stdio::null());
    run_status(&mut command).map_err(|error| {
        format!(
            "failed to codesign Camera Extension prototype with identity `{}`: {error}. \
Install an Apple Development or Developer ID Application certificate in Keychain, \
or set VTUBE_RS_CODESIGN_IDENTITY to a valid local codesigning identity.",
            identity.value
        )
    })?;
    if identity.is_ad_hoc() {
        println!(
            "Code signed Camera Extension prototype with ad-hoc identity. No valid Apple codesigning identity was found in Keychain."
        );
    } else {
        println!(
            "Code signed Camera Extension prototype with identity `{}` ({}).",
            identity.value, identity.source
        );
    }
    Ok(())
}

fn install_camera_app_wrapper(
    root: &Path,
    executable: &Path,
    release: bool,
    system_camera_source_enabled: bool,
) -> Result<PathBuf> {
    let executable_name = "vtube-studio-rs";
    let bundle_dir = root.join("target/dev-app").join(DEV_APP_BUNDLE_NAME);
    let contents_dir = bundle_dir.join("Contents");
    let macos_dir = contents_dir.join("MacOS");
    let resources_dir = contents_dir.join("Resources");
    let app_executable = macos_dir.join(executable_name);
    fs::create_dir_all(&macos_dir)?;
    fs::create_dir_all(&resources_dir)?;
    fs::write(
        contents_dir.join("Info.plist"),
        dev_camera_info_plist(executable_name),
    )?;
    let container_entitlements = resources_dir.join("ContainerApp.entitlements");
    if system_camera_source_enabled {
        fs::write(&container_entitlements, camera_container_app_entitlements())?;
        embed_provisioning_profile_if_available(
            root,
            &contents_dir,
            CONTAINER_PROVISION_PROFILE_ENV,
            DEFAULT_CONTAINER_PROVISION_PROFILE,
            ProvisioningProfileKind::ContainerApp,
        )?;
    } else {
        let _ = fs::remove_file(&container_entitlements);
        let _ = fs::remove_file(contents_dir.join("embedded.provisionprofile"));
    }
    let _ = fs::remove_file(&app_executable);
    fs::copy(executable, &app_executable)?;
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&app_executable)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&app_executable, permissions)?;
    }
    let system_extensions_dir = contents_dir.join("Library/SystemExtensions");
    if system_camera_source_enabled {
        let extension_executable = build_camera_extension_executable(root, release)?;
        let extension_bundle = install_camera_extension_bundle(root, &extension_executable)?;
        embed_camera_extension_bundle(&contents_dir, &extension_bundle)?;
    } else {
        let _ = fs::remove_dir_all(&system_extensions_dir);
    }
    sign_camera_dev_app(root, &bundle_dir, system_camera_source_enabled)?;
    Ok(bundle_dir)
}

fn embed_camera_extension_bundle(contents_dir: &Path, extension_bundle: &Path) -> Result<PathBuf> {
    let system_extensions_dir = contents_dir.join("Library/SystemExtensions");
    fs::create_dir_all(&system_extensions_dir)?;
    let embedded_bundle = system_extensions_dir.join(VIRTUAL_CAMERA_BUNDLE_NAME);
    copy_dir_replace(extension_bundle, &embedded_bundle)?;
    Ok(embedded_bundle)
}

fn install_app_bundle_to_applications(root: &Path, bundle_dir: &Path) -> Result<PathBuf> {
    let installed_bundle = PathBuf::from("/Applications").join(DEV_APP_BUNDLE_NAME);
    copy_dir_replace(bundle_dir, &installed_bundle).map_err(|error| {
        format!(
            "failed to install app wrapper to {}: {error}. \
System Extension activation requires the app bundle to live in /Applications.",
            installed_bundle.display()
        )
    })?;
    println!(
        "Installed app wrapper for System Extension activation: {}",
        installed_bundle.display()
    );
    println!("Source app wrapper: {}", relative_display(root, bundle_dir));
    Ok(installed_bundle)
}

fn embed_provisioning_profile_if_available(
    root: &Path,
    contents_dir: &Path,
    env_name: &str,
    default_relative_path: &str,
    kind: ProvisioningProfileKind,
) -> Result<Option<PathBuf>> {
    let label = kind.label();
    let profile_path = match env::var_os(env_name) {
        Some(value) if !value.is_empty() => {
            let path = PathBuf::from(value);
            if !path.is_file() {
                return Err(format!(
                    "{env_name} points to {}, but that file does not exist.",
                    path.display()
                )
                .into());
            }
            path
        }
        _ => {
            let path = root.join(default_relative_path);
            if !path.is_file() {
                println!(
                    "No {label} provisioning profile embedded. Set {env_name} or place one at {default_relative_path}."
                );
                return Ok(None);
            }
            path
        }
    };

    let summary = validate_provisioning_profile(&profile_path, kind).map_err(|error| {
        format!(
            "{label} provisioning profile {} is not usable for `{}`: {error}",
            profile_path.display(),
            kind.expected_bundle_id()
        )
    })?;
    let embedded_path = contents_dir.join("embedded.provisionprofile");
    fs::copy(&profile_path, &embedded_path).map_err(|error| {
        format!(
            "failed to embed {label} provisioning profile {} into {}: {error}",
            profile_path.display(),
            embedded_path.display()
        )
    })?;
    println!(
        "Embedded {label} provisioning profile: {} -> {}",
        relative_display(root, &profile_path),
        relative_display(root, &embedded_path)
    );
    println!(
        "Provisioning profile validated: name={} app_id={} team={}",
        summary.name.as_deref().unwrap_or("unknown"),
        summary.application_identifier,
        summary.team_identifier.as_deref().unwrap_or("unknown")
    );
    Ok(Some(embedded_path))
}

#[derive(Debug, Clone)]
struct ProvisioningProfileSummary {
    name: Option<String>,
    application_identifier: String,
    team_identifier: Option<String>,
    app_groups: Vec<String>,
    system_extension_install: bool,
}

fn validate_provisioning_profile(
    profile_path: &Path,
    kind: ProvisioningProfileKind,
) -> Result<ProvisioningProfileSummary> {
    let value = decode_provisioning_profile_json(profile_path)?;
    let summary = provisioning_profile_summary_from_json(&value)?;
    validate_provisioning_profile_summary(&summary, kind)?;
    Ok(summary)
}

fn decode_provisioning_profile_json(profile_path: &Path) -> Result<serde_json::Value> {
    let security_output = Command::new("security")
        .arg("cms")
        .arg("-D")
        .arg("-i")
        .arg(profile_path)
        .output()
        .map_err(|error| {
            format!(
                "failed to run `security cms -D -i {}`: {error}",
                profile_path.display()
            )
        })?;
    if !security_output.status.success() {
        return Err(format!(
            "`security cms -D -i {}` failed with status {}\nstderr:\n{}",
            profile_path.display(),
            security_output.status,
            String::from_utf8_lossy(&security_output.stderr)
        )
        .into());
    }

    let mut plutil = Command::new("plutil")
        .arg("-convert")
        .arg("json")
        .arg("-o")
        .arg("-")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start `plutil`: {error}"))?;
    {
        let stdin = plutil.stdin.as_mut().ok_or("failed to open plutil stdin")?;
        stdin.write_all(&security_output.stdout)?;
    }
    let plutil_output = plutil.wait_with_output()?;
    if !plutil_output.status.success() {
        return Err(format!(
            "`plutil -convert json` failed with status {}\nstderr:\n{}",
            plutil_output.status,
            String::from_utf8_lossy(&plutil_output.stderr)
        )
        .into());
    }

    serde_json::from_slice(&plutil_output.stdout)
        .map_err(|error| format!("failed to parse provisioning profile JSON: {error}").into())
}

fn provisioning_profile_summary_from_json(
    value: &serde_json::Value,
) -> Result<ProvisioningProfileSummary> {
    let entitlements = value
        .get("Entitlements")
        .and_then(serde_json::Value::as_object)
        .ok_or("provisioning profile has no Entitlements dictionary")?;
    let application_identifier = entitlements
        .get("application-identifier")
        .and_then(serde_json::Value::as_str)
        .ok_or("provisioning profile has no Entitlements.application-identifier")?
        .to_string();
    let team_identifier = value
        .get("TeamIdentifier")
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.iter().find_map(serde_json::Value::as_str))
        .or_else(|| {
            value
                .get("ApplicationIdentifierPrefix")
                .and_then(serde_json::Value::as_array)
                .and_then(|values| values.iter().find_map(serde_json::Value::as_str))
        })
        .map(str::to_string);
    let app_groups = entitlements
        .get("com.apple.security.application-groups")
        .and_then(serde_json::Value::as_array)
        .map(|groups| {
            groups
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let system_extension_install = entitlements
        .get("com.apple.developer.system-extension.install")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    Ok(ProvisioningProfileSummary {
        name: value
            .get("Name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        application_identifier,
        team_identifier,
        app_groups,
        system_extension_install,
    })
}

fn validate_provisioning_profile_summary(
    summary: &ProvisioningProfileSummary,
    kind: ProvisioningProfileKind,
) -> Result<()> {
    if !summary
        .application_identifier
        .ends_with(kind.expected_bundle_id())
    {
        return Err(format!(
            "application-identifier `{}` does not match expected bundle id `{}`",
            summary.application_identifier,
            kind.expected_bundle_id()
        )
        .into());
    }
    if !summary
        .app_groups
        .iter()
        .any(|group| group == VIRTUAL_CAMERA_APP_GROUP)
    {
        return Err(format!(
            "profile does not include required app group `{}`",
            VIRTUAL_CAMERA_APP_GROUP
        )
        .into());
    }
    if kind.requires_system_extension_install() && !summary.system_extension_install {
        return Err(
            "container app profile does not include com.apple.developer.system-extension.install"
                .into(),
        );
    }
    Ok(())
}

fn embedded_provisioning_profile_summary(
    bundle_path: &Path,
    kind: ProvisioningProfileKind,
) -> Option<ProvisioningProfileSummary> {
    let profile_path = bundle_path.join("Contents/embedded.provisionprofile");
    if !profile_path.is_file() {
        return None;
    }
    validate_provisioning_profile(&profile_path, kind).ok()
}

fn camera_provisioning_profiles_available(root: &Path) -> bool {
    configured_provisioning_profile_valid(
        root,
        CONTAINER_PROVISION_PROFILE_ENV,
        DEFAULT_CONTAINER_PROVISION_PROFILE,
        ProvisioningProfileKind::ContainerApp,
    ) && configured_provisioning_profile_valid(
        root,
        CAMERA_EXTENSION_PROVISION_PROFILE_ENV,
        DEFAULT_CAMERA_EXTENSION_PROVISION_PROFILE,
        ProvisioningProfileKind::CameraExtension,
    )
}

fn configured_provisioning_profile_valid(
    root: &Path,
    env_name: &str,
    default_relative_path: &str,
    kind: ProvisioningProfileKind,
) -> bool {
    let path = env::var_os(env_name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(default_relative_path));
    path.is_file() && validate_provisioning_profile(&path, kind).is_ok()
}

fn system_camera_source_unavailable_message() -> String {
    format!(
        "System Camera Source is selected, but the required Apple Developer Program provisioning profiles are missing or invalid.\n\
This Apple ID cannot currently complete the no-desktop virtual camera path.\n\n\
Use the supported OBS desktop window path instead:\n\
  cargo xtask configure-obs-recording --build\n\
  cargo xtask run-metal --release\n\n\
If you later get Apple Developer Program profiles, place them at `{DEFAULT_CONTAINER_PROVISION_PROFILE}` and `{DEFAULT_CAMERA_EXTENSION_PROVISION_PROFILE}`, then run:\n\
  cargo xtask build-app --release"
    )
}

fn config_requests_system_camera_source(config_path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(config_path) else {
        return false;
    };
    let Ok(config) = toml::from_str::<toml::Value>(&content) else {
        return false;
    };
    let Some(output) = config.get("output") else {
        return false;
    };
    let mode = output
        .get("mode")
        .and_then(toml::Value::as_str)
        .unwrap_or("window")
        .trim()
        .to_ascii_lowercase();
    let internal = output.get("internal");
    let producer = internal
        .and_then(|value| value.get("producer"))
        .and_then(toml::Value::as_str)
        .unwrap_or("none")
        .trim()
        .to_ascii_lowercase();
    let activate_virtual_camera = internal
        .and_then(|value| value.get("activate_virtual_camera"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    mode == "internal" && producer == "iosurface" && activate_virtual_camera
}

fn launch_camera_app_wrapper(
    root: &Path,
    bundle_dir: &Path,
    include_dir: &Path,
    lib_dir: &Path,
    config_path: &Path,
    model_path: Option<&str>,
    log_stem: &str,
    extra_env: &[(&str, &Path)],
) -> Result<()> {
    let log_dir = root.join("target/camera-test");
    fs::create_dir_all(&log_dir)?;
    let stdout_path = log_dir.join(format!("{log_stem}.stdout.log"));
    let stderr_path = log_dir.join(format!("{log_stem}.stderr.log"));
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);
    let mut command = Command::new("open");
    command
        .arg("-n")
        .arg("--stdout")
        .arg(&stdout_path)
        .arg("--stderr")
        .arg(&stderr_path)
        .arg("--env")
        .arg(format!("CUBISM_CORE_INCLUDE_DIR={}", include_dir.display()))
        .arg("--env")
        .arg(format!("CUBISM_CORE_LIB_DIR={}", lib_dir.display()))
        .arg(&bundle_dir);
    for (name, value) in extra_env {
        command
            .arg("--env")
            .arg(format!("{name}={}", value.display()));
    }
    command.arg("--args").arg("--config").arg(config_path);
    if let Some(model_path) = model_path {
        command.arg(absolute_path(&root, model_path));
    }

    let status = command.status()?;
    if !status.success() {
        return Err(format!("vtube-studio-rs app wrapper failed with status {status}").into());
    }

    let pid_path = root.join("target/vtube-studio-rs.pid");
    let pid = wait_for_pid_file(&pid_path, Duration::from_secs(10))?;
    println!("vtube-studio-rs app launched with pid {pid}.");
    println!("Camera stdout: {}", relative_display(&root, &stdout_path));
    println!("Camera stderr: {}", relative_display(&root, &stderr_path));
    println!("Close the avatar window to end this command.");
    while pid_is_alive(pid) {
        thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}

fn dev_camera_info_plist(executable_name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>{executable_name}</string>
  <key>CFBundleIdentifier</key>
  <string>{DEV_CAMERA_BUNDLE_ID}</string>
  <key>CFBundleName</key>
  <string>vtube-studio-rs Dev</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>1</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSCameraUsageDescription</key>
  <string>vtube-studio-rs uses the local camera to estimate face landmarks and drive the avatar. Frames are not stored, written to disk, or logged.</string>
  <key>NSMicrophoneUsageDescription</key>
  <string>vtube-studio-rs can use the local microphone level to drive avatar mouth movement when microphone input is enabled.</string>
  <key>NSSystemExtensionUsageDescription</key>
  <string>vtube-studio-rs can install its Camera Extension to publish avatar frames as a system camera source.</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
"#
    )
}

fn sign_camera_dev_app(
    root: &Path,
    bundle_dir: &Path,
    system_camera_source_enabled: bool,
) -> Result<()> {
    let identity = camera_codesign_identity_choice();
    let entitlements = bundle_dir
        .join("Contents/Resources")
        .join("ContainerApp.entitlements");
    let mut command = Command::new("codesign");
    command
        .arg("--force")
        .arg("--deep")
        .arg("--options")
        .arg("runtime");
    if system_camera_source_enabled {
        command.arg("--entitlements").arg(&entitlements);
    }
    command
        .arg("--sign")
        .arg(&identity.value)
        .arg("--identifier")
        .arg(DEV_CAMERA_BUNDLE_ID)
        .arg(bundle_dir)
        .current_dir(root)
        .stdin(Stdio::null());
    run_status(&mut command).map_err(|error| {
        format!(
            "failed to codesign camera dev app with identity `{}`: {error}. \
Install an Apple Development or Developer ID Application certificate in Keychain, \
or set VTUBE_RS_CODESIGN_IDENTITY to a valid local codesigning identity.",
            identity.value
        )
    })?;

    if identity.is_ad_hoc() {
        println!(
            "Code signed camera dev app with ad-hoc identity and stable identifier {DEV_CAMERA_BUNDLE_ID}."
        );
        println!(
            "No valid Apple codesigning identity was found. Once Xcode installs one, cargo xtask build-app will auto-detect it."
        );
    } else {
        println!(
            "Code signed camera dev app with identity `{}` ({}).",
            identity.value, identity.source
        );
    }
    Ok(())
}

fn camera_codesign_identity() -> String {
    camera_codesign_identity_choice().value
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CodesignIdentityChoice {
    value: String,
    source: &'static str,
}

impl CodesignIdentityChoice {
    fn is_ad_hoc(&self) -> bool {
        self.value == "-"
    }
}

fn camera_codesign_identity_choice() -> CodesignIdentityChoice {
    env::var("VTUBE_RS_CODESIGN_IDENTITY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| CodesignIdentityChoice {
            value,
            source: "VTUBE_RS_CODESIGN_IDENTITY",
        })
        .or_else(|| {
            detect_codesign_identity().map(|value| CodesignIdentityChoice {
                value,
                source: "auto-detected Keychain identity",
            })
        })
        .unwrap_or_else(|| CodesignIdentityChoice {
            value: "-".to_string(),
            source: "ad-hoc fallback",
        })
}

fn detect_codesign_identity() -> Option<String> {
    let output = Command::new("security")
        .arg("find-identity")
        .arg("-v")
        .arg("-p")
        .arg("codesigning")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    for preferred in [
        "Apple Development",
        "Developer ID Application",
        "Apple Distribution",
        "Mac Developer",
        "3rd Party Mac Developer Application",
    ] {
        if let Some(identity) = find_codesign_identity_line(&text, preferred) {
            return Some(identity);
        }
    }
    None
}

fn find_codesign_identity_line(text: &str, needle: &str) -> Option<String> {
    text.lines()
        .filter(|line| line.contains(needle))
        .find_map(|line| {
            let start = line.find('"')? + 1;
            let end = line[start..].find('"')? + start;
            Some(line[start..end].to_string())
        })
}

fn find_untrusted_codesign_identity_line(text: &str) -> Option<String> {
    text.lines()
        .find(|line| {
            line.contains("CSSMERR_TP_NOT_TRUSTED")
                && [
                    "Apple Development",
                    "Developer ID Application",
                    "Apple Distribution",
                    "Mac Developer",
                    "3rd Party Mac Developer Application",
                ]
                .iter()
                .any(|needle| line.contains(needle))
        })
        .map(str::trim)
        .map(str::to_string)
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct RunMetalOptions {
    release: bool,
    model_path: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct BuildAppOptions {
    release: bool,
}

fn parse_build_app_args(args: Vec<String>) -> Result<BuildAppOptions> {
    let mut release = false;

    for arg in args {
        match arg.as_str() {
            "--release" => {
                if release {
                    return Err("usage: cargo xtask build-app [--release]".into());
                }
                release = true;
            }
            "--dev" => {
                if release {
                    return Err("usage: cargo xtask build-app [--release]".into());
                }
            }
            "-h" | "--help" => {
                return Err("usage: cargo xtask build-app [--release]".into());
            }
            value => {
                return Err(format!("unknown build-app option: {value}").into());
            }
        }
    }

    Ok(BuildAppOptions { release })
}

fn parse_run_metal_args(args: Vec<String>) -> Result<RunMetalOptions> {
    let mut release = false;
    let mut model_path = None;

    for arg in args {
        match arg.as_str() {
            "--release" => {
                if release {
                    return Err("usage: cargo xtask run-metal [--release] [MODEL_PATH]".into());
                }
                release = true;
            }
            "--dev" => {
                if release {
                    return Err("usage: cargo xtask run-metal [--release] [MODEL_PATH]".into());
                }
            }
            "-h" | "--help" => {
                return Err("usage: cargo xtask run-metal [--release] [MODEL_PATH]".into());
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown run-metal option: {value}").into());
            }
            value => {
                if model_path.is_some() {
                    return Err("usage: cargo xtask run-metal [--release] [MODEL_PATH]".into());
                }
                model_path = Some(value.to_string());
            }
        }
    }

    Ok(RunMetalOptions {
        release,
        model_path,
    })
}

fn run_space_test(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err("usage: cargo xtask run-space-test [MODEL_PATH]".into());
    }

    let root = project_root()?;
    let model_label = args
        .first()
        .map(String::as_str)
        .unwrap_or("profile config [model].path, or public/model/0.model3.json when unset");
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/space-test"));
    fs::create_dir_all(&output_dir)?;

    if env::var("VTUBE_RS_SKIP_TARGET_CLEAN").unwrap_or_default() != "1" {
        clean(vec!["--generated".to_string()])?;
        fs::create_dir_all(&output_dir)?;
    }

    let timestamp = timestamp_for_filename();
    let log_path = output_dir.join(format!("space-test-{timestamp}.log"));
    let report_path = output_dir.join(format!("space-test-{timestamp}.md"));
    let (include_dir, lib_dir) = cubism_core_paths(&root)?;

    terminate_app_processes(&root);
    let _ = fs::remove_file(root.join("target/vtube-studio-rs.pid"));

    println!("Starting vtube-studio-rs Space/display reliability run.");
    println!("Model: {model_label}");
    println!("Log: {}", log_path.display());
    println!("Report: {}", report_path.display());
    println!();
    println!("Checklist:");
    println!("  [ ] Wait for the avatar window and confirm Frames keep increasing.");
    println!("  [ ] Switch between macOS Spaces several times.");
    println!("  [ ] Place the avatar beside a full-screen app and confirm it remains visible.");
    println!("  [ ] Check ScreenCaptureKit probe summaries for frame/stall/recovery events.");
    println!("  [ ] Optionally test display sleep/wake and confirm the avatar recovers.");
    println!("  [ ] Confirm reruns do not leave duplicate avatar windows.");
    println!("  [ ] Press Ctrl-C here to stop, print the summary, and write the report.");
    println!();

    let log_file = fs::File::create(&log_path)?;
    let log_stderr = log_file.try_clone()?;
    let mut command = Command::new("cargo");
    command
        .arg("run")
        .arg("--features")
        .arg("metal-renderer screen-capture-kit")
        .arg("--");
    if let Some(model_path) = args.first() {
        command.arg(model_path);
    }

    let app_child = command
        .current_dir(&root)
        .env("CUBISM_CORE_INCLUDE_DIR", include_dir)
        .env("CUBISM_CORE_LIB_DIR", lib_dir)
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_stderr))
        .spawn()?;
    let mut app = ChildCleanupGuard {
        child: app_child,
        pid_path: root.join("target/vtube-studio-rs.pid"),
    };
    let mut tail = TailThreadGuard::start(log_path.clone());

    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_handler = Arc::clone(&stop);
    ctrlc::set_handler(move || {
        stop_for_handler.store(true, Ordering::SeqCst);
    })?;

    while !stop.load(Ordering::SeqCst) {
        if app.has_exited()? {
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }

    tail.stop();
    drop(app);
    terminate_app_processes(&root);
    let report = write_space_test_report(model_label, &log_path, &report_path)?;
    print_space_test_summary(&report, &log_path, &report_path);
    Ok(())
}

fn sample_compatibility_sweep(args: Vec<String>) -> Result<()> {
    if args.len() > 1 {
        return Err("usage: cargo xtask sample-compatibility-sweep [SAMPLES_ROOT]".into());
    }

    let root = project_root()?;
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression"));
    fs::create_dir_all(&output_dir)?;

    let samples_root = args
        .first()
        .cloned()
        .unwrap_or_else(|| "public/CubismSdkForNative/Samples/Resources".to_string());
    let canonical_probe_path = output_dir.join("probe.txt");
    let probe_path = env::var_os("PROBE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("compatibility-probe.txt"));
    let report_path = env::var_os("REPORT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| output_dir.join("compatibility-sweep.md"));

    run_model_probe(
        &root,
        std::slice::from_ref(&samples_root),
        &canonical_probe_path,
    )?;
    if canonical_probe_path != probe_path {
        fs::copy(&canonical_probe_path, &probe_path)?;
    }
    let probe = fs::read_to_string(&probe_path)?;
    let report = compatibility_report(&root, &samples_root, &probe_path, &probe);
    fs::write(&report_path, report)?;

    println!("{}", report_path.display());
    Ok(())
}

fn capture_mask_mode_matrix(
    model_path: &str,
    default_output_dir: &str,
    matrix_label: &str,
    done_label: &str,
) -> Result<()> {
    let root = project_root()?;
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(default_output_dir));
    capture_mask_mode_matrix_to(
        &root,
        model_path,
        output_dir,
        matrix_label,
        done_label,
        capture_options_from_env(),
    )
}

#[derive(Clone, Copy)]
struct CaptureRunOptions {
    clean_before: bool,
    report_after: bool,
}

fn capture_options_from_env() -> CaptureRunOptions {
    CaptureRunOptions {
        clean_before: env::var("VTUBE_RS_SKIP_TARGET_CLEAN").unwrap_or_default() != "1",
        report_after: env::var("VTUBE_RS_SKIP_REPORT").unwrap_or_default() != "1",
    }
}

fn capture_mask_mode_matrix_to(
    root: &Path,
    model_path: &str,
    output_dir: PathBuf,
    matrix_label: &str,
    done_label: &str,
    options: CaptureRunOptions,
) -> Result<()> {
    fs::create_dir_all(&output_dir)?;

    if options.clean_before {
        clean(vec!["--generated".to_string()])?;
        fs::create_dir_all(&output_dir)?;
    }

    let config_path = root.join(DEVELOPMENT_CONFIG_PATH);
    let example_config_path = root.join(DEVELOPMENT_EXAMPLE_CONFIG_PATH);
    let _config_guard = ConfigRestoreGuard::prepare(&config_path, &example_config_path)?;

    capture_renderer_mode(
        &root,
        &output_dir,
        model_path,
        matrix_label,
        "shared",
        false,
        false,
    )?;
    capture_renderer_mode(
        &root,
        &output_dir,
        model_path,
        matrix_label,
        "high-precision",
        false,
        true,
    )?;
    capture_renderer_mode(
        &root,
        &output_dir,
        model_path,
        matrix_label,
        "no-mask",
        true,
        false,
    )?;

    println!("{done_label}: {}", output_dir.display());
    if options.report_after {
        run_render_regression_report_safe(root)?;
    }
    Ok(())
}

fn capture_quality_mode_matrix(models: Vec<String>) -> Result<()> {
    let root = project_root()?;
    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression/quality-matrix"));
    capture_quality_mode_matrix_to(&root, models, output_dir, capture_options_from_env())
}

fn capture_quality_mode_matrix_to(
    root: &Path,
    models: Vec<String>,
    output_dir: PathBuf,
    options: CaptureRunOptions,
) -> Result<()> {
    fs::create_dir_all(&output_dir)?;

    if options.clean_before {
        clean(vec!["--generated".to_string()])?;
        fs::create_dir_all(&output_dir)?;
    }

    let config_path = root.join(DEVELOPMENT_CONFIG_PATH);
    let example_config_path = root.join(DEVELOPMENT_EXAMPLE_CONFIG_PATH);
    let _config_guard = ConfigRestoreGuard::prepare(&config_path, &example_config_path)?;

    for model_path in models {
        if !model_exists(&root, &model_path) {
            eprintln!("Skipping missing model: {model_path}");
            continue;
        }

        capture_configured_mode(
            &root,
            &output_dir,
            &model_path,
            "quality",
            "mipmaps-off",
            &[
                ("disable_masks", "false".to_string()),
                ("high_precision_masks", "false".to_string()),
                ("atlas_mipmaps", "false".to_string()),
                ("atlas_anisotropy", "1".to_string()),
                ("debug_texture_mode", "\"none\"".to_string()),
            ],
        )?;
        capture_configured_mode(
            &root,
            &output_dir,
            &model_path,
            "quality",
            "mipmaps-on",
            &[
                ("disable_masks", "false".to_string()),
                ("high_precision_masks", "false".to_string()),
                ("atlas_mipmaps", "true".to_string()),
                ("atlas_anisotropy", "1".to_string()),
                ("debug_texture_mode", "\"none\"".to_string()),
            ],
        )?;
        capture_configured_mode(
            &root,
            &output_dir,
            &model_path,
            "quality",
            "mipmaps-on-aniso8",
            &[
                ("disable_masks", "false".to_string()),
                ("high_precision_masks", "false".to_string()),
                ("atlas_mipmaps", "true".to_string()),
                ("atlas_anisotropy", "8".to_string()),
                ("debug_texture_mode", "\"none\"".to_string()),
            ],
        )?;
    }

    println!("Quality matrix screenshots: {}", output_dir.display());
    if options.report_after {
        run_render_regression_report_safe(root)?;
    }
    Ok(())
}

fn capture_rice_stress_matrix(model_path: &str) -> Result<()> {
    let root = project_root()?;
    if !model_exists(&root, model_path) {
        return Err(format!("Missing optional Rice stress model: {model_path}").into());
    }

    let output_dir = env::var_os("OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/render-regression/rice-stress"));
    capture_rice_stress_matrix_to(&root, model_path, output_dir, capture_options_from_env())
}

fn capture_rice_stress_matrix_to(
    root: &Path,
    model_path: &str,
    output_dir: PathBuf,
    options: CaptureRunOptions,
) -> Result<()> {
    if !model_exists(root, model_path) {
        return Err(format!("Missing optional Rice stress model: {model_path}").into());
    }

    fs::create_dir_all(&output_dir)?;

    if options.clean_before {
        clean(vec!["--generated".to_string()])?;
        fs::create_dir_all(&output_dir)?;
    }

    let probe_path = output_dir.parent().unwrap_or(root).join("probe.txt");
    run_model_probe(root, &[model_path.to_string()], &probe_path)?;

    let config_path = root.join(DEVELOPMENT_CONFIG_PATH);
    let example_config_path = root.join(DEVELOPMENT_EXAMPLE_CONFIG_PATH);
    let _config_guard = ConfigRestoreGuard::prepare(&config_path, &example_config_path)?;

    capture_renderer_mode(
        &root,
        &output_dir,
        model_path,
        "stress",
        "shared",
        false,
        false,
    )?;
    capture_renderer_mode(
        &root,
        &output_dir,
        model_path,
        "stress",
        "high-precision",
        false,
        true,
    )?;
    capture_renderer_mode(
        &root,
        &output_dir,
        model_path,
        "stress",
        "no-mask",
        true,
        false,
    )?;

    println!("Rice stress screenshots: {}", output_dir.display());
    if options.report_after {
        run_render_regression_report_safe(root)?;
    }
    Ok(())
}

fn select_model(args: Vec<String>) -> Result<()> {
    let (target, model_arg) = parse_select_model_args(args)?;

    let root = project_root()?;
    let input_path = Path::new(&model_arg);
    let model_path = if input_path.is_absolute() {
        input_path.to_path_buf()
    } else {
        root.join(input_path)
    };

    if !is_model3_path(&model_path) {
        return Err(format!(
            "model path must end with .model3.json: {}",
            model_path.display()
        )
        .into());
    }
    require_file(&model_path, "Missing model manifest")?;

    let summary = ModelManifestSummary::load(&model_path)?;
    let stored_path = relative_display(&root, &model_path);
    let config_path = root.join(target.config_path());
    let example_config_path = root.join(target.example_config_path());
    let content = if config_path.is_file() {
        fs::read_to_string(&config_path)?
    } else if example_config_path.is_file() {
        fs::read_to_string(&example_config_path)?
    } else {
        String::new()
    };
    let updated = set_toml_section_value(
        &content,
        "model",
        "path",
        &toml_string_literal(&stored_path),
    );
    fs::write(&config_path, updated)?;

    println!("Selected model: {}", summary.name);
    println!("Target: {}", target.label());
    println!("Config: {}", relative_display(&root, &config_path));
    println!("Path: {stored_path}");
    println!(
        "Resources: textures {} | motions {} | expressions {} | physics {} | display {}",
        summary.texture_count,
        summary.motion_count,
        summary.expression_count,
        yes_no(summary.has_physics),
        yes_no(summary.has_display_info)
    );
    println!("Run with: cargo xtask run-metal");

    Ok(())
}

fn configure_obs_recording(args: Vec<String>) -> Result<()> {
    let (target, placement) = parse_obs_recording_args(args)?;
    let root = project_root()?;
    let config_path = root.join(target.config_path());
    let example_config_path = root.join(target.example_config_path());
    let mut content = if config_path.is_file() {
        fs::read_to_string(&config_path)?
    } else if example_config_path.is_file() {
        fs::read_to_string(&example_config_path)?
    } else {
        String::new()
    };
    content = remove_toml_section(&content, "output");

    let runtime_profile = match target {
        SelectModelTarget::Development => "development",
        SelectModelTarget::Build => "release",
    };
    let (window_x, window_y) = placement.origin();
    content = set_toml_section_values(
        &content,
        "output",
        &[
            ("mode", toml_string_literal("window")),
            ("internal.width", "1080.0".to_string()),
            ("internal.height", "1080.0".to_string()),
            ("internal.producer", toml_string_literal("none")),
            (
                "internal.manifest_path",
                toml_string_literal("target/internal-output/iosurface.json"),
            ),
            ("internal.obs_preview_window", "false".to_string()),
            ("internal.activate_virtual_camera", "false".to_string()),
        ],
    );
    content = set_toml_section_values(
        &content,
        "app",
        &[
            ("runtime_profile", toml_string_literal(runtime_profile)),
            ("window_level", toml_string_literal("screen_saver")),
            ("window_x", format!("{window_x:.1}")),
            ("window_y", format!("{window_y:.1}")),
            ("window_width", "540.0".to_string()),
            ("window_height", "720.0".to_string()),
            ("window_capture_friendly", "true".to_string()),
        ],
    );
    content = set_toml_section_value(&content, "diagnostics", "show", "false");
    content = set_toml_section_values(
        &content,
        "capture.screen_capture_kit",
        &[
            ("enabled", "false".to_string()),
            ("target_fps", "10".to_string()),
            ("log_interval_seconds", "2.0".to_string()),
            ("stalled_after_seconds", "2.0".to_string()),
        ],
    );
    content = set_toml_section_values(
        &content,
        "renderer",
        &[
            ("disable_masks", "false".to_string()),
            ("high_precision_masks", "false".to_string()),
            ("enable_msaa", "true".to_string()),
            ("atlas_mipmaps", "true".to_string()),
            ("atlas_anisotropy", "8".to_string()),
            ("debug_texture_mode", toml_string_literal("none")),
        ],
    );
    fs::write(&config_path, content)?;

    println!("OBS Window Capture preset updated.");
    println!("Target: {}", target.label());
    println!("Config: {}", relative_display(&root, &config_path));
    println!(
        "Output: {} for OBS Window Capture or macOS Screen Capture",
        placement.label()
    );
    println!(
        "Placement: {} at x={window_x:.1}, y={window_y:.1}",
        placement.label()
    );
    println!("Note: this is not an internal no-desktop OBS output path.");
    println!(
        "Window: level screen_saver | size 540x720 | title `vtube-studio-rs OBS Source` | capture-friendly on | diagnostics off"
    );
    println!("Renderer: MSAA on | mipmaps on | anisotropy 8 | masks enabled");
    println!("ScreenCaptureKit probe: off (not an OBS output path)");
    println!(
        "Run with: cargo xtask run-metal{}",
        if matches!(target, SelectModelTarget::Build) {
            " --release"
        } else {
            ""
        }
    );
    Ok(())
}

fn configure_internal_output(args: Vec<String>) -> Result<()> {
    let target = parse_internal_output_args(args)?;
    let root = project_root()?;
    let config_path = root.join(target.config_path());
    let example_config_path = root.join(target.example_config_path());
    let mut content = if config_path.is_file() {
        fs::read_to_string(&config_path)?
    } else if example_config_path.is_file() {
        fs::read_to_string(&example_config_path)?
    } else {
        String::new()
    };
    content = remove_toml_section(&content, "output");

    let runtime_profile = match target {
        SelectModelTarget::Development => "development",
        SelectModelTarget::Build => "release",
    };
    content = set_toml_section_values(
        &content,
        "output",
        &[
            ("mode", toml_string_literal("internal")),
            ("internal.width", "1080.0".to_string()),
            ("internal.height", "1080.0".to_string()),
            ("internal.producer", toml_string_literal("iosurface")),
            (
                "internal.manifest_path",
                toml_string_literal("target/internal-output/iosurface.json"),
            ),
            ("internal.obs_preview_window", "false".to_string()),
            ("internal.activate_virtual_camera", "true".to_string()),
        ],
    );
    content = set_toml_section_values(
        &content,
        "app",
        &[
            ("runtime_profile", toml_string_literal(runtime_profile)),
            ("window_capture_friendly", "false".to_string()),
        ],
    );
    content = set_toml_section_value(&content, "diagnostics", "show", "false");
    content = set_toml_section_values(
        &content,
        "capture.screen_capture_kit",
        &[
            ("enabled", "false".to_string()),
            ("target_fps", "10".to_string()),
            ("log_interval_seconds", "2.0".to_string()),
            ("stalled_after_seconds", "2.0".to_string()),
        ],
    );
    fs::write(&config_path, content)?;

    println!("System camera output preset updated.");
    println!("Target: {}", target.label());
    println!("Config: {}", relative_display(&root, &config_path));
    println!(
        "Output: Metal renders into IOSurface without a desktop avatar window and auto-requests Camera Extension activation"
    );
    println!("Manifest: target/internal-output/iosurface.json");
    println!("OBS: use `VTube Studio RS Camera` after macOS approves the Camera Extension.");
    println!(
        "Run with: cargo xtask run-metal{}",
        if matches!(target, SelectModelTarget::Build) {
            " --release"
        } else {
            ""
        }
    );
    Ok(())
}

fn camera_extension_plan(args: Vec<String>) -> Result<()> {
    let target = parse_camera_extension_plan_args(args)?;
    let root = project_root()?;
    let config_path = root.join(target.config_path());
    let output_dir = root.join("target/virtual-camera");
    let template_dir = output_dir.join("camera-extension-prototype");
    fs::create_dir_all(&template_dir)?;

    let readiness = build_virtual_camera_readiness_report(&root, target, &config_path)?;
    let readiness_path = output_dir.join("readiness.md");
    let plan_path = output_dir.join("camera-extension-plan.md");
    let app_entitlements_path = template_dir.join("ContainerApp.entitlements");
    let extension_entitlements_path = template_dir.join("CameraExtension.entitlements");
    let info_plist_path = template_dir.join("CameraExtension.Info.plist");

    fs::write(&readiness_path, &readiness.markdown)?;
    fs::write(&plan_path, camera_extension_plan_markdown(target, &root))?;
    fs::write(&app_entitlements_path, camera_container_app_entitlements())?;
    fs::write(
        &extension_entitlements_path,
        camera_extension_entitlements(),
    )?;
    fs::write(&info_plist_path, camera_extension_info_plist())?;

    println!("Camera Extension prototype plan written.");
    println!("Target: {}", target.label());
    println!("Plan: {}", relative_display(&root, &plan_path));
    println!(
        "Info.plist template: {}",
        relative_display(&root, &info_plist_path)
    );
    println!(
        "Entitlements: {}, {}",
        relative_display(&root, &app_entitlements_path),
        relative_display(&root, &extension_entitlements_path)
    );
    println!("Camera name: {VIRTUAL_CAMERA_NAME}");
    println!("Bundle id: {VIRTUAL_CAMERA_EXTENSION_BUNDLE_ID}");
    println!("Readiness: {}", readiness.status_label());
    Ok(())
}

fn virtual_camera_readiness(args: Vec<String>) -> Result<()> {
    let target = parse_virtual_camera_readiness_args(args)?;
    let root = project_root()?;
    let config_path = root.join(target.config_path());
    let report_dir = root.join("target/virtual-camera");
    let report_path = report_dir.join("readiness.md");
    fs::create_dir_all(&report_dir)?;

    let report = build_virtual_camera_readiness_report(&root, target, &config_path)?;
    fs::write(&report_path, &report.markdown)?;

    println!("Virtual camera readiness report written.");
    println!("Target: {}", target.label());
    println!("Report: {}", relative_display(&root, &report_path));
    println!("Status: {}", report.status_label());
    println!("Next: {}", report.next_action);
    Ok(())
}

fn tune_input(args: Vec<String>) -> Result<()> {
    let (target, input, preset) = parse_tune_input_args(args)?;

    let root = project_root()?;
    let config_path = root.join(target.config_path());
    let example_config_path = root.join(target.example_config_path());
    let content = if config_path.is_file() {
        fs::read_to_string(&config_path)?
    } else if example_config_path.is_file() {
        fs::read_to_string(&example_config_path)?
    } else {
        String::new()
    };

    let (section, updates) = input.preset_updates(preset);
    let updated = set_toml_section_values(&content, section, &updates);
    fs::write(&config_path, updated)?;

    println!("Input tuning updated.");
    println!("Target: {}", target.label());
    println!("Config: {}", relative_display(&root, &config_path));
    println!("Input: {}", input.config_label());
    println!("Preset: {}", preset.config_label());
    println!(
        "Run with: cargo xtask run-metal{}",
        if matches!(target, SelectModelTarget::Build) {
            " --release"
        } else {
            ""
        }
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum SelectModelTarget {
    Development,
    Build,
}

impl SelectModelTarget {
    fn config_path(self) -> &'static str {
        match self {
            Self::Development => DEVELOPMENT_CONFIG_PATH,
            Self::Build => BUILD_CONFIG_PATH,
        }
    }

    fn example_config_path(self) -> &'static str {
        match self {
            Self::Development => DEVELOPMENT_EXAMPLE_CONFIG_PATH,
            Self::Build => BUILD_EXAMPLE_CONFIG_PATH,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Build => "build",
        }
    }

    fn flag_name(self) -> &'static str {
        match self {
            Self::Development => "dev",
            Self::Build => "build",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TuneInputTarget {
    Mouse,
    Mouth,
    Camera,
}

impl TuneInputTarget {
    fn from_arg(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mouse" => Some(Self::Mouse),
            "mouth" | "microphone" | "mic" => Some(Self::Mouth),
            "camera" => Some(Self::Camera),
            _ => None,
        }
    }

    fn config_label(self) -> &'static str {
        match self {
            Self::Mouse => "mouse",
            Self::Mouth => "mouth",
            Self::Camera => "camera",
        }
    }

    fn preset_updates(self, preset: TunePreset) -> (&'static str, Vec<(&'static str, String)>) {
        match self {
            Self::Mouse => ("input.mouse", mouse_tune_updates(preset)),
            Self::Mouth => ("input.microphone", mouth_tune_updates(preset)),
            Self::Camera => ("input.camera", camera_tune_updates(preset)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TunePreset {
    Soft,
    Normal,
    Expressive,
}

impl TunePreset {
    fn from_arg(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "soft" => Some(Self::Soft),
            "normal" => Some(Self::Normal),
            "expressive" | "strong" => Some(Self::Expressive),
            _ => None,
        }
    }

    fn config_label(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Normal => "normal",
            Self::Expressive => "expressive",
        }
    }
}

fn mouse_tune_updates(preset: TunePreset) -> Vec<(&'static str, String)> {
    let (smoothing, eye, angle_scale) = match preset {
        TunePreset::Soft => (7.5, 0.65, 0.65),
        TunePreset::Normal => (10.0, 1.0, 1.0),
        TunePreset::Expressive => (12.5, 1.35, 1.35),
    };
    vec![
        ("enabled", "true".to_string()),
        ("smoothing", format_decimal(smoothing)),
        ("eye_x_range", format_decimal(eye)),
        ("eye_y_range", format_decimal(eye)),
        ("angle_x_degrees", format_decimal(30.0 * angle_scale)),
        ("angle_y_degrees", format_decimal(22.0 * angle_scale)),
        ("angle_z_degrees", format_decimal(-12.0 * angle_scale)),
    ]
}

fn mouth_tune_updates(preset: TunePreset) -> Vec<(&'static str, String)> {
    let (gain, response_curve, attack, release, max_open) = match preset {
        TunePreset::Soft => (6.5, 0.75, 25.6, 8.0, 0.75),
        TunePreset::Normal => (10.0, 0.6, 32.0, 10.0, 1.0),
        TunePreset::Expressive => (14.5, 0.45, 40.0, 11.5, 1.0),
    };
    vec![
        ("enabled", "true".to_string()),
        ("parameter", toml_string_literal("ParamMouthOpenY")),
        ("gain", format_decimal(gain)),
        ("response_curve", format_decimal(response_curve)),
        ("attack", format_decimal(attack)),
        ("release", format_decimal(release)),
        ("min_open", "0.0".to_string()),
        ("max_open", format_decimal(max_open)),
    ]
}

fn camera_tune_updates(preset: TunePreset) -> Vec<(&'static str, String)> {
    let (smoothing, eye, angle_scale, mouth_gain, mouth_max_open) = match preset {
        TunePreset::Soft => (9.6, 0.7, 0.7, 1.05, 0.85),
        TunePreset::Normal => (12.0, 1.0, 1.0, 1.4, 1.0),
        TunePreset::Expressive => (14.4, 1.25, 1.25, 1.89, 1.0),
    };
    vec![
        ("enabled", "true".to_string()),
        ("pose_mode", toml_string_literal("camera_when_available")),
        ("smoothing", format_decimal(smoothing)),
        ("eye_x_range", format_decimal(eye)),
        ("eye_y_range", format_decimal(eye)),
        ("angle_x_degrees", format_decimal(30.0 * angle_scale)),
        ("angle_y_degrees", format_decimal(22.0 * angle_scale)),
        ("angle_z_degrees", format_decimal(12.0 * angle_scale)),
        ("mouth_gain", format_decimal(mouth_gain)),
        ("mouth_min_open", "0.0".to_string()),
        ("mouth_max_open", format_decimal(mouth_max_open)),
        ("mouth_combine", toml_string_literal("max")),
    ]
}

fn format_decimal(value: f64) -> String {
    format!("{value:.2}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

#[derive(Debug, Default)]
struct DoctorLocalCheck {
    issues: usize,
}

fn check_local_config(root: &Path, target: SelectModelTarget) -> Result<DoctorLocalCheck> {
    let config_path = root.join(target.config_path());
    let display_path = relative_display(root, &config_path);
    if !config_path.is_file() {
        println!("[!] {} config missing: {}", target.label(), display_path);
        println!(
            "    Create it with: cp {} {}",
            target.example_config_path(),
            target.config_path()
        );
        return Ok(DoctorLocalCheck { issues: 1 });
    }

    let content = fs::read_to_string(&config_path)?;
    let config: DoctorConfig = match toml::from_str(&content) {
        Ok(config) => config,
        Err(error) => {
            println!(
                "[!] {} config parse failed: {}",
                target.label(),
                display_path
            );
            println!("    {error}");
            return Ok(DoctorLocalCheck { issues: 1 });
        }
    };

    let issues = check_doctor_app_config(target, &config.app)
        + check_doctor_capture_config(target, &config.capture)
        + check_doctor_renderer_config(target, &config.renderer)
        + check_doctor_motion_config(target, &config.motion)
        + check_doctor_input_config(target, &config.input);

    let Some(model_path) = config.model.path.as_deref() else {
        println!("[!] {} config has no [model].path", target.label());
        println!(
            "    Run: cargo xtask select-model --{} MODEL_PATH",
            target.flag_name()
        );
        return Ok(DoctorLocalCheck { issues: issues + 1 });
    };

    let full_model_path = root.join(model_path);
    if !is_model3_path(&full_model_path) {
        println!(
            "[!] {} model path is not a .model3.json: {model_path}",
            target.label()
        );
        println!("    Run: cargo xtask list-models");
        println!(
            "    Then: cargo xtask select-model --{} MODEL_PATH",
            target.flag_name()
        );
        return Ok(DoctorLocalCheck { issues: issues + 1 });
    }
    if !full_model_path.is_file() {
        println!(
            "[!] {} selected model missing: {model_path}",
            target.label()
        );
        println!("    Run: cargo xtask list-models");
        println!(
            "    Then: cargo xtask select-model --{} MODEL_PATH",
            target.flag_name()
        );
        return Ok(DoctorLocalCheck { issues: issues + 1 });
    }

    match ModelManifestSummary::load(&full_model_path) {
        Ok(summary) => {
            println!(
                "[x] {} config: {} -> {} (textures {}, motions {}, expressions {}, physics {}, display {})",
                target.label(),
                display_path,
                model_path,
                summary.texture_count,
                summary.motion_count,
                summary.expression_count,
                yes_no(summary.has_physics),
                yes_no(summary.has_display_info)
            );
            Ok(DoctorLocalCheck { issues })
        }
        Err(error) => {
            println!(
                "[!] {} selected model manifest is invalid: {model_path}",
                target.label()
            );
            println!("    {error}");
            Ok(DoctorLocalCheck { issues: issues + 1 })
        }
    }
}

fn check_doctor_app_config(target: SelectModelTarget, app: &DoctorAppConfig) -> usize {
    let mut issues = 0usize;
    for (field, value) in [
        ("window_width", app.window_width),
        ("window_height", app.window_height),
    ] {
        if let Some(value) = value {
            if !valid_doctor_window_dimension(value) {
                println!(
                    "[!] {} {field} should be a finite value from 96.0 to 2400.0, got {value}",
                    target.label()
                );
                issues += 1;
            }
        }
    }

    if issues == 0 {
        println!(
            "[x] {} window size config: width {} | height {} | capture_friendly {}",
            target.label(),
            app.window_width
                .map(|value| format!("{value:.1}"))
                .unwrap_or_else(|| "default".to_string()),
            app.window_height
                .map(|value| format!("{value:.1}"))
                .unwrap_or_else(|| "default".to_string()),
            on_off(app.window_capture_friendly)
        );
    }

    issues
}

fn valid_doctor_window_dimension(value: f64) -> bool {
    value.is_finite() && (96.0..=2400.0).contains(&value)
}

fn check_doctor_capture_config(target: SelectModelTarget, capture: &DoctorCaptureConfig) -> usize {
    let mut issues = 0usize;
    let sckit = &capture.screen_capture_kit;
    if let Some(value) = sckit.target_fps {
        if !(1..=60).contains(&value) {
            println!(
                "[!] {} capture.screen_capture_kit.target_fps should be from 1 to 60, got {value}",
                target.label()
            );
            issues += 1;
        }
    }
    issues += check_optional_range(
        target,
        "capture.screen_capture_kit.log_interval_seconds",
        sckit.log_interval_seconds,
        0.25,
        60.0,
    );
    issues += check_optional_range(
        target,
        "capture.screen_capture_kit.stalled_after_seconds",
        sckit.stalled_after_seconds,
        0.25,
        60.0,
    );
    println!(
        "[x] {} ScreenCaptureKit probe config: enabled {} | fps {} | log {} | stall {}",
        target.label(),
        sckit
            .enabled
            .map(|value| if value { "on" } else { "off" })
            .unwrap_or("profile-default"),
        sckit
            .target_fps
            .map(|value| value.to_string())
            .unwrap_or_else(|| "default".to_string()),
        optional_seconds_label(sckit.log_interval_seconds),
        optional_seconds_label(sckit.stalled_after_seconds)
    );
    issues
}

fn optional_seconds_label(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.1}s"))
        .unwrap_or_else(|| "default".to_string())
}

fn check_doctor_renderer_config(
    target: SelectModelTarget,
    renderer: &DoctorRendererConfig,
) -> usize {
    let mut issues = 0usize;
    if let Some(value) = renderer.debug_texture_mode.as_deref() {
        if normalized_debug_texture_mode(value).is_none() {
            println!(
                "[!] {} renderer.debug_texture_mode is invalid: {:?}",
                target.label(),
                value
            );
            println!("    Use debug_texture_mode = \"none\", \"uv\", \"rgb\", or \"alpha\"");
            issues += 1;
        }
    }
    if let Some(value) = renderer.atlas_anisotropy {
        if !(1..=16).contains(&value) {
            println!(
                "[!] {} renderer.atlas_anisotropy should be from 1 to 16, got {value}",
                target.label()
            );
            issues += 1;
        }
    }
    println!(
        "[x] {} renderer config: debug_texture_mode {} | anisotropy {}",
        target.label(),
        renderer
            .debug_texture_mode
            .as_deref()
            .and_then(normalized_debug_texture_mode)
            .unwrap_or("none"),
        renderer
            .atlas_anisotropy
            .map(|value| value.to_string())
            .unwrap_or_else(|| "default".to_string())
    );
    issues
}

fn check_doctor_motion_config(target: SelectModelTarget, motion: &DoctorMotionConfig) -> usize {
    let mut issues = 0usize;
    issues += check_optional_range(
        target,
        "motion.blink_interval",
        motion.blink_interval,
        0.5,
        60.0,
    );
    issues += check_optional_range(
        target,
        "motion.blink_duration",
        motion.blink_duration,
        0.05,
        5.0,
    );
    if motion
        .expression
        .as_deref()
        .is_some_and(|expression| expression.trim().is_empty())
    {
        println!(
            "[!] {} motion.expression is empty; remove it or set a model expression name/index",
            target.label()
        );
        issues += 1;
    }
    println!(
        "[x] {} motion config: blink_interval {} | blink_duration {}",
        target.label(),
        motion
            .blink_interval
            .map(|value| value.to_string())
            .unwrap_or_else(|| "default".to_string()),
        motion
            .blink_duration
            .map(|value| value.to_string())
            .unwrap_or_else(|| "default".to_string())
    );
    issues
}

fn check_doctor_input_config(target: SelectModelTarget, input: &DoctorInputConfig) -> usize {
    let mut issues = 0usize;
    issues += check_doctor_mouse_config(target, &input.mouse);
    issues += check_doctor_microphone_config(target, &input.microphone);
    issues += check_doctor_camera_config(target, &input.camera);
    println!(
        "[x] {} input config: mouse {} | microphone {} | camera {}",
        target.label(),
        on_off(input.mouse.enabled),
        on_off(input.microphone.enabled),
        on_off(input.camera.enabled)
    );
    issues
}

fn check_doctor_mouse_config(target: SelectModelTarget, mouse: &DoctorMouseConfig) -> usize {
    let mut issues = 0usize;
    if let Some(value) = mouse.coordinate_space.as_deref() {
        if normalized_mouse_coordinate_space(value).is_none() {
            println!(
                "[!] {} input.mouse.coordinate_space is invalid: {:?}",
                target.label(),
                value
            );
            println!("    Use coordinate_space = \"screen\" or \"window\"");
            issues += 1;
        }
    }
    issues += check_optional_range(target, "input.mouse.smoothing", mouse.smoothing, 1.0, 60.0);
    issues += check_optional_range(target, "input.mouse.dead_zone", mouse.dead_zone, 0.0, 0.95);
    issues += check_optional_range(
        target,
        "input.mouse.eye_x_range",
        mouse.eye_x_range,
        0.0,
        3.0,
    );
    issues += check_optional_range(
        target,
        "input.mouse.eye_y_range",
        mouse.eye_y_range,
        0.0,
        3.0,
    );
    issues += check_optional_range(
        target,
        "input.mouse.angle_x_degrees",
        mouse.angle_x_degrees,
        -90.0,
        90.0,
    );
    issues += check_optional_range(
        target,
        "input.mouse.angle_y_degrees",
        mouse.angle_y_degrees,
        -90.0,
        90.0,
    );
    issues += check_optional_range(
        target,
        "input.mouse.angle_z_degrees",
        mouse.angle_z_degrees,
        -90.0,
        90.0,
    );
    issues
}

fn check_doctor_microphone_config(
    target: SelectModelTarget,
    microphone: &DoctorMicrophoneConfig,
) -> usize {
    let mut issues = 0usize;
    if microphone
        .parameter
        .as_deref()
        .is_some_and(|parameter| parameter.trim().is_empty())
    {
        println!(
            "[!] {} input.microphone.parameter is empty; runtime will fall back to ParamMouthOpenY",
            target.label()
        );
        issues += 1;
    }
    issues += check_optional_range(target, "input.microphone.gain", microphone.gain, 0.1, 80.0);
    issues += check_optional_range(
        target,
        "input.microphone.noise_gate",
        microphone.noise_gate,
        0.0,
        0.5,
    );
    issues += check_optional_range(
        target,
        "input.microphone.response_curve",
        microphone.response_curve,
        0.2,
        3.0,
    );
    issues += check_optional_range(
        target,
        "input.microphone.smoothing",
        microphone.smoothing,
        1.0,
        120.0,
    );
    issues += check_optional_range(
        target,
        "input.microphone.attack",
        microphone.attack,
        1.0,
        120.0,
    );
    issues += check_optional_range(
        target,
        "input.microphone.release",
        microphone.release,
        1.0,
        120.0,
    );
    issues += check_optional_range(
        target,
        "input.microphone.min_open",
        microphone.min_open,
        0.0,
        1.0,
    );
    issues += check_optional_range(
        target,
        "input.microphone.max_open",
        microphone.max_open,
        0.0,
        1.0,
    );
    issues
}

fn check_doctor_camera_config(target: SelectModelTarget, camera: &DoctorCameraConfig) -> usize {
    let mut issues = 0usize;
    if let Some(value) = camera.pose_mode.as_deref() {
        if normalized_camera_pose_mode(value).is_none() {
            println!(
                "[!] {} input.camera.pose_mode is invalid: {:?}",
                target.label(),
                value
            );
            println!("    Use pose_mode = \"camera_when_available\", \"camera\", or \"mouse\"");
            issues += 1;
        }
    }
    if let Some(value) = camera.mouth_combine.as_deref() {
        if normalized_mouth_combine_mode(value).is_none() {
            println!(
                "[!] {} input.camera.mouth_combine is invalid: {:?}",
                target.label(),
                value
            );
            println!("    Use mouth_combine = \"max\", \"camera\", or \"microphone\"");
            issues += 1;
        }
    }
    if let Some(value) = camera.target_fps {
        if !(1..=60).contains(&value) {
            println!(
                "[!] {} input.camera.target_fps should be from 1 to 60, got {value}",
                target.label()
            );
            issues += 1;
        }
    }
    issues += check_optional_range(
        target,
        "input.camera.smoothing",
        camera.smoothing,
        1.0,
        60.0,
    );
    issues += check_optional_range(
        target,
        "input.camera.dead_zone",
        camera.dead_zone,
        0.0,
        0.95,
    );
    for (field, value) in [
        ("input.camera.face_x_offset", camera.face_x_offset),
        ("input.camera.face_y_offset", camera.face_y_offset),
        ("input.camera.gaze_x_offset", camera.gaze_x_offset),
        ("input.camera.gaze_y_offset", camera.gaze_y_offset),
        ("input.camera.roll_offset", camera.roll_offset),
    ] {
        issues += check_optional_range(target, field, value, -1.0, 1.0);
    }
    for (field, value) in [
        ("input.camera.eye_x_range", camera.eye_x_range),
        ("input.camera.eye_y_range", camera.eye_y_range),
    ] {
        issues += check_optional_range(target, field, value, 0.0, 3.0);
    }
    for (field, value) in [
        ("input.camera.angle_x_degrees", camera.angle_x_degrees),
        ("input.camera.angle_y_degrees", camera.angle_y_degrees),
        ("input.camera.angle_z_degrees", camera.angle_z_degrees),
    ] {
        issues += check_optional_range(target, field, value, -90.0, 90.0);
    }
    issues += check_optional_range(
        target,
        "input.camera.mouth_gain",
        camera.mouth_gain,
        0.1,
        10.0,
    );
    issues += check_optional_range(
        target,
        "input.camera.mouth_open_offset",
        camera.mouth_open_offset,
        -1.0,
        1.0,
    );
    issues += check_optional_range(
        target,
        "input.camera.mouth_min_open",
        camera.mouth_min_open,
        0.0,
        1.0,
    );
    issues += check_optional_range(
        target,
        "input.camera.mouth_max_open",
        camera.mouth_max_open,
        0.0,
        1.0,
    );
    issues += check_optional_range(
        target,
        "input.camera.blink_close_threshold",
        camera.blink_close_threshold,
        0.0,
        1.0,
    );
    issues += check_optional_range(
        target,
        "input.camera.blink_open_threshold",
        camera.blink_open_threshold,
        0.0,
        1.0,
    );
    if let (Some(close), Some(open)) = (camera.blink_close_threshold, camera.blink_open_threshold) {
        if close >= open {
            println!(
                "[!] {} camera blink thresholds should satisfy blink_close_threshold < blink_open_threshold",
                target.label()
            );
            issues += 1;
        }
    }
    issues
}

fn check_optional_range(
    target: SelectModelTarget,
    field: &str,
    value: Option<f64>,
    min: f64,
    max: f64,
) -> usize {
    let Some(value) = value else {
        return 0;
    };
    if value.is_finite() && (min..=max).contains(&value) {
        return 0;
    }
    println!(
        "[!] {} {field} should be a finite value from {min} to {max}, got {value}",
        target.label()
    );
    1
}

fn normalized_mouse_coordinate_space(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "screen" => Some("screen"),
        "window" => Some("window"),
        _ => None,
    }
}

fn normalized_debug_texture_mode(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "none" => Some("none"),
        "uv" => Some("uv"),
        "rgb" | "texture" | "color" => Some("rgb"),
        "alpha" => Some("alpha"),
        _ => None,
    }
}

fn normalized_camera_pose_mode(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "mouse" => Some("mouse"),
        "camera" | "face" => Some("camera"),
        "" | "camera_when_available" | "camera-when-available" | "auto" => {
            Some("camera_when_available")
        }
        _ => None,
    }
}

fn normalized_mouth_combine_mode(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "max" => Some("max"),
        "camera" => Some("camera"),
        "microphone" | "mic" => Some("microphone"),
        _ => None,
    }
}

fn on_off(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "on",
        Some(false) => "off",
        None => "default",
    }
}

fn check_cubism_core_sdk(root: &Path) -> usize {
    match cubism_core_paths(root) {
        Ok((include_dir, lib_dir)) => {
            println!(
                "[x] Cubism Core SDK: include={} lib={}",
                relative_display(root, &include_dir),
                relative_display(root, &lib_dir)
            );
            0
        }
        Err(error) => {
            println!("[!] Cubism Core SDK missing or incomplete");
            println!("    {error}");
            println!(
                "    Expected default location: public/CubismSdkForNative/Core/include and public/CubismSdkForNative/Core/lib/macos/{}",
                host_arch_lib_dir().unwrap_or("arm64")
            );
            println!(
                "    Or set LIVE2D_CUBISM_SDK_NATIVE_DIR, CUBISM_CORE_INCLUDE_DIR, or CUBISM_CORE_LIB_DIR."
            );
            1
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DoctorConfig {
    app: DoctorAppConfig,
    capture: DoctorCaptureConfig,
    input: DoctorInputConfig,
    model: DoctorModelConfig,
    motion: DoctorMotionConfig,
    renderer: DoctorRendererConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DoctorAppConfig {
    window_width: Option<f64>,
    window_height: Option<f64>,
    window_capture_friendly: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DoctorCaptureConfig {
    screen_capture_kit: DoctorScreenCaptureKitConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DoctorScreenCaptureKitConfig {
    enabled: Option<bool>,
    target_fps: Option<u32>,
    log_interval_seconds: Option<f64>,
    stalled_after_seconds: Option<f64>,
}

struct VirtualCameraReadinessReport {
    markdown: String,
    ready_for_extension_prototype: bool,
    next_action: String,
}

impl VirtualCameraReadinessReport {
    fn status_label(&self) -> &'static str {
        if self.ready_for_extension_prototype {
            "ready for extension prototype"
        } else {
            "setup incomplete"
        }
    }
}

#[derive(Debug, Default)]
struct InternalOutputReadiness {
    mode: String,
    producer: String,
    activate_virtual_camera: bool,
    manifest_path: String,
    manifest_exists: bool,
    manifest_frames: Option<u64>,
    manifest_surface_id: Option<u64>,
    manifest_size: Option<(u64, u64)>,
    manifest_pixel_format: Option<String>,
    manifest_frame_rate: Option<u64>,
    manifest_updated_unix_ms: Option<u64>,
}

fn build_virtual_camera_readiness_report(
    root: &Path,
    target: SelectModelTarget,
    config_path: &Path,
) -> Result<VirtualCameraReadinessReport> {
    let output = inspect_internal_output_readiness(root, config_path)?;
    let platform_ok = env::consts::OS == "macos";
    let app_bundle_path = root.join("target/dev-app/vtube-studio-rs Dev.app");
    let app_bundle_exists = app_bundle_path.is_dir();
    let embedded_extension_path = app_bundle_path
        .join("Contents/Library/SystemExtensions")
        .join(VIRTUAL_CAMERA_BUNDLE_NAME);
    let embedded_extension_exists = embedded_extension_path.is_dir();
    let installed_app_path = PathBuf::from("/Applications").join(DEV_APP_BUNDLE_NAME);
    let installed_app_exists = installed_app_path.is_dir();
    let installed_extension_path = installed_app_path
        .join("Contents/Library/SystemExtensions")
        .join(VIRTUAL_CAMERA_BUNDLE_NAME);
    let installed_extension_exists = installed_extension_path.is_dir();
    let installed_app_display = installed_app_path.display().to_string();
    let codesign_identity = camera_codesign_identity();
    let has_real_codesign_identity = codesign_identity != "-";
    let installed_app_entitlements = signed_entitlements_text(&installed_app_path);
    let app_has_system_extension_entitlement = installed_app_entitlements
        .as_deref()
        .is_some_and(|text| text.contains("com.apple.developer.system-extension.install"));
    let installed_app_profile_exists = installed_app_path
        .join("Contents/embedded.provisionprofile")
        .is_file();
    let installed_extension_profile_exists = installed_extension_path
        .join("Contents/embedded.provisionprofile")
        .is_file();
    let installed_app_profile_summary = embedded_provisioning_profile_summary(
        &installed_app_path,
        ProvisioningProfileKind::ContainerApp,
    );
    let installed_extension_profile_summary = embedded_provisioning_profile_summary(
        &installed_extension_path,
        ProvisioningProfileKind::CameraExtension,
    );
    let installed_app_profile_valid = installed_app_profile_summary.is_some();
    let installed_extension_profile_valid = installed_extension_profile_summary.is_some();
    let system_extension_active = camera_system_extension_active();
    let manifest_contract_ok = output.manifest_size == Some((1080, 1080))
        && output.manifest_pixel_format.as_deref() == Some("BGRA8Unorm")
        && output.manifest_frame_rate == Some(60);
    let internal_ready = output.mode == "internal"
        && output.producer == "iosurface"
        && output.activate_virtual_camera
        && output.manifest_exists
        && output.manifest_frames.unwrap_or(0) > 0
        && manifest_contract_ok;
    let ready_for_extension_prototype = platform_ok
        && internal_ready
        && embedded_extension_exists
        && installed_app_exists
        && installed_extension_exists
        && app_has_system_extension_entitlement
        && installed_app_profile_valid
        && installed_extension_profile_valid
        && has_real_codesign_identity
        && system_extension_active;
    let manifest_size = output
        .manifest_size
        .map(|(width, height)| format!("{width}x{height}"))
        .unwrap_or_else(|| "unknown".to_string());
    let manifest_frames = output
        .manifest_frames
        .map(|frames| frames.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let manifest_surface_id = output
        .manifest_surface_id
        .map(|surface_id| surface_id.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let manifest_pixel_format = output
        .manifest_pixel_format
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let manifest_frame_rate = output
        .manifest_frame_rate
        .map(|frame_rate| format!("{frame_rate} fps"))
        .unwrap_or_else(|| "unknown".to_string());
    let manifest_updated = output
        .manifest_updated_unix_ms
        .map(|updated| updated.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let config_display = relative_display(root, config_path);
    let app_bundle_display = relative_display(root, &app_bundle_path);
    let embedded_extension_display = relative_display(root, &embedded_extension_path);
    let manifest_full_path = absolute_path(root, &output.manifest_path);
    let manifest_display = relative_display(root, &manifest_full_path);
    let status = if ready_for_extension_prototype {
        "ready for extension prototype"
    } else {
        "setup incomplete"
    };
    let next_action = virtual_camera_next_action(
        &output,
        platform_ok,
        app_bundle_exists,
        embedded_extension_exists,
        has_real_codesign_identity,
        installed_app_exists,
        app_has_system_extension_entitlement,
        installed_app_profile_exists,
        installed_extension_profile_exists,
        installed_app_profile_valid,
        installed_extension_profile_valid,
        system_extension_active,
    );
    let container_profile_detail = profile_readiness_detail(
        installed_app_profile_exists,
        installed_app_profile_summary.as_ref(),
    );
    let extension_profile_detail = profile_readiness_detail(
        installed_extension_profile_exists,
        installed_extension_profile_summary.as_ref(),
    );
    let markdown = format!(
        "# Virtual Camera Readiness\n\n\
Generated for `{target_label}` profile.\n\n\
Status: **{status}**\n\n\
## Product Boundary\n\n\
- The project will not ship an OBS-specific plugin.\n\
- The no-desktop path stays inside vtube-studio-rs as a macOS virtual camera output.\n\
- OBS, QuickRecord, Zoom, Discord, and similar apps should eventually consume the same system camera source.\n\n\
## Current Checks\n\n\
| Check | Status | Detail |\n\
| --- | --- | --- |\n\
| macOS host | {platform_status} | `{os}` |\n\
| Camera API direction | ok | CoreMediaIO Camera Extension, not legacy DAL or OBS plugin |\n\
| Rust CMIO bindings | ok | `objc2-core-media-io` + `objc2-core-video` behind `virtual-camera-extension` |\n\
| Active config | ok | `{config_display}` |\n\
| Output mode | {mode_status} | `{mode}` |\n\
| Internal producer | {producer_status} | `{producer}` |\n\
| Camera activation | {activation_status} | `{activate_virtual_camera}` |\n\
| IOSurface manifest | {manifest_status} | `{manifest_display}` |\n\
| IOSurface id | {surface_status} | `{surface_id}` |\n\
| Internal frame count | {frames_status} | `{frames}` |\n\
| Texture size | {size_status} | `{size}` |\n\
| Camera pixel format | {pixel_format_status} | `{pixel_format}` |\n\
| Camera frame rate | {frame_rate_status} | `{frame_rate}` |\n\
| Manifest updated | {updated_status} | `{updated_unix_ms}` |\n\
| App wrapper | {bundle_status} | `{bundle}` |\n\
| Embedded Camera Extension | {embedded_extension_status} | `{embedded_extension}` |\n\
| Installed app wrapper | {installed_app_status} | `{installed_app}` |\n\
| Installed Camera Extension | {installed_extension_status} | `{installed_extension}` |\n\
| System extension install entitlement | {system_extension_entitlement_status} | `{system_extension_entitlement}` |\n\
| Container provisioning profile | {container_profile_status} | `{container_profile}` |\n\
| Extension provisioning profile | {extension_profile_status} | `{extension_profile}` |\n\
| System Extension active | {system_extension_active_status} | `{system_extension_active}` |\n\
| Codesign identity | {codesign_status} | `{codesign}` |\n\n\
## Next Implementation Slice\n\n\
1. Keep the existing internal IOSurface producer as the frame source.\n\
2. Build and embed the macOS Camera Extension target owned by this project.\n\
3. Use `VT -> OBS / Recording Output -> Apply System Camera Source...` or `cargo xtask configure-internal-output --{target_flag}` so IOSurface output and Camera Extension activation are enabled together for `{extension_bundle_id}`.\n\
4. Feed the extension from the IOSurface manifest/producer bridge as `1080x1080 60fps BGRA` sample buffers.\n\
5. Register one camera named `VTube Studio RS Camera` and validate in QuickRecord, then OBS.\n\n\
Generate the prototype bundle templates with `cargo xtask camera-extension-plan --{target_flag}`.\n\n\
## Setup Commands\n\n\
```bash\n\
cargo xtask configure-internal-output --{target_flag}\n\
cargo xtask run-metal{run_release_flag}\n\
cargo xtask virtual-camera-readiness --{target_flag}\n\
cargo xtask camera-extension-plan --{target_flag}\n\
cargo xtask build-camera-extension --{target_flag}\n\
cargo xtask build-app{run_release_flag}\n\
```\n\n\
If `Codesign identity` is `warn`, set `VTUBE_RS_CODESIGN_IDENTITY` to an Apple Development or Developer ID Application identity before building a real Camera Extension.\n",
        target_label = target.label(),
        status = status,
        platform_status = readiness_status(platform_ok),
        os = env::consts::OS,
        config_display = config_display,
        mode_status = readiness_status(output.mode == "internal"),
        mode = output.mode,
        producer_status = readiness_status(output.producer == "iosurface"),
        producer = output.producer,
        activation_status = readiness_status(output.activate_virtual_camera),
        activate_virtual_camera = output.activate_virtual_camera,
        manifest_status = readiness_status(output.manifest_exists),
        manifest_display = manifest_display,
        surface_status = readiness_status(output.manifest_surface_id.is_some()),
        surface_id = manifest_surface_id,
        frames_status = readiness_status(output.manifest_frames.unwrap_or(0) > 0),
        frames = manifest_frames,
        size_status = readiness_status(output.manifest_size == Some((1080, 1080))),
        size = manifest_size,
        pixel_format_status =
            readiness_status(output.manifest_pixel_format.as_deref() == Some("BGRA8Unorm")),
        pixel_format = manifest_pixel_format,
        frame_rate_status = readiness_status(output.manifest_frame_rate == Some(60)),
        frame_rate = manifest_frame_rate,
        updated_status = readiness_status(output.manifest_updated_unix_ms.is_some()),
        updated_unix_ms = manifest_updated,
        bundle_status = readiness_status(app_bundle_exists),
        bundle = app_bundle_display,
        embedded_extension_status = readiness_status(embedded_extension_exists),
        embedded_extension = embedded_extension_display,
        installed_app_status = readiness_status(installed_app_exists),
        installed_app = installed_app_display,
        installed_extension_status = readiness_status(installed_extension_exists),
        installed_extension = installed_extension_path.display(),
        system_extension_entitlement_status =
            readiness_status(app_has_system_extension_entitlement),
        system_extension_entitlement = app_has_system_extension_entitlement,
        container_profile_status = readiness_status(installed_app_profile_valid),
        container_profile = container_profile_detail,
        extension_profile_status = readiness_status(installed_extension_profile_valid),
        extension_profile = extension_profile_detail,
        system_extension_active_status = readiness_status(system_extension_active),
        system_extension_active = system_extension_active,
        codesign_status = if has_real_codesign_identity {
            "ok"
        } else {
            "warn"
        },
        codesign = codesign_identity,
        extension_bundle_id = VIRTUAL_CAMERA_EXTENSION_BUNDLE_ID,
        target_flag = target.flag_name(),
        run_release_flag = if matches!(target, SelectModelTarget::Build) {
            " --release"
        } else {
            ""
        },
    );

    Ok(VirtualCameraReadinessReport {
        markdown,
        ready_for_extension_prototype,
        next_action,
    })
}

fn virtual_camera_next_action(
    output: &InternalOutputReadiness,
    platform_ok: bool,
    app_bundle_exists: bool,
    embedded_extension_exists: bool,
    has_real_codesign_identity: bool,
    installed_app_exists: bool,
    app_has_system_extension_entitlement: bool,
    installed_app_profile_exists: bool,
    installed_extension_profile_exists: bool,
    installed_app_profile_valid: bool,
    installed_extension_profile_valid: bool,
    system_extension_active: bool,
) -> String {
    if !platform_ok {
        return "run this readiness check on macOS.".to_string();
    }
    if output.mode != "internal"
        || output.producer != "iosurface"
        || !output.activate_virtual_camera
    {
        return "run cargo xtask configure-internal-output --build.".to_string();
    }
    if !output.manifest_exists || output.manifest_frames.unwrap_or(0) == 0 {
        return "run cargo xtask run-metal --release once to create and update the IOSurface manifest.".to_string();
    }
    if output.manifest_size != Some((1080, 1080))
        || output.manifest_pixel_format.as_deref() != Some("BGRA8Unorm")
        || output.manifest_frame_rate != Some(60)
    {
        return "rebuild and rerun cargo xtask run-metal --release so the IOSurface manifest uses 1080x1080 BGRA at 60fps.".to_string();
    }
    if !app_bundle_exists || !embedded_extension_exists {
        return "run cargo xtask build-app --release to embed the Camera Extension into the app wrapper.".to_string();
    }
    if !has_real_codesign_identity {
        return "install an Apple Development or Developer ID Application certificate in Xcode/Keychain, then run cargo xtask build-app --release; xtask will auto-detect it.".to_string();
    }
    if !installed_app_exists {
        return "run cargo xtask build-app --release so the signed app wrapper is copied to /Applications for System Extension activation.".to_string();
    }
    if !app_has_system_extension_entitlement {
        return "rebuild the app so the /Applications copy is signed with com.apple.developer.system-extension.install.".to_string();
    }
    if !installed_app_profile_exists || !installed_extension_profile_exists {
        return format!(
            "the app is signed, but embedded provisioning profiles are missing. Place Apple Developer Program profiles at `{DEFAULT_CONTAINER_PROVISION_PROFILE}` and `{DEFAULT_CAMERA_EXTENSION_PROVISION_PROFILE}` or set `{CONTAINER_PROVISION_PROFILE_ENV}` and `{CAMERA_EXTENSION_PROVISION_PROFILE_ENV}`, then run cargo xtask build-app --release."
        );
    }
    if !installed_app_profile_valid || !installed_extension_profile_valid {
        return "embedded provisioning profiles are present but do not match the required bundle ids, app group, or System Extension entitlement; replace them and run cargo xtask build-app --release.".to_string();
    }
    if !system_extension_active {
        return "launch the /Applications app, approve the Camera Extension in System Settings > General > Login Items & Extensions > Camera Extensions, then re-run this readiness check.".to_string();
    }
    "test VTube Studio RS Camera in QuickRecord or OBS.".to_string()
}

fn profile_readiness_detail(exists: bool, summary: Option<&ProvisioningProfileSummary>) -> String {
    match (exists, summary) {
        (false, _) => "missing".to_string(),
        (true, Some(summary)) => {
            let name = summary.name.as_deref().unwrap_or("unknown");
            let team = summary.team_identifier.as_deref().unwrap_or("unknown");
            format!(
                "valid name={name} app_id={} team={team}",
                summary.application_identifier
            )
        }
        (true, None) => "present but invalid".to_string(),
    }
}

fn signed_entitlements_text(bundle_path: &Path) -> Option<String> {
    if !bundle_path.exists() {
        return None;
    }
    let output = Command::new("codesign")
        .arg("-d")
        .arg("--entitlements")
        .arg(":-")
        .arg(bundle_path)
        .output()
        .ok()?;
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Some(text)
}

fn camera_system_extension_active() -> bool {
    let output = Command::new("systemextensionsctl").arg("list").output();
    let Ok(output) = output else {
        return false;
    };
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text.lines().any(|line| {
        line.contains(VIRTUAL_CAMERA_EXTENSION_BUNDLE_ID) && line.contains("[activated enabled]")
    })
}

fn camera_extension_plan_markdown(target: SelectModelTarget, root: &Path) -> String {
    let prototype_dir = root.join("target/virtual-camera/camera-extension-prototype");
    format!(
        "# CoreMediaIO Camera Extension Prototype\n\n\
Generated for `{target}` profile.\n\n\
## Decision\n\n\
- Use Apple's modern CoreMediaIO Camera Extension path, not legacy DAL, Syphon, NDI, or an OBS plugin.\n\
- Keep the main app as the frame producer: Live2D -> Metal -> IOSurface manifest.\n\
- Expose one system camera named `{camera_name}` so OBS, QuickRecord, Zoom, and other apps consume the same source.\n\n\
## Rust Binding Stack\n\n\
| Layer | Rust crate | Role |\n\
| --- | --- | --- |\n\
| CoreMediaIO extension API | `objc2-core-media-io = 0.3.2` | Provider, device, stream, and CMIO sample buffers |\n\
| CoreVideo frame bridge | `objc2-core-video = 0.3.2` | CVPixelBuffer / IOSurface handoff for frames |\n\
| Existing producer | `iosurface-output` feature | Metal render target backed by IOSurface |\n\n\
## Identifiers\n\n\
| Item | Value |\n\
| --- | --- |\n\
| Camera localized name | `{camera_name}` |\n\
| Extension bundle id | `{bundle_id}` |\n\
| Mach service | `{mach_service}` |\n\
| App group | `{app_group}` |\n\
| Producer manifest | `target/internal-output/iosurface.json` |\n\n\
## Generated Templates\n\n\
- `{prototype_dir}/CameraExtension.Info.plist`\n\
- `{prototype_dir}/CameraExtension.entitlements`\n\
- `{prototype_dir}/ContainerApp.entitlements`\n\n\
These files are templates for the next implementation slice. The current app wrapper embeds the prototype `.systemextension` and exposes a first-pass `OSSystemExtensionManager` activation menu item. The extension now starts the CMIO provider service and contains a first-pass IOSurface -> CVPixelBuffer -> CMSampleBuffer sender, but a finished virtual camera still needs validation from `/Applications` with a real signing identity plus consumer testing.\n\n\
## Implementation Checklist\n\n\
- [x] Add a Rust Camera Extension target or bundle step that builds a system extension binary.\n\
- [x] Define provider/device/stream contracts, stable UUIDs, BGRA 1080x1080 format, IOSurface manifest input, and stream lifecycle state.\n\
- [x] Implement first-pass `CMIOExtensionProviderSource` bridge class with provider properties.\n\
- [x] Implement first-pass `CMIOExtensionDeviceSource` bridge class with model properties.\n\
- [x] Implement first-pass `CMIOExtensionStreamSource` bridge class with BGRA 1080x1080 60fps format and start/stop logging.\n\
- [x] Wire bridge source classes into a `CMIOExtensionProvider`, device, and stream object graph.\n\
- [x] Start the `CMIOExtensionProvider` service inside the installed extension runtime.\n\
- [x] Open the latest IOSurface id from `target/internal-output/iosurface.json`.\n\
- [x] Convert the IOSurface-backed frame into a `CVPixelBuffer` and then a `CMSampleBuffer`/`CMIOSampleBufferCreate` payload.\n\
- [x] Keep sending frames while the stream is active.\n\
- [ ] Add app-group manifest handoff and a neutral transparent frame when the producer is stale.\n\
- [x] Add app-side activation command for the embedded system extension prototype.\n\
- [ ] Add app-side deactivation/status feedback once the extension delegate is implemented.\n\
- [ ] Validate first in QuickRecord, then OBS as a normal camera source.\n\n\
## Local Commands\n\n\
```bash\n\
cargo xtask configure-internal-output --{flag}\n\
cargo xtask run-metal{release_flag}\n\
cargo xtask virtual-camera-readiness --{flag}\n\
cargo xtask camera-extension-plan --{flag}\n\
cargo xtask build-camera-extension --{flag}\n\
cargo test --features \"virtual-camera-extension\"\n\
```\n",
        target = target.label(),
        camera_name = VIRTUAL_CAMERA_NAME,
        bundle_id = VIRTUAL_CAMERA_EXTENSION_BUNDLE_ID,
        mach_service = VIRTUAL_CAMERA_MACH_SERVICE,
        app_group = VIRTUAL_CAMERA_APP_GROUP,
        prototype_dir = relative_display(root, &prototype_dir),
        flag = target.flag_name(),
        release_flag = if matches!(target, SelectModelTarget::Build) {
            " --release"
        } else {
            ""
        },
    )
}

fn camera_extension_info_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>CameraExtension</string>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleName</key>
    <string>VTube Studio RS Camera Extension</string>
    <key>CFBundlePackageType</key>
    <string>SYSX</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CMIOExtensionMachServiceName</key>
    <string>{mach_service}</string>
    <key>NSSystemExtensionUsageDescription</key>
    <string>Publishes VTube Studio RS frames as a macOS virtual camera.</string>
</dict>
</plist>
"#,
        bundle_id = VIRTUAL_CAMERA_EXTENSION_BUNDLE_ID,
        mach_service = VIRTUAL_CAMERA_MACH_SERVICE
    )
}

fn camera_container_app_entitlements() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.developer.system-extension.install</key>
    <true/>
    <key>com.apple.security.application-groups</key>
    <array>
        <string>{app_group}</string>
    </array>
</dict>
</plist>
"#,
        app_group = VIRTUAL_CAMERA_APP_GROUP
    )
}

fn camera_extension_entitlements() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.app-sandbox</key>
    <true/>
    <key>com.apple.security.application-groups</key>
    <array>
        <string>{app_group}</string>
    </array>
</dict>
</plist>
"#,
        app_group = VIRTUAL_CAMERA_APP_GROUP
    )
}

fn inspect_internal_output_readiness(
    root: &Path,
    config_path: &Path,
) -> Result<InternalOutputReadiness> {
    let content = fs::read_to_string(config_path).unwrap_or_default();
    let config: toml::Value =
        toml::from_str(&content).unwrap_or_else(|_| toml::Value::Table(Default::default()));
    let output = config.get("output");
    let internal = output.and_then(|value| value.get("internal"));
    let mode = output
        .and_then(|value| value.get("mode"))
        .and_then(toml::Value::as_str)
        .unwrap_or("window")
        .trim()
        .to_ascii_lowercase();
    let producer = internal
        .and_then(|value| value.get("producer"))
        .and_then(toml::Value::as_str)
        .unwrap_or("none")
        .trim()
        .to_ascii_lowercase();
    let activate_virtual_camera = internal
        .and_then(|value| value.get("activate_virtual_camera"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    let manifest_path = internal
        .and_then(|value| value.get("manifest_path"))
        .and_then(toml::Value::as_str)
        .unwrap_or("target/internal-output/iosurface.json")
        .trim()
        .to_string();
    let manifest_full_path = absolute_path(root, &manifest_path);
    let manifest = read_iosurface_manifest_summary(&manifest_full_path);

    Ok(InternalOutputReadiness {
        mode,
        producer,
        activate_virtual_camera,
        manifest_path,
        manifest_exists: manifest_full_path.is_file(),
        manifest_frames: manifest.as_ref().and_then(|value| value.frames),
        manifest_surface_id: manifest.as_ref().and_then(|value| value.surface_id),
        manifest_size: manifest.as_ref().and_then(|value| value.size),
        manifest_pixel_format: manifest
            .as_ref()
            .and_then(|value| value.pixel_format.clone()),
        manifest_frame_rate: manifest.as_ref().and_then(|value| value.frame_rate),
        manifest_updated_unix_ms: manifest.and_then(|value| value.updated_unix_ms),
    })
}

#[derive(Debug)]
struct IosurfaceManifestSummary {
    frames: Option<u64>,
    surface_id: Option<u64>,
    size: Option<(u64, u64)>,
    pixel_format: Option<String>,
    frame_rate: Option<u64>,
    updated_unix_ms: Option<u64>,
}

fn read_iosurface_manifest_summary(path: &Path) -> Option<IosurfaceManifestSummary> {
    let text = fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let width = json.get("width").and_then(serde_json::Value::as_u64);
    let height = json.get("height").and_then(serde_json::Value::as_u64);
    Some(IosurfaceManifestSummary {
        frames: json.get("frames").and_then(serde_json::Value::as_u64),
        surface_id: json.get("iosurface_id").and_then(serde_json::Value::as_u64),
        size: width.zip(height),
        pixel_format: json
            .get("pixel_format")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        frame_rate: json.get("frame_rate").and_then(serde_json::Value::as_u64),
        updated_unix_ms: json
            .get("updated_unix_ms")
            .and_then(serde_json::Value::as_u64),
    })
}

fn readiness_status(ok: bool) -> &'static str {
    if ok { "ok" } else { "warn" }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DoctorRendererConfig {
    atlas_anisotropy: Option<u64>,
    debug_texture_mode: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DoctorMotionConfig {
    expression: Option<String>,
    blink_interval: Option<f64>,
    blink_duration: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DoctorInputConfig {
    mouse: DoctorMouseConfig,
    microphone: DoctorMicrophoneConfig,
    camera: DoctorCameraConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DoctorMouseConfig {
    enabled: Option<bool>,
    coordinate_space: Option<String>,
    smoothing: Option<f64>,
    dead_zone: Option<f64>,
    eye_x_range: Option<f64>,
    eye_y_range: Option<f64>,
    angle_x_degrees: Option<f64>,
    angle_y_degrees: Option<f64>,
    angle_z_degrees: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DoctorMicrophoneConfig {
    enabled: Option<bool>,
    parameter: Option<String>,
    gain: Option<f64>,
    noise_gate: Option<f64>,
    response_curve: Option<f64>,
    smoothing: Option<f64>,
    attack: Option<f64>,
    release: Option<f64>,
    min_open: Option<f64>,
    max_open: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DoctorCameraConfig {
    enabled: Option<bool>,
    target_fps: Option<u32>,
    pose_mode: Option<String>,
    smoothing: Option<f64>,
    dead_zone: Option<f64>,
    face_x_offset: Option<f64>,
    face_y_offset: Option<f64>,
    gaze_x_offset: Option<f64>,
    gaze_y_offset: Option<f64>,
    roll_offset: Option<f64>,
    eye_x_range: Option<f64>,
    eye_y_range: Option<f64>,
    angle_x_degrees: Option<f64>,
    angle_y_degrees: Option<f64>,
    angle_z_degrees: Option<f64>,
    mouth_gain: Option<f64>,
    mouth_open_offset: Option<f64>,
    mouth_min_open: Option<f64>,
    mouth_max_open: Option<f64>,
    mouth_combine: Option<String>,
    blink_close_threshold: Option<f64>,
    blink_open_threshold: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DoctorModelConfig {
    path: Option<String>,
}

fn parse_select_model_args(args: Vec<String>) -> Result<(SelectModelTarget, String)> {
    match args.as_slice() {
        [model_path] => Ok((SelectModelTarget::Development, model_path.clone())),
        [flag, model_path] if flag == "--dev" || flag == "--development" => {
            Ok((SelectModelTarget::Development, model_path.clone()))
        }
        [flag, model_path] if flag == "--build" => {
            Ok((SelectModelTarget::Build, model_path.clone()))
        }
        _ => Err("usage: cargo xtask select-model [--dev|--build] MODEL_PATH".into()),
    }
}

fn parse_obs_recording_args(args: Vec<String>) -> Result<(SelectModelTarget, ObsWindowPlacement)> {
    let mut target = SelectModelTarget::Build;
    let mut placement = ObsWindowPlacement::Desktop;
    for arg in args {
        match arg.as_str() {
            "--dev" | "--development" => target = SelectModelTarget::Development,
            "--build" => target = SelectModelTarget::Build,
            "--desktop" => placement = ObsWindowPlacement::Desktop,
            "--offscreen" => placement = ObsWindowPlacement::Offscreen,
            _ => {
                return Err(
                    "usage: cargo xtask configure-obs-recording [--dev|--build] [--desktop|--offscreen]"
                        .into(),
                );
            }
        }
    }
    Ok((target, placement))
}

fn parse_internal_output_args(args: Vec<String>) -> Result<SelectModelTarget> {
    match args.as_slice() {
        [] => Ok(SelectModelTarget::Build),
        [flag] if flag == "--dev" || flag == "--development" => Ok(SelectModelTarget::Development),
        [flag] if flag == "--build" => Ok(SelectModelTarget::Build),
        _ => Err("usage: cargo xtask configure-internal-output [--dev|--build]".into()),
    }
}

fn parse_camera_extension_plan_args(args: Vec<String>) -> Result<SelectModelTarget> {
    match args.as_slice() {
        [] => Ok(SelectModelTarget::Build),
        [flag] if flag == "--dev" || flag == "--development" => Ok(SelectModelTarget::Development),
        [flag] if flag == "--build" => Ok(SelectModelTarget::Build),
        _ => Err("usage: cargo xtask camera-extension-plan [--dev|--build]".into()),
    }
}

fn parse_virtual_camera_readiness_args(args: Vec<String>) -> Result<SelectModelTarget> {
    match args.as_slice() {
        [] => Ok(SelectModelTarget::Build),
        [flag] if flag == "--dev" || flag == "--development" => Ok(SelectModelTarget::Development),
        [flag] if flag == "--build" => Ok(SelectModelTarget::Build),
        _ => Err("usage: cargo xtask virtual-camera-readiness [--dev|--build]".into()),
    }
}

fn parse_tune_input_args(
    args: Vec<String>,
) -> Result<(SelectModelTarget, TuneInputTarget, TunePreset)> {
    let (target, rest) = match args.as_slice() {
        [flag, rest @ ..] if flag == "--dev" || flag == "--development" => {
            (SelectModelTarget::Development, rest)
        }
        [flag, rest @ ..] if flag == "--build" => (SelectModelTarget::Build, rest),
        rest => (SelectModelTarget::Development, rest),
    };

    let [input, preset] = rest else {
        return Err(
            "usage: cargo xtask tune-input [--dev|--build] <mouse|mouth|camera> <soft|normal|expressive>"
                .into(),
        );
    };
    let input = TuneInputTarget::from_arg(input).ok_or_else(|| {
        format!("unknown tune-input target `{input}`; use mouse, mouth, or camera")
    })?;
    let preset = TunePreset::from_arg(preset).ok_or_else(|| {
        format!("unknown tune-input preset `{preset}`; use soft, normal, or expressive")
    })?;
    Ok((target, input, preset))
}

fn capture_risk_models_to(
    root: &Path,
    models: Vec<String>,
    output_dir: PathBuf,
    options: CaptureRunOptions,
) -> Result<()> {
    fs::create_dir_all(&output_dir)?;

    if options.clean_before {
        clean(vec!["--generated".to_string()])?;
        fs::create_dir_all(&output_dir)?;
    }

    let probe_models = existing_models(root, models.clone());
    if probe_models.is_empty() {
        eprintln!("Skipping model probe because no configured models were found.");
    } else {
        run_model_probe(root, &probe_models, &output_dir.join("probe.txt"))?;
    }

    for model_path in models {
        if !model_exists(root, &model_path) {
            eprintln!("Skipping missing model: {model_path}");
            continue;
        }

        println!("Capturing {model_path}");
        capture_model_latest(root, &output_dir, &model_path, None)?;
    }

    println!("Render regression screenshots: {}", output_dir.display());
    if options.report_after {
        run_render_regression_report_safe(root)?;
    }
    Ok(())
}

fn capture_renderer_mode(
    root: &Path,
    output_dir: &Path,
    model_path: &str,
    matrix_label: &str,
    label: &str,
    disable_masks: bool,
    high_precision_masks: bool,
) -> Result<()> {
    capture_configured_mode(
        root,
        output_dir,
        model_path,
        matrix_label,
        label,
        &[
            ("disable_masks", disable_masks.to_string()),
            ("high_precision_masks", high_precision_masks.to_string()),
            ("debug_texture_mode", "\"none\"".to_string()),
        ],
    )
}

fn capture_configured_mode(
    root: &Path,
    output_dir: &Path,
    model_path: &str,
    matrix_label: &str,
    label: &str,
    config_updates: &[(&str, String)],
) -> Result<()> {
    let config_path = root.join(DEVELOPMENT_CONFIG_PATH);
    set_toml_values(&config_path, config_updates)?;
    let model_name = model_name_from_path(model_path);
    println!("Capturing {model_name} {matrix_label} {label}");
    capture_model_latest(root, output_dir, model_path, Some(label))
}

fn capture_model_latest(
    root: &Path,
    output_dir: &Path,
    model_path: &str,
    latest_label: Option<&str>,
) -> Result<()> {
    let model_name = model_name_from_path(model_path);
    let captured_path = capture_metal_to(root, model_path, output_dir)?;
    require_file(&captured_path, "Missing captured screenshot")?;

    let latest_path = match latest_label {
        Some(label) => output_dir.join(format!("latest-{model_name}-{label}.png")),
        None => output_dir.join(format!("latest-{model_name}.png")),
    };
    fs::copy(&captured_path, &latest_path)?;
    println!("  {}", captured_path.display());
    println!("  {}", latest_path.display());
    Ok(())
}

fn capture_metal_to(root: &Path, model_path: &str, output_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(output_dir)?;
    let (include_dir, lib_dir) = cubism_core_paths(root)?;
    terminate_app_processes(root);
    let _ = fs::remove_file(root.join("target/vtube-studio-rs.pid"));

    let capture_log_path = output_dir.join("capture.log");
    let capture_log = fs::File::create(&capture_log_path)?;
    let capture_stderr = capture_log.try_clone()?;
    let child = Command::new("cargo")
        .arg("run")
        .arg("--features")
        .arg("metal-renderer")
        .arg("--")
        .arg(model_path)
        .current_dir(root)
        .env("CUBISM_CORE_INCLUDE_DIR", include_dir)
        .env("CUBISM_CORE_LIB_DIR", lib_dir)
        .stdout(Stdio::from(capture_log))
        .stderr(Stdio::from(capture_stderr))
        .spawn()?;
    let mut app = ChildCleanupGuard {
        child,
        pid_path: root.join("target/vtube-studio-rs.pid"),
    };

    let wait_seconds = env_f64("WAIT_SECONDS", 12.0)?;
    let window_id = wait_for_app_window(&mut app, &capture_log_path, wait_seconds)?;
    let post_wait_seconds = env_f64("POST_WINDOW_WAIT_SECONDS", 1.25)?;
    thread::sleep(Duration::from_secs_f64(post_wait_seconds.max(0.0)));

    let model_name = model_name_from_path(model_path);
    let output_path = output_dir.join(format!("{}-{}.png", model_name, timestamp_for_filename()));
    let capture_attempts = env_u32("CAPTURE_ATTEMPTS", 5)?.max(1);
    capture_window_with_retries(
        &window_id,
        &output_path,
        &capture_log_path,
        capture_attempts,
    )?;
    Ok(output_path)
}

struct ChildCleanupGuard {
    child: Child,
    pid_path: PathBuf,
}

impl ChildCleanupGuard {
    fn has_exited(&mut self) -> Result<bool> {
        Ok(self.child.try_wait()?.is_some())
    }
}

impl Drop for ChildCleanupGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.pid_path);
    }
}

struct TailThreadGuard {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl TailThreadGuard {
    fn start(path: PathBuf) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || tail_log_file(path, stop_for_thread));
        Self {
            stop,
            handle: Some(handle),
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for TailThreadGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

fn tail_log_file(path: PathBuf, stop: Arc<AtomicBool>) {
    let mut offset = 0_u64;
    while !stop.load(Ordering::SeqCst) {
        if let Ok(mut file) = fs::File::open(&path) {
            if file.seek(SeekFrom::Start(offset)).is_ok() {
                let mut chunk = String::new();
                if let Ok(bytes_read) = file.read_to_string(&mut chunk) {
                    if bytes_read > 0 {
                        print!("{chunk}");
                        let _ = io::stdout().flush();
                        offset += bytes_read as u64;
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn wait_for_app_window(
    app: &mut ChildCleanupGuard,
    capture_log_path: &Path,
    wait_seconds: f64,
) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs_f64(wait_seconds.max(0.0));
    loop {
        if app.has_exited()? {
            return Err(format!(
                "vtube-studio-rs exited before a window appeared. Last log lines:\n{}",
                tail_lines(capture_log_path, 40)
            )
            .into());
        }

        if let Some(window_id) = find_vtube_window_id()? {
            return Ok(window_id);
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "Could not find vtube-studio-rs window. Last log lines:\n{}",
                tail_lines(capture_log_path, 40)
            )
            .into());
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn find_vtube_window_id() -> Result<Option<String>> {
    macos_window::find_window_id_by_owner("vtube-studio-rs")
}

fn capture_window_with_retries(
    window_id: &str,
    output_path: &Path,
    capture_log_path: &Path,
    attempts: u32,
) -> Result<()> {
    for attempt in 1..=attempts {
        match macos_window::capture_window_png(window_id, output_path) {
            Ok(()) => {
                println!("Captured window {window_id} through CoreGraphics.");
                return Ok(());
            }
            Err(error) if attempt < attempts => {
                eprintln!(
                    "Window capture attempt {attempt}/{attempts} failed through CoreGraphics: {error}"
                );
            }
            Err(error) => {
                return Err(format!(
                    "Could not capture vtube-studio-rs window {window_id} after {attempts} attempts through CoreGraphics.\nLast capture error:\n{error}\nLast app log lines:\n{}",
                    tail_lines(capture_log_path, 40)
                )
                .into());
            }
        }
        thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
mod macos_window {
    use super::{Path, Result};

    pub fn find_window_id_by_owner(_owner_name: &str) -> Result<Option<String>> {
        Err("window capture is only supported on macOS".into())
    }

    pub fn capture_window_png(_window_id: &str, _output_path: &Path) -> Result<()> {
        Err("window capture is only supported on macOS".into())
    }
}

#[cfg(target_os = "macos")]
mod macos_window {
    use super::{CStr, Path, Result, RgbaImage, c_char, c_uint, c_void};
    use core_foundation_sys::{
        array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef},
        base::{Boolean, CFIndex, CFRelease, CFTypeRef},
        data::{CFDataGetBytePtr, CFDataGetLength, CFDataRef},
        dictionary::{CFDictionaryGetValueIfPresent, CFDictionaryRef},
        number::{CFNumberGetValue, CFNumberRef, kCFNumberSInt64Type},
        string::{CFStringGetCString, CFStringGetCStringPtr, CFStringRef, kCFStringEncodingUTF8},
    };

    type CGImageRef = *const c_void;
    type CGDataProviderRef = *const c_void;
    type CGWindowID = c_uint;
    type CGWindowListOption = c_uint;
    type CGWindowImageOption = c_uint;
    type CGBitmapInfo = c_uint;

    const KCG_NULL_WINDOW_ID: CGWindowID = 0;
    const KCG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: CGWindowListOption = 1 << 0;
    const KCG_WINDOW_LIST_OPTION_INCLUDING_WINDOW: CGWindowListOption = 1 << 3;
    const KCG_WINDOW_IMAGE_BOUNDS_IGNORE_FRAMING: CGWindowImageOption = 1 << 0;

    const KCG_IMAGE_ALPHA_PREMULTIPLIED_LAST: CGBitmapInfo = 1;
    const KCG_IMAGE_ALPHA_PREMULTIPLIED_FIRST: CGBitmapInfo = 2;
    const KCG_IMAGE_ALPHA_LAST: CGBitmapInfo = 3;
    const KCG_IMAGE_ALPHA_FIRST: CGBitmapInfo = 4;
    const KCG_IMAGE_ALPHA_NONE_SKIP_LAST: CGBitmapInfo = 5;
    const KCG_IMAGE_ALPHA_NONE_SKIP_FIRST: CGBitmapInfo = 6;
    const KCG_IMAGE_ALPHA_MASK: CGBitmapInfo = 0x1f;
    const KCG_BITMAP_BYTE_ORDER_32_LITTLE: CGBitmapInfo = 2 << 12;
    const KCG_BITMAP_BYTE_ORDER_32_BIG: CGBitmapInfo = 4 << 12;
    const KCG_BITMAP_BYTE_ORDER_MASK: CGBitmapInfo = 0x7000;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        static CGRectNull: CGRect;
        static kCGWindowNumber: CFStringRef;
        static kCGWindowOwnerName: CFStringRef;

        fn CGWindowListCopyWindowInfo(
            option: CGWindowListOption,
            relative_to_window: CGWindowID,
        ) -> CFArrayRef;
        fn CGWindowListCreateImage(
            screen_bounds: CGRect,
            list_option: CGWindowListOption,
            window_id: CGWindowID,
            image_option: CGWindowImageOption,
        ) -> CGImageRef;
        fn CGImageGetWidth(image: CGImageRef) -> usize;
        fn CGImageGetHeight(image: CGImageRef) -> usize;
        fn CGImageGetBytesPerRow(image: CGImageRef) -> usize;
        fn CGImageGetBitsPerPixel(image: CGImageRef) -> usize;
        fn CGImageGetBitmapInfo(image: CGImageRef) -> CGBitmapInfo;
        fn CGImageGetDataProvider(image: CGImageRef) -> CGDataProviderRef;
        fn CGDataProviderCopyData(provider: CGDataProviderRef) -> CFDataRef;
    }

    pub fn find_window_id_by_owner(owner_name: &str) -> Result<Option<String>> {
        let window_list = unsafe {
            CGWindowListCopyWindowInfo(KCG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY, KCG_NULL_WINDOW_ID)
        };
        if window_list.is_null() {
            return Ok(None);
        }

        let found = unsafe { find_window_id_in_list(window_list, owner_name) };
        unsafe {
            CFRelease(window_list as CFTypeRef);
        }
        found
    }

    pub fn capture_window_png(window_id: &str, output_path: &Path) -> Result<()> {
        let window_id = window_id
            .parse::<CGWindowID>()
            .map_err(|error| format!("invalid window id {window_id}: {error}"))?;
        let image = unsafe {
            CGWindowListCreateImage(
                CGRectNull,
                KCG_WINDOW_LIST_OPTION_INCLUDING_WINDOW,
                window_id,
                KCG_WINDOW_IMAGE_BOUNDS_IGNORE_FRAMING,
            )
        };
        if image.is_null() {
            return Err(
                "CoreGraphics returned no window image; check Screen Recording permission".into(),
            );
        }

        let result = unsafe { write_cg_image_png(image, output_path) };
        unsafe {
            CFRelease(image as CFTypeRef);
        }
        result
    }

    unsafe fn write_cg_image_png(image: CGImageRef, output_path: &Path) -> Result<()> {
        let width = unsafe { CGImageGetWidth(image) };
        let height = unsafe { CGImageGetHeight(image) };
        let bytes_per_row = unsafe { CGImageGetBytesPerRow(image) };
        let bits_per_pixel = unsafe { CGImageGetBitsPerPixel(image) };
        let bitmap_info = unsafe { CGImageGetBitmapInfo(image) };
        if width == 0 || height == 0 {
            return Err("CoreGraphics captured an empty window image".into());
        }
        if bits_per_pixel != 32 {
            return Err(
                format!("unsupported CoreGraphics image depth: {bits_per_pixel} bpp").into(),
            );
        }

        let provider = unsafe { CGImageGetDataProvider(image) };
        if provider.is_null() {
            return Err("CoreGraphics image has no data provider".into());
        }
        let data = unsafe { CGDataProviderCopyData(provider) };
        if data.is_null() {
            return Err("CoreGraphics image data provider returned no bytes".into());
        }

        let result = unsafe {
            let length = CFDataGetLength(data).max(0) as usize;
            let bytes = CFDataGetBytePtr(data);
            if bytes.is_null() {
                Err("CoreGraphics image data is null".into())
            } else {
                let source = std::slice::from_raw_parts(bytes, length);
                let rgba =
                    cg_image_bytes_to_rgba(source, width, height, bytes_per_row, bitmap_info)?;
                rgba.save(output_path)?;
                Ok(())
            }
        };
        unsafe {
            CFRelease(data as CFTypeRef);
        }
        result
    }

    fn cg_image_bytes_to_rgba(
        source: &[u8],
        width: usize,
        height: usize,
        bytes_per_row: usize,
        bitmap_info: CGBitmapInfo,
    ) -> Result<RgbaImage> {
        let required_len = bytes_per_row
            .checked_mul(height)
            .ok_or("CoreGraphics image byte size overflow")?;
        if source.len() < required_len {
            return Err(format!(
                "CoreGraphics image data is too short: {} bytes for {width}x{height} rows of {bytes_per_row}",
                source.len()
            )
            .into());
        }

        let mut output = Vec::with_capacity(width * height * 4);
        for row in 0..height {
            let row_start = row * bytes_per_row;
            for column in 0..width {
                let pixel_start = row_start + column * 4;
                output.extend_from_slice(&pixel_to_rgba(
                    &source[pixel_start..pixel_start + 4],
                    bitmap_info,
                ));
            }
        }

        RgbaImage::from_raw(width as u32, height as u32, output)
            .ok_or_else(|| "failed to build RGBA screenshot image".into())
    }

    fn pixel_to_rgba(pixel: &[u8], bitmap_info: CGBitmapInfo) -> [u8; 4] {
        let alpha = bitmap_info & KCG_IMAGE_ALPHA_MASK;
        let byte_order = bitmap_info & KCG_BITMAP_BYTE_ORDER_MASK;
        let premultiplied = alpha == KCG_IMAGE_ALPHA_PREMULTIPLIED_FIRST
            || alpha == KCG_IMAGE_ALPHA_PREMULTIPLIED_LAST;

        let (r, g, b, a) = match (byte_order, alpha) {
            (
                KCG_BITMAP_BYTE_ORDER_32_LITTLE,
                KCG_IMAGE_ALPHA_FIRST
                | KCG_IMAGE_ALPHA_PREMULTIPLIED_FIRST
                | KCG_IMAGE_ALPHA_NONE_SKIP_FIRST,
            ) => (pixel[2], pixel[1], pixel[0], alpha_value(pixel[3], alpha)),
            (
                KCG_BITMAP_BYTE_ORDER_32_LITTLE,
                KCG_IMAGE_ALPHA_LAST
                | KCG_IMAGE_ALPHA_PREMULTIPLIED_LAST
                | KCG_IMAGE_ALPHA_NONE_SKIP_LAST,
            ) => (pixel[3], pixel[2], pixel[1], alpha_value(pixel[0], alpha)),
            (
                KCG_BITMAP_BYTE_ORDER_32_BIG,
                KCG_IMAGE_ALPHA_FIRST
                | KCG_IMAGE_ALPHA_PREMULTIPLIED_FIRST
                | KCG_IMAGE_ALPHA_NONE_SKIP_FIRST,
            ) => (pixel[1], pixel[2], pixel[3], alpha_value(pixel[0], alpha)),
            (
                KCG_BITMAP_BYTE_ORDER_32_BIG,
                KCG_IMAGE_ALPHA_LAST
                | KCG_IMAGE_ALPHA_PREMULTIPLIED_LAST
                | KCG_IMAGE_ALPHA_NONE_SKIP_LAST,
            ) => (pixel[0], pixel[1], pixel[2], alpha_value(pixel[3], alpha)),
            _ => (pixel[2], pixel[1], pixel[0], pixel[3]),
        };

        if premultiplied {
            unpremultiply_rgba(r, g, b, a)
        } else {
            [r, g, b, a]
        }
    }

    fn alpha_value(value: u8, alpha: CGBitmapInfo) -> u8 {
        if alpha == KCG_IMAGE_ALPHA_NONE_SKIP_FIRST || alpha == KCG_IMAGE_ALPHA_NONE_SKIP_LAST {
            255
        } else {
            value
        }
    }

    fn unpremultiply_rgba(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
        if a == 0 || a == 255 {
            return [r, g, b, a];
        }
        let alpha = a as u32;
        [
            ((r as u32 * 255 + alpha / 2) / alpha).min(255) as u8,
            ((g as u32 * 255 + alpha / 2) / alpha).min(255) as u8,
            ((b as u32 * 255 + alpha / 2) / alpha).min(255) as u8,
            a,
        ]
    }

    unsafe fn find_window_id_in_list(
        window_list: CFArrayRef,
        owner_name: &str,
    ) -> Result<Option<String>> {
        let count = unsafe { CFArrayGetCount(window_list) };
        for index in 0..count {
            let window = unsafe { CFArrayGetValueAtIndex(window_list, index) as CFDictionaryRef };
            if window.is_null() {
                continue;
            }

            let Some(owner_ref) =
                (unsafe { dictionary_value(window, kCGWindowOwnerName as *const c_void) })
            else {
                continue;
            };
            let Some(owner) = (unsafe { cf_string_to_string(owner_ref as CFStringRef) }) else {
                continue;
            };
            if owner != owner_name {
                continue;
            }

            let Some(number_ref) =
                (unsafe { dictionary_value(window, kCGWindowNumber as *const c_void) })
            else {
                continue;
            };
            let Some(window_id) = (unsafe { cf_number_to_i64(number_ref as CFNumberRef) }) else {
                continue;
            };
            return Ok(Some(window_id.to_string()));
        }

        Ok(None)
    }

    unsafe fn dictionary_value(
        dictionary: CFDictionaryRef,
        key: *const c_void,
    ) -> Option<*const c_void> {
        let mut value = std::ptr::null();
        let present: Boolean =
            unsafe { CFDictionaryGetValueIfPresent(dictionary, key, &mut value) };
        if present == 0 || value.is_null() {
            None
        } else {
            Some(value)
        }
    }

    unsafe fn cf_string_to_string(value: CFStringRef) -> Option<String> {
        if value.is_null() {
            return None;
        }

        let direct = unsafe { CFStringGetCStringPtr(value, kCFStringEncodingUTF8) };
        if !direct.is_null() {
            return Some(
                unsafe { CStr::from_ptr(direct) }
                    .to_string_lossy()
                    .into_owned(),
            );
        }

        let mut buffer = [0 as c_char; 1024];
        let copied = unsafe {
            CFStringGetCString(
                value,
                buffer.as_mut_ptr(),
                buffer.len() as CFIndex,
                kCFStringEncodingUTF8,
            )
        };
        if copied == 0 {
            None
        } else {
            Some(
                unsafe { CStr::from_ptr(buffer.as_ptr()) }
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }

    unsafe fn cf_number_to_i64(value: CFNumberRef) -> Option<i64> {
        if value.is_null() {
            return None;
        }

        let mut output = 0_i64;
        let copied = unsafe {
            CFNumberGetValue(value, kCFNumberSInt64Type, (&mut output as *mut i64).cast())
        };
        if copied { Some(output) } else { None }
    }
}

fn env_f64(name: &str, default: f64) -> Result<f64> {
    match env::var(name) {
        Ok(value) => value
            .parse::<f64>()
            .map_err(|error| format!("invalid {name}={value}: {error}").into()),
        Err(_) => Ok(default),
    }
}

fn env_u32(name: &str, default: u32) -> Result<u32> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u32>()
            .map_err(|error| format!("invalid {name}={value}: {error}").into()),
        Err(_) => Ok(default),
    }
}

fn timestamp_for_filename() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("unix{}", duration.as_secs()),
        Err(_) => "unix0".to_string(),
    }
}

fn tail_lines(path: &Path, limit: usize) -> String {
    let Ok(content) = fs::read_to_string(path) else {
        return String::new();
    };
    let lines: Vec<&str> = content.lines().collect();
    lines
        .iter()
        .skip(lines.len().saturating_sub(limit))
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug)]
struct SpaceTestReport {
    instance_guard_acquired: usize,
    app_nap_guard_started: usize,
    window_configured: usize,
    app_active_changed: usize,
    window_visible_changed: usize,
    window_occlusion_changed: usize,
    long_frame_gap: usize,
    display_wake_inferred: usize,
    next_drawable_unavailable: usize,
    next_drawable_recovered: usize,
    drawable_size_changed: usize,
    sckit_probe_started: usize,
    sckit_frame_summary: usize,
    sckit_stalled: usize,
    sckit_recovered: usize,
    sckit_probe_failed: usize,
    recent_events: String,
}

fn write_space_test_report(
    model_path: &str,
    log_path: &Path,
    report_path: &Path,
) -> Result<SpaceTestReport> {
    let log = fs::read_to_string(log_path).unwrap_or_default();
    let report = SpaceTestReport {
        instance_guard_acquired: renderer_event_count(&log, "instance_guard_acquired"),
        app_nap_guard_started: renderer_event_count(&log, "app_nap_guard_started"),
        window_configured: renderer_event_count(&log, "window_configured"),
        app_active_changed: renderer_event_count(&log, "app_active_changed"),
        window_visible_changed: renderer_event_count(&log, "window_visible_changed"),
        window_occlusion_changed: renderer_event_count(&log, "window_occlusion_changed"),
        long_frame_gap: renderer_event_count(&log, "long_frame_gap"),
        display_wake_inferred: renderer_event_count(&log, "display_wake_inferred"),
        next_drawable_unavailable: renderer_event_count(&log, "next_drawable_unavailable"),
        next_drawable_recovered: renderer_event_count(&log, "next_drawable_recovered"),
        drawable_size_changed: renderer_event_count(&log, "drawable_size_changed"),
        sckit_probe_started: renderer_event_count(&log, "sckit_probe_started"),
        sckit_frame_summary: renderer_event_count(&log, "sckit_frame_summary"),
        sckit_stalled: renderer_event_count(&log, "sckit_stalled"),
        sckit_recovered: renderer_event_count(&log, "sckit_recovered"),
        sckit_probe_failed: renderer_event_count(&log, "sckit_probe_failed"),
        recent_events: recent_renderer_events(&log, 20),
    };

    let (startup_status, startup_detail) = if report.instance_guard_acquired >= 1
        && report.app_nap_guard_started >= 1
        && report.window_configured >= 1
    {
        (
            "PASS",
            "startup guard, App Nap guard, and window configuration were logged",
        )
    } else {
        (
            "RISK",
            "expected startup guard/App Nap/window configuration events were missing",
        )
    };
    let (drawable_status, drawable_detail) =
        if report.next_drawable_unavailable > report.next_drawable_recovered {
            (
                "RISK",
                "next_drawable_unavailable is greater than next_drawable_recovered",
            )
        } else {
            (
                "PASS",
                "drawable availability did not report an unrecovered loss",
            )
        };
    let (wake_status, wake_detail) = if report.display_wake_inferred > 0 {
        (
            "CHECK",
            "display_wake_inferred appeared; manually confirm the avatar recovered",
        )
    } else {
        ("PASS", "no inferred display wake was logged")
    };
    let (gap_status, gap_detail) = if report.long_frame_gap > 0 {
        (
            "INFO",
            "long_frame_gap is treated as a transition signal, not an automatic failure",
        )
    } else {
        ("PASS", "no long frame gaps were logged")
    };
    let (sckit_status, sckit_detail) = if report.sckit_probe_failed > 0 {
        (
            "CHECK",
            "ScreenCaptureKit probe failed; check Screen Recording permission and logs",
        )
    } else if report.sckit_probe_started > 0 && report.sckit_frame_summary > 0 {
        (
            "PASS",
            "ScreenCaptureKit probe started and reported frame summaries",
        )
    } else if report.sckit_probe_started > 0 {
        (
            "CHECK",
            "ScreenCaptureKit probe started but no frame summary was captured before shutdown",
        )
    } else {
        (
            "INFO",
            "ScreenCaptureKit probe did not start; it may be disabled in the active profile",
        )
    };

    let markdown = format!(
        "\
# vtube-studio-rs Space Reliability Report

- Generated: {}
- Model: `{}`
- Log: `{}`

## Manual Checklist

- [ ] Frames kept increasing during Space switches.
- [ ] FPS recovered to roughly 60 after transitions.
- [ ] Avatar window remained visible after Space switches.
- [ ] Avatar remained visible beside a full-screen app.
- [ ] ScreenCaptureKit probe kept reporting frames or recovered after stalls.
- [ ] Avatar recovered after display sleep/wake.
- [ ] No duplicate avatar windows appeared after reruns.
- [ ] Notes:

## Event Counts

| Event | Count |
| --- | ---: |
| instance_guard_acquired | {} |
| app_nap_guard_started | {} |
| window_configured | {} |
| app_active_changed | {} |
| window_visible_changed | {} |
| window_occlusion_changed | {} |
| long_frame_gap | {} |
| display_wake_inferred | {} |
| next_drawable_unavailable | {} |
| next_drawable_recovered | {} |
| drawable_size_changed | {} |
| sckit_probe_started | {} |
| sckit_frame_summary | {} |
| sckit_stalled | {} |
| sckit_recovered | {} |
| sckit_probe_failed | {} |

## Automatic Assessment

| Check | Status | Detail |
| --- | --- | --- |
| Startup guards | {} | {} |
| Drawable recovery | {} | {} |
| Display wake | {} | {} |
| Long frame gaps | {} | {} |
| ScreenCaptureKit probe | {} | {} |

## Recent Renderer Events

```text
{}
```
",
        generated_stamp(),
        model_path,
        log_path.display(),
        report.instance_guard_acquired,
        report.app_nap_guard_started,
        report.window_configured,
        report.app_active_changed,
        report.window_visible_changed,
        report.window_occlusion_changed,
        report.long_frame_gap,
        report.display_wake_inferred,
        report.next_drawable_unavailable,
        report.next_drawable_recovered,
        report.drawable_size_changed,
        report.sckit_probe_started,
        report.sckit_frame_summary,
        report.sckit_stalled,
        report.sckit_recovered,
        report.sckit_probe_failed,
        startup_status,
        startup_detail,
        drawable_status,
        drawable_detail,
        wake_status,
        wake_detail,
        gap_status,
        gap_detail,
        sckit_status,
        sckit_detail,
        report.recent_events
    );
    fs::write(report_path, markdown)?;
    Ok(report)
}

fn renderer_event_count(log: &str, event: &str) -> usize {
    let needle = format!("renderer_event={event}");
    log.lines()
        .filter(|line| {
            line.split_once(&needle)
                .map(|(_, rest)| rest.is_empty() || rest.starts_with(char::is_whitespace))
                .unwrap_or(false)
        })
        .count()
}

fn recent_renderer_events(log: &str, limit: usize) -> String {
    let events: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("renderer_event="))
        .collect();
    if events.is_empty() {
        "No renderer_event lines were recorded.".to_string()
    } else {
        events
            .iter()
            .skip(events.len().saturating_sub(limit))
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn print_space_test_summary(report: &SpaceTestReport, log_path: &Path, report_path: &Path) {
    println!();
    println!("Space/display event summary:");
    println!(
        "  {:<34} {}",
        "instance_guard_acquired", report.instance_guard_acquired
    );
    println!(
        "  {:<34} {}",
        "app_nap_guard_started", report.app_nap_guard_started
    );
    println!("  {:<34} {}", "window_configured", report.window_configured);
    println!(
        "  {:<34} {}",
        "app_active_changed", report.app_active_changed
    );
    println!(
        "  {:<34} {}",
        "window_visible_changed", report.window_visible_changed
    );
    println!(
        "  {:<34} {}",
        "window_occlusion_changed", report.window_occlusion_changed
    );
    println!("  {:<34} {}", "long_frame_gap", report.long_frame_gap);
    println!(
        "  {:<34} {}",
        "display_wake_inferred", report.display_wake_inferred
    );
    println!(
        "  {:<34} {}",
        "next_drawable_unavailable", report.next_drawable_unavailable
    );
    println!(
        "  {:<34} {}",
        "next_drawable_recovered", report.next_drawable_recovered
    );
    println!(
        "  {:<34} {}",
        "drawable_size_changed", report.drawable_size_changed
    );
    println!(
        "  {:<34} {}",
        "sckit_probe_started", report.sckit_probe_started
    );
    println!(
        "  {:<34} {}",
        "sckit_frame_summary", report.sckit_frame_summary
    );
    println!("  {:<34} {}", "sckit_stalled", report.sckit_stalled);
    println!("  {:<34} {}", "sckit_recovered", report.sckit_recovered);
    println!(
        "  {:<34} {}",
        "sckit_probe_failed", report.sckit_probe_failed
    );
    println!();
    println!("Recent renderer events:");
    println!("{}", report.recent_events);
    println!();
    println!("Full log: {}", log_path.display());
    println!("Markdown report: {}", report_path.display());
}

fn model_exists(root: &Path, model_path: &str) -> bool {
    let path = Path::new(model_path);
    path.is_file() || root.join(path).is_file()
}

fn existing_models(root: &Path, models: Vec<String>) -> Vec<String> {
    models
        .into_iter()
        .filter(|model| model_exists(root, model))
        .collect()
}

fn run_full_step<F>(name: &str, step: F) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    println!("\n==> {name}");
    let root = project_root()?;
    terminate_app_processes(&root);
    let result = step();
    terminate_app_processes(&root);
    result
}

struct ConfigRestoreGuard {
    path: PathBuf,
    original: Option<String>,
}

impl ConfigRestoreGuard {
    fn prepare(config_path: &Path, example_config_path: &Path) -> Result<Self> {
        let original = if config_path.is_file() {
            Some(fs::read_to_string(config_path)?)
        } else {
            require_file(example_config_path, "Missing config template")?;
            fs::copy(example_config_path, config_path)?;
            None
        };
        Ok(Self {
            path: config_path.to_path_buf(),
            original,
        })
    }
}

impl Drop for ConfigRestoreGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(content) => {
                let _ = fs::write(&self.path, content);
            }
            None => {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}

fn set_toml_values(path: &Path, updates: &[(&str, String)]) -> Result<()> {
    let mut content = fs::read_to_string(path)?;
    for (key, value) in updates {
        content = set_toml_value(&content, key, value);
    }
    fs::write(path, content)?;
    Ok(())
}

fn remove_toml_section(content: &str, section: &str) -> String {
    let section_header = format!("[{section}]");
    let subsection_prefix = format!("[{section}.");
    let mut output = String::new();
    let mut removing = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == section_header || trimmed.starts_with(&subsection_prefix) {
            removing = true;
            continue;
        }
        if removing && trimmed.starts_with('[') && trimmed.ends_with(']') {
            removing = false;
        }
        if !removing {
            output.push_str(line);
            output.push('\n');
        }
    }

    output
}

fn set_toml_value(content: &str, key: &str, value: &str) -> String {
    let mut found = false;
    let mut output = String::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(key) && trimmed[key.len()..].trim_start().starts_with('=') {
            let indent_len = line.len() - trimmed.len();
            output.push_str(&line[..indent_len]);
            output.push_str(key);
            output.push_str(" = ");
            output.push_str(value);
            output.push('\n');
            found = true;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    if !found {
        output.push_str(key);
        output.push_str(" = ");
        output.push_str(value);
        output.push('\n');
    }
    output
}

fn set_toml_section_value(content: &str, section: &str, key: &str, value: &str) -> String {
    let section_header = format!("[{section}]");
    let mut output = String::new();
    let mut found_section = false;
    let mut in_target_section = false;
    let mut found_key = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if in_target_section
            && trimmed.starts_with('[')
            && trimmed.ends_with(']')
            && trimmed != section_header
        {
            if !found_key {
                output.push_str(key);
                output.push_str(" = ");
                output.push_str(value);
                output.push('\n');
                found_key = true;
            }
            in_target_section = false;
        }

        if trimmed == section_header {
            found_section = true;
            in_target_section = true;
        }

        if in_target_section {
            let trimmed_start = line.trim_start();
            if trimmed_start.starts_with(key)
                && trimmed_start[key.len()..].trim_start().starts_with('=')
            {
                let indent_len = line.len() - trimmed_start.len();
                output.push_str(&line[..indent_len]);
                output.push_str(key);
                output.push_str(" = ");
                output.push_str(value);
                output.push('\n');
                found_key = true;
                continue;
            }
        }

        output.push_str(line);
        output.push('\n');
    }

    if found_section && in_target_section && !found_key {
        output.push_str(key);
        output.push_str(" = ");
        output.push_str(value);
        output.push('\n');
    } else if !found_section {
        if !output.is_empty() && !output.ends_with("\n\n") {
            output.push('\n');
        }
        output.push_str(&section_header);
        output.push('\n');
        output.push_str(key);
        output.push_str(" = ");
        output.push_str(value);
        output.push('\n');
    }

    output
}

fn set_toml_section_values(content: &str, section: &str, updates: &[(&str, String)]) -> String {
    let mut output = content.to_string();
    for (key, value) in updates {
        output = set_toml_section_value(&output, section, key, value);
    }
    output
}

fn toml_string_literal(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn model_name_from_path(model_path: &str) -> String {
    let file_name = Path::new(model_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(model_path);
    file_name
        .strip_suffix(".model3.json")
        .unwrap_or(file_name)
        .to_string()
}

#[derive(Debug)]
struct ModelManifestSummary {
    path: PathBuf,
    name: String,
    texture_count: usize,
    motion_count: usize,
    expression_count: usize,
    has_physics: bool,
    has_display_info: bool,
}

impl ModelManifestSummary {
    fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        let manifest: ModelManifestLite = serde_json::from_str(&text)?;
        let references = manifest.file_references;
        let motion_count = references
            .motions
            .values()
            .map(std::vec::Vec::len)
            .sum::<usize>();

        Ok(Self {
            path: path.to_path_buf(),
            name: model_name_from_path(&path.to_string_lossy()),
            texture_count: references.textures.len(),
            motion_count,
            expression_count: references.expressions.len(),
            has_physics: references.physics.is_some(),
            has_display_info: references.display_info.is_some(),
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ModelManifestLite {
    file_references: FileReferencesLite,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FileReferencesLite {
    #[serde(default)]
    textures: Vec<String>,
    physics: Option<String>,
    display_info: Option<String>,
    #[serde(default)]
    motions: HashMap<String, Vec<serde_json::Value>>,
    #[serde(default)]
    expressions: Vec<serde_json::Value>,
}

fn collect_model3_paths(root: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let metadata = fs::metadata(root)
        .map_err(|error| format!("failed to inspect {}: {error}", root.display()))?;
    if metadata.is_file() {
        if is_model3_path(root) {
            paths.push(root.to_path_buf());
        }
        return Ok(());
    }

    for entry in
        fs::read_dir(root).map_err(|error| format!("failed to read {}: {error}", root.display()))?
    {
        let entry = entry
            .map_err(|error| format!("failed to read entry in {}: {error}", root.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if file_type.is_dir() {
            collect_model3_paths(&path, paths)?;
        } else if file_type.is_file() && is_model3_path(&path) {
            paths.push(path);
        }
    }

    Ok(())
}

fn is_model3_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".model3.json"))
}

fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn absolute_path(root: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn wait_for_pid_file(path: &Path, timeout: Duration) -> Result<u32> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if let Ok(text) = fs::read_to_string(path) {
            if let Ok(pid) = text.trim().parse::<u32>() {
                return Ok(pid);
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!("timed out waiting for {}", path.display()).into())
}

fn pid_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .is_ok_and(|status| status.success())
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn run_render_regression_report_safe(root: &Path) -> Result<()> {
    if env::var("VTUBE_RS_SKIP_REPORT").unwrap_or_default() == "1" {
        println!("Render regression report: skipped (VTUBE_RS_SKIP_REPORT=1)");
        return Ok(());
    }

    let output = Command::new(env::current_exe()?)
        .arg("render-regression-report")
        .current_dir(root)
        .env_remove("OUTPUT_DIR")
        .output()?;
    if output.status.success() {
        println!(
            "Render regression report: {}",
            String::from_utf8_lossy(&output.stdout).trim()
        );
        Ok(())
    } else {
        Err(format!(
            "render-regression-report failed with status {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

fn run_model_probe(root: &Path, roots: &[String], probe_path: &Path) -> Result<()> {
    if let Some(parent) = probe_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let (include_dir, lib_dir) = cubism_core_paths(root)?;

    let output = Command::new("cargo")
        .arg("run")
        .arg("--features")
        .arg("metal-renderer")
        .arg("--")
        .arg("--probe-models")
        .args(roots)
        .current_dir(&root)
        .env("CUBISM_CORE_INCLUDE_DIR", &include_dir)
        .env("CUBISM_CORE_LIB_DIR", &lib_dir)
        .output()?;

    let mut report = String::new();
    report.push_str("# vtube-studio-rs Model Risk Probe\n\n");
    report.push_str(&format!("Generated: {}\n", generated_stamp()));
    report.push_str(&format!("Roots: {}\n\n", roots.join(" ")));
    report.push_str(std::str::from_utf8(&output.stdout)?);
    if !output.status.success() && !output.stderr.is_empty() {
        report.push_str("\n## Command stderr\n\n```text\n");
        report.push_str(std::str::from_utf8(&output.stderr)?);
        report.push_str("\n```\n");
    }
    fs::write(&probe_path, report)?;

    if !output.status.success() {
        return Err(format!("model probe failed with status {}", output.status).into());
    }

    Ok(())
}

fn cubism_core_paths(root: &Path) -> Result<(PathBuf, PathBuf)> {
    let sdk_root = env::var_os("LIVE2D_CUBISM_SDK_NATIVE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("public/CubismSdkForNative"));
    let include_dir = env::var_os("CUBISM_CORE_INCLUDE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| sdk_root.join("Core/include"));
    let lib_dir = match env::var_os("CUBISM_CORE_LIB_DIR") {
        Some(path) => PathBuf::from(path),
        None => sdk_root.join("Core/lib/macos").join(host_arch_lib_dir()?),
    };

    require_file(
        &include_dir.join("Live2DCubismCore.h"),
        "Missing Live2DCubismCore.h. Set LIVE2D_CUBISM_SDK_NATIVE_DIR, CUBISM_CORE_INCLUDE_DIR, or install the SDK under public/CubismSdkForNative",
    )?;
    require_file(
        &lib_dir.join("libLive2DCubismCore.a"),
        "Missing libLive2DCubismCore.a. Set CUBISM_CORE_LIB_DIR or install the SDK under public/CubismSdkForNative",
    )?;

    Ok((include_dir, lib_dir))
}

fn compatibility_report(root: &Path, samples_root: &str, probe_path: &Path, probe: &str) -> String {
    let mut report = String::new();
    report.push_str("# vtube-studio-rs Sample Compatibility Sweep\n\n");
    report.push_str(&format!("- Generated: {}\n", generated_stamp()));
    report.push_str(&format!("- Samples root: `{samples_root}`\n"));
    report.push_str(&format!(
        "- Probe: `{}`\n\n",
        relative_path(root, probe_path)
    ));
    report.push_str(&compatibility_recommendations(probe));
    report.push_str("## Model Risk Table\n\n");
    report.push_str("| Model | Risk | Masks | Max Mask | Ext Blend | Offscreens | Reasons |\n");
    report.push_str("| --- | --- | ---: | ---: | ---: | ---: | --- |\n");
    for model in parse_probe_models(probe) {
        report.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} | {} |\n",
            model.path,
            model.status,
            model.masks,
            model.max_mask,
            model.extended_blends,
            model.offscreens,
            model.reasons
        ));
    }
    report.push_str("\n## Raw Probe\n\n```text\n");
    report.push_str(&first_lines(probe, 260));
    report.push_str("```\n");
    report
}

fn compatibility_recommendations(probe: &str) -> String {
    let high_count = probe.matches("risk:high").count();
    let offscreen_count = probe.matches("\n  risk offscreen objects:").count();
    let extended_count = probe.matches("\n  risk extended blend objects:").count();
    let dense_count = probe.matches("\n  risk dense clipping:").count();

    format!(
        "\
## Recommendations

- High-risk models found: {high_count}.
- Models with offscreen objects: {offscreen_count}.
- Models with extended blend objects: {extended_count}.
- Models with dense clipping: {dense_count}.
- Keep `Mao` in the mask matrix while it remains the dense clipping stress model.
- Keep `Ren` in the offscreen matrix while it remains the offscreen/extended blend stress model.
- Add another model to a screenshot matrix only when this sweep shows a new risk shape not covered by `Mao` or `Ren`.

"
    )
}

fn mao_mask_audit_report(
    root: &Path,
    output_dir: &Path,
    model_path: &str,
    probe_path: &Path,
    probe: &str,
) -> String {
    let mut report = String::new();
    report.push_str("# Mao Mask Matrix Audit\n\n");
    report.push_str(&format!("- Generated: {}\n", generated_stamp()));
    report.push_str(&format!("- Model: `{model_path}`\n"));
    report.push_str(&format!(
        "- Probe: `{}`\n\n",
        relative_path(root, probe_path)
    ));

    report.push_str(
        "\
## Audit Focus

- [ ] Shared and high-precision captures match except for expected edge precision differences.
- [ ] No-mask capture only differs where clipping is expected.
- [ ] Eye masks preserve pupil/eyeball layers and do not create white or empty eyes.
- [ ] Inverted masks do not produce unexpected holes around face, wand, hearts, or eyes.
- [ ] Dense masks, especially drawables with 6+ masks, do not leak outside their intended regions.

## Manual Decision

| Area | Decision | Notes |
| --- | --- | --- |
| Shared vs high-precision | [ ] Pass [ ] Investigate [ ] Rerun |  |
| No-mask expected differences | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Eye clipping | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Inverted masks | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Dense multi-mask drawables | [ ] Pass [ ] Investigate renderer |  |

",
    );

    report.push_str("## Risk Summary\n\n");
    report.push_str(&risk_lines_as_markdown(probe));
    report.push_str("\n## Masked Drawable Samples\n\n");
    report.push_str(&masked_drawable_table(probe));
    report.push_str("\n## Inverted Mask Drawables\n\n");
    report.push_str(&inverted_mask_table(probe));
    report.push_str("\n## Eye Mask Drawables\n\n");
    report.push_str(&eye_mask_table(probe));
    report.push_str("\n## Capture References\n\n");
    report.push_str(&capture_references(root, output_dir, "mask-matrix", "Mao"));
    report.push_str("\n## Raw Probe\n\n```text\n");
    report.push_str(&first_lines(probe, 260));
    report.push_str("```\n");
    report
}

fn ren_offscreen_audit_report(
    root: &Path,
    output_dir: &Path,
    model_path: &str,
    probe_path: &Path,
    probe: &str,
) -> String {
    let mut report = String::new();
    report.push_str("# Ren Offscreen / Extended Blend Audit\n\n");
    report.push_str(&format!("- Generated: {}\n", generated_stamp()));
    report.push_str(&format!("- Model: `{model_path}`\n"));
    report.push_str(&format!(
        "- Probe: `{}`\n\n",
        relative_path(root, probe_path)
    ));

    report.push_str(
        "\
## Audit Focus

- [ ] Offscreen render order matches the owner part order expected by Cubism Framework.
- [ ] Nested offscreens composite after descendants and before parent target flush.
- [ ] Masked offscreens apply the same mask matrix path as masked drawables.
- [ ] Extended offscreens and extended drawables sample the correct pre-composite snapshot.
- [ ] High-precision mask fallback is expected while offscreen rendering is active.

## Manual Decision

| Area | Decision | Notes |
| --- | --- | --- |
| Shared vs high-precision fallback | [ ] Pass [ ] Investigate [ ] Rerun |  |
| No-mask expected differences | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Masked offscreen clipping | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Nested offscreen order | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Extended blend snapshots | [ ] Pass [ ] Investigate renderer |  |

",
    );

    report.push_str("## Risk Summary\n\n");
    report.push_str(&risk_lines_as_markdown(probe));
    report.push_str("\n## Offscreen Plan Timeline\n\n");
    report.push_str("This is the render-order walkthrough from the model probe. Use it to verify where offscreens begin, where extended blends snapshot the current target, and where nested targets flush upward.\n\n");
    report.push_str("### Automatic Plan Checks\n\n");
    report.push_str(&offscreen_plan_checks(probe));
    report.push_str("\n### Timeline\n\n");
    report.push_str(&offscreen_plan_timeline(probe));
    report.push_str("\n## Masked Offscreen Objects\n\n");
    report.push_str(&offscreen_table(probe, OffscreenTableFilter::Masked));
    report.push_str("\n## Nested Offscreen Objects\n\n");
    report.push_str(&offscreen_table(probe, OffscreenTableFilter::Nested));
    report.push_str("\n## Extended Offscreen Objects\n\n");
    report.push_str(&offscreen_table(probe, OffscreenTableFilter::Extended));
    report.push_str("\n## Offscreen Objects\n\n");
    report.push_str(&offscreen_table(probe, OffscreenTableFilter::All));
    report.push_str("\n## Extended Drawable Objects\n\n");
    report.push_str(&extended_drawable_table(probe));
    report.push_str("\n## Capture References\n\n");
    report.push_str(&capture_references(
        root,
        output_dir,
        "offscreen-matrix",
        "Ren",
    ));
    report.push_str("\n## Raw Probe\n\n```text\n");
    report.push_str(&first_lines(probe, 260));
    report.push_str("```\n");
    report
}

fn rice_stress_audit_report(
    root: &Path,
    output_dir: &Path,
    model_path: &str,
    probe_path: &Path,
    probe: &str,
) -> String {
    let mut report = String::new();
    report.push_str("# Rice Optional Stress Audit\n\n");
    report.push_str(&format!("- Generated: {}\n", generated_stamp()));
    report.push_str(&format!("- Model: `{model_path}`\n"));
    report.push_str(&format!(
        "- Probe: `{}`\n\n",
        relative_path(root, probe_path)
    ));

    report.push_str(
        "\
## Audit Focus

- [ ] Additive drawables are not unexpectedly dark, overbright, or missing.
- [ ] Inverted masks clip the intended regions and do not reverse visible holes.
- [ ] Translucent drawables preserve layering and opacity relative to no-mask capture.
- [ ] Shared and high-precision captures match except for expected mask precision differences.
- [ ] No-mask capture only differs where clipping is expected.

## Manual Decision

| Area | Decision | Notes |
| --- | --- | --- |
| Additive blend | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Inverted masks | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Translucent layering | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Overall Rice stress | [ ] Pass [ ] Investigate renderer |  |

",
    );

    report.push_str("## Risk Summary\n\n");
    report.push_str(&risk_lines_as_markdown(probe));
    report.push_str("\n## Additive Drawables\n\n");
    report.push_str(&additive_drawable_table(probe));
    report.push_str("\n## Inverted Mask Summary\n\n");
    report.push_str(&inverted_mask_summary(probe));
    report.push_str("\n## Translucent Drawable Samples\n\n");
    report.push_str(&translucent_drawable_table(probe));
    report.push_str("\n## Capture References\n\n");
    report.push_str(&capture_references(root, output_dir, "rice-stress", "Rice"));
    report.push_str("\n## Raw Probe\n\n```text\n");
    report.push_str(&first_lines(probe, 260));
    report.push_str("```\n");
    report
}

fn additive_drawable_table(probe: &str) -> String {
    let mut output = String::new();
    output.push_str("| Drawable | Part | Render | Opacity | Masks |\n");
    output.push_str("| ---: | --- | ---: | ---: | ---: |\n");
    for drawable in probe.lines().filter_map(parse_drawable_line) {
        if drawable.kind != DrawableLineKind::Drawable || drawable.blend != "Additive" {
            continue;
        }
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {} |\n",
            drawable.index,
            escape_markdown_table_cell(&drawable.part),
            drawable.render_order,
            drawable.opacity,
            drawable.masks
        ));
    }
    output
}

fn inverted_mask_summary(probe: &str) -> String {
    let mut output = String::new();
    if let Some(line) = probe
        .lines()
        .find_map(|line| line.strip_prefix("  risk inverted masks:"))
    {
        output.push_str("- inverted masks:");
        output.push_str(line);
        output.push('\n');
    } else {
        output.push_str("- inverted masks: none reported by probe risk summary\n");
    }
    output.push_str(
        "- Probe detail output may sample only representative drawables, so use the risk summary as the source of truth for the count.\n",
    );
    output.push_str(
        "- Review shared and high-precision captures against no-mask for unexpectedly reversed holes or clipped highlights.\n",
    );
    output
}

fn translucent_drawable_table(probe: &str) -> String {
    let mut output = String::new();
    output.push_str("| Drawable | Part | Render | Blend | Opacity | Sample Alpha | Masks |\n");
    output.push_str("| ---: | --- | ---: | --- | ---: | ---: | ---: |\n");
    for drawable in probe.lines().filter_map(parse_drawable_line) {
        if !matches!(
            drawable.kind,
            DrawableLineKind::Drawable | DrawableLineKind::Sampled
        ) {
            continue;
        }

        let opacity_is_translucent = drawable
            .opacity
            .parse::<f32>()
            .map(|opacity| opacity > 0.0 && opacity < 1.0)
            .unwrap_or(false);
        let alpha_is_translucent = drawable
            .sample_alpha
            .as_deref()
            .and_then(|alpha| alpha.parse::<f32>().ok())
            .map(|alpha| alpha < 0.99)
            .unwrap_or(false);
        if !opacity_is_translucent && !alpha_is_translucent {
            continue;
        }

        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} | {} |\n",
            drawable.index,
            escape_markdown_table_cell(&drawable.part),
            drawable.render_order,
            escape_markdown_table_cell(&drawable.blend),
            drawable.opacity,
            drawable.sample_alpha.as_deref().unwrap_or("-"),
            drawable.masks
        ));
    }
    output
}

fn quality_visual_diff_report(
    root: &Path,
    output_dir: &Path,
    matrix_dir: &Path,
    diff_dir: &Path,
) -> Result<String> {
    let mut report = String::new();
    report.push_str("# Quality Visual Diff\n\n");
    report.push_str(&format!("- Generated: {}\n", generated_stamp()));
    report.push_str(&format!(
        "- Matrix: `{}`\n\n",
        relative_path(root, matrix_dir)
    ));
    report.push_str(
        "\
## Manual Decision

| Model | Decision | Notes |
| --- | --- | --- |
| Default model | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Mao | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Ren | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Overall mipmap quality | [ ] Pass [ ] Keep mipmaps off [ ] Investigate renderer |  |
| Overall anisotropy quality | [ ] Pass [ ] Keep anisotropy at 1 [ ] Investigate renderer |  |

",
    );

    for model in ["0", "Mao", "Ren"] {
        report.push_str(&quality_model_diff(
            root, output_dir, matrix_dir, diff_dir, model,
        )?);
    }

    Ok(report)
}

fn quality_model_diff(
    root: &Path,
    output_dir: &Path,
    matrix_dir: &Path,
    diff_dir: &Path,
    model: &str,
) -> Result<String> {
    let off_image = matrix_dir.join(format!("latest-{model}-mipmaps-off.png"));
    let on_image = matrix_dir.join(format!("latest-{model}-mipmaps-on.png"));
    let aniso_image = matrix_dir.join(format!("latest-{model}-mipmaps-on-aniso8.png"));
    let slug = format!("{model}-mipmaps");
    let mut output = String::new();
    output.push_str(&format!("## {}\n\n", model_label(model)));

    if !off_image.is_file() || !on_image.is_file() {
        output.push_str(
            "_Missing mipmap screenshots. Run `cargo xtask capture-quality-matrix` first._\n\n",
        );
        return Ok(output);
    }

    let off = image::open(&off_image)?.to_rgba8();
    let on = image::open(&on_image)?.to_rgba8();
    let (width, height) = off.dimensions();
    if on.dimensions() != (width, height) {
        output.push_str(
            "_Mipmap screenshots have different dimensions; rerun `cargo xtask capture-quality-matrix`._\n\n",
        );
        return Ok(output);
    }

    let diff = diff_image(&off, &on);
    let heat = heat_image(&diff);
    let diff_path = diff_dir.join(format!("{slug}-diff.png"));
    let heat_path = diff_dir.join(format!("{slug}-heat.png"));
    diff.save(&diff_path)?;
    heat.save(&heat_path)?;

    let face_crop = crop_rect(width, height, 38, 20, 24, 22);
    let hair_crop = crop_rect(width, height, 32, 17, 36, 27);
    let torso_crop = crop_rect(width, height, 32, 34, 36, 34);
    let edge_crop = crop_rect(width, height, 28, 17, 44, 60);
    let rows = [
        ("whole image", None),
        ("face and eyes", Some(face_crop)),
        ("hair", Some(hair_crop)),
        ("torso", Some(torso_crop)),
        ("edge-heavy avatar area", Some(edge_crop)),
    ];

    output.push_str(&format!(
        "- Mipmaps off: `{}`\n",
        relative_path(root, &off_image)
    ));
    output.push_str(&format!(
        "- Mipmaps on: `{}`\n",
        relative_path(root, &on_image)
    ));
    output.push_str(&format!("- Dimensions: {width}x{height}\n\n"));
    output.push_str("| Preview | Diff | Heatmap |\n");
    output.push_str("| --- | --- | --- |\n");
    output.push_str(&format!(
        "| <img src=\"{}\" width=\"120\"> | `{}` | `{}` |\n\n",
        report_image_path(root, output_dir, &heat_path),
        relative_path(root, &diff_path),
        relative_path(root, &heat_path)
    ));
    output.push_str("| Region | Crop | Mean Diff | Max Diff | Pixels >2% | Pixels >10% |\n");
    output.push_str("| --- | --- | ---: | ---: | ---: | ---: |\n");

    let mut focused_failed = false;
    for (label, crop) in rows {
        let metrics = diff_metrics(&diff, crop);
        if crop.is_some() && metrics.changed_strong_percent > 0.20 {
            focused_failed = true;
        }
        output.push_str(&format!(
            "| {label} | `{}` | {:.6} | {:.6} | {:.3}% | {:.3}% |\n",
            crop.map(format_crop_rect)
                .unwrap_or_else(|| "full".to_string()),
            metrics.mean,
            metrics.max,
            metrics.changed_soft_percent,
            metrics.changed_strong_percent
        ));
    }

    output.push_str("\n### Automatic Checks\n\n");
    if focused_failed {
        output.push_str(
            "- [ ] Investigate: one or more focused avatar regions changed strongly with mipmaps enabled.\n",
        );
    } else {
        output.push_str("- [x] Focused avatar regions stay within the mipmap diff threshold.\n");
    }
    output.push_str("- [x] Whole-image metrics are reported as info because diagnostics overlay text can change between captures.\n\n");
    output.push_str(&quality_anisotropy_diff(
        root,
        output_dir,
        diff_dir,
        &on_image,
        &aniso_image,
        model,
    )?);
    Ok(output)
}

fn quality_anisotropy_diff(
    root: &Path,
    output_dir: &Path,
    diff_dir: &Path,
    baseline_image: &Path,
    aniso_image: &Path,
    model: &str,
) -> Result<String> {
    let mut output = String::new();
    output.push_str("### Mipmaps On vs Anisotropy 8\n\n");

    if !aniso_image.is_file() {
        output.push_str(
            "_Missing anisotropy screenshot. Run `cargo xtask capture-quality-matrix` again._\n\n",
        );
        return Ok(output);
    }

    let baseline = image::open(baseline_image)?.to_rgba8();
    let aniso = image::open(aniso_image)?.to_rgba8();
    let (width, height) = baseline.dimensions();
    if aniso.dimensions() != (width, height) {
        output.push_str(
            "_Anisotropy screenshots have different dimensions; rerun `cargo xtask capture-quality-matrix`._\n\n",
        );
        return Ok(output);
    }

    let diff = diff_image(&baseline, &aniso);
    let heat = heat_image(&diff);
    let slug = format!("{model}-anisotropy");
    let diff_path = diff_dir.join(format!("{slug}-diff.png"));
    let heat_path = diff_dir.join(format!("{slug}-heat.png"));
    diff.save(&diff_path)?;
    heat.save(&heat_path)?;

    output.push_str(&format!(
        "- Mipmaps on, anisotropy 1: `{}`\n",
        relative_path(root, baseline_image)
    ));
    output.push_str(&format!(
        "- Mipmaps on, anisotropy 8: `{}`\n",
        relative_path(root, aniso_image)
    ));
    output.push_str("| Preview | Diff | Heatmap |\n");
    output.push_str("| --- | --- | --- |\n");
    output.push_str(&format!(
        "| <img src=\"{}\" width=\"120\"> | `{}` | `{}` |\n\n",
        report_image_path(root, output_dir, &heat_path),
        relative_path(root, &diff_path),
        relative_path(root, &heat_path)
    ));

    let edge_crop = crop_rect(width, height, 28, 17, 44, 60);
    output.push_str("| Region | Crop | Mean Diff | Max Diff | Pixels >2% | Pixels >10% |\n");
    output.push_str("| --- | --- | ---: | ---: | ---: | ---: |\n");
    for (label, crop) in [
        ("whole image", None),
        ("edge-heavy avatar area", Some(edge_crop)),
    ] {
        let metrics = diff_metrics(&diff, crop);
        output.push_str(&format!(
            "| {label} | `{}` | {:.6} | {:.6} | {:.3}% | {:.3}% |\n",
            crop.map(format_crop_rect)
                .unwrap_or_else(|| "full".to_string()),
            metrics.mean,
            metrics.max,
            metrics.changed_soft_percent,
            metrics.changed_strong_percent
        ));
    }
    output.push('\n');
    Ok(output)
}

fn ren_visual_diff_report(
    root: &Path,
    output_dir: &Path,
    diff_dir: &Path,
    shared_image: &Path,
    high_precision_image: &Path,
    no_mask_image: &Path,
) -> Result<String> {
    let shared = image::open(shared_image)?.to_rgba8();
    let high_precision = image::open(high_precision_image)?.to_rgba8();
    let no_mask = image::open(no_mask_image)?.to_rgba8();
    let (width, height) = shared.dimensions();
    if high_precision.dimensions() != (width, height) || no_mask.dimensions() != (width, height) {
        return Err(
            "Ren screenshots have different dimensions; rerun `cargo xtask capture-offscreen-matrix`."
                .into(),
        );
    }

    let shared_high = ren_pair_diff(
        root,
        output_dir,
        diff_dir,
        RenPairSpec {
            title: "Shared vs High Precision Fallback",
            slug: "shared-vs-high-precision",
            expectation: "Expected: nearly identical, because Ren currently falls high-precision masks back to the shared offscreen path.",
            left: &shared,
            right: &high_precision,
        },
    )?;
    let shared_no_mask = ren_pair_diff(
        root,
        output_dir,
        diff_dir,
        RenPairSpec {
            title: "Shared vs No Mask",
            slug: "shared-vs-no-mask",
            expectation: "Expected: differences should concentrate around masked pupil/offscreen regions and clipping-sensitive transparent layers.",
            left: &shared,
            right: &no_mask,
        },
    )?;
    let high_no_mask = ren_pair_diff(
        root,
        output_dir,
        diff_dir,
        RenPairSpec {
            title: "High Precision Fallback vs No Mask",
            slug: "high-precision-vs-no-mask",
            expectation: "Expected: similar to shared vs no-mask while offscreen fallback is active.",
            left: &high_precision,
            right: &no_mask,
        },
    )?;

    let mut report = String::new();
    report.push_str("# Ren Visual Diff\n\n");
    report.push_str(&format!("- Generated: {}\n", generated_stamp()));
    report.push_str(&format!(
        "- Shared: `{}`\n",
        relative_path(root, shared_image)
    ));
    report.push_str(&format!(
        "- High precision fallback: `{}`\n",
        relative_path(root, high_precision_image)
    ));
    report.push_str(&format!(
        "- No mask: `{}`\n",
        relative_path(root, no_mask_image)
    ));
    report.push_str(&format!("- Dimensions: {width}x{height}\n\n"));
    report.push_str(
        "\
## Manual Decision

| Area | Decision | Notes |
| --- | --- | --- |
| Shared vs high-precision fallback | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Masked pupils against no-mask | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Hair shadow / transparent layers | [ ] Pass [ ] Investigate renderer |  |
| Overall Ren visual diff | [ ] Pass [ ] Investigate renderer |  |

",
    );
    report.push_str(&shared_high.section);
    report.push_str(&shared_no_mask.section);
    report.push_str(&high_no_mask.section);
    report.push_str(&ren_automatic_diff_checks(
        width,
        height,
        &shared_high.diff,
        &shared_no_mask.diff,
    ));
    Ok(report)
}

struct RenPairSpec<'a> {
    title: &'a str,
    slug: &'a str,
    expectation: &'a str,
    left: &'a RgbaImage,
    right: &'a RgbaImage,
}

struct RenPairOutput {
    section: String,
    diff: RgbaImage,
}

fn ren_pair_diff(
    root: &Path,
    output_dir: &Path,
    diff_dir: &Path,
    spec: RenPairSpec<'_>,
) -> Result<RenPairOutput> {
    let (width, height) = spec.left.dimensions();
    let diff = diff_image(spec.left, spec.right);
    let heat = heat_image(&diff);
    let diff_path = diff_dir.join(format!("{}-diff.png", spec.slug));
    let heat_path = diff_dir.join(format!("{}-heat.png", spec.slug));
    diff.save(&diff_path)?;
    heat.save(&heat_path)?;

    let rows = ren_visual_diff_rows(width, height);
    let mut section = String::new();
    section.push_str(&format!("## {}\n\n", spec.title));
    section.push_str(spec.expectation);
    section.push_str("\n\n");
    section.push_str("| Preview | Diff | Heatmap |\n");
    section.push_str("| --- | --- | --- |\n");
    section.push_str(&format!(
        "| <img src=\"{}\" width=\"120\"> | `{}` | `{}` |\n\n",
        report_image_path(root, output_dir, &heat_path),
        relative_path(root, &diff_path),
        relative_path(root, &heat_path)
    ));
    section.push_str("| Region | Crop | Mean Diff | Max Diff | Pixels >2% | Pixels >10% |\n");
    section.push_str("| --- | --- | ---: | ---: | ---: | ---: |\n");
    for (label, crop) in rows {
        let metrics = diff_metrics(&diff, crop);
        section.push_str(&format!(
            "| {label} | `{}` | {:.6} | {:.6} | {:.3}% | {:.3}% |\n",
            crop.map(format_crop_rect)
                .unwrap_or_else(|| "full".to_string()),
            metrics.mean,
            metrics.max,
            metrics.changed_soft_percent,
            metrics.changed_strong_percent
        ));
    }
    section.push('\n');

    Ok(RenPairOutput { section, diff })
}

fn ren_visual_diff_rows(width: u32, height: u32) -> [(&'static str, Option<CropRect>); 5] {
    [
        ("whole image", None),
        (
            "face and eyes",
            Some(crop_rect(width, height, 38, 20, 24, 22)),
        ),
        (
            "hair shadow",
            Some(crop_rect(width, height, 32, 17, 36, 27)),
        ),
        (
            "torso and transparent layers",
            Some(crop_rect(width, height, 32, 34, 36, 34)),
        ),
        (
            "pupil offscreens",
            Some(crop_rect(width, height, 40, 24, 20, 18)),
        ),
    ]
}

fn ren_automatic_diff_checks(
    width: u32,
    height: u32,
    shared_high_diff: &RgbaImage,
    shared_no_mask_diff: &RgbaImage,
) -> String {
    let face_crop = crop_rect(width, height, 38, 20, 24, 22);
    let hair_crop = crop_rect(width, height, 32, 17, 36, 27);
    let torso_crop = crop_rect(width, height, 32, 34, 36, 34);
    let pupil_crop = crop_rect(width, height, 40, 24, 20, 18);
    let focused_crops = [face_crop, hair_crop, torso_crop, pupil_crop];

    let shared_high_failed = focused_crops
        .into_iter()
        .any(|crop| diff_metrics(shared_high_diff, Some(crop)).changed_strong_percent > 0.05);
    let pupil_no_mask_strong =
        diff_metrics(shared_no_mask_diff, Some(pupil_crop)).changed_strong_percent;

    let mut output = String::new();
    output.push_str("## Automatic Diff Checks\n\n");
    if shared_high_failed {
        output.push_str("- [ ] Investigate: shared and high-precision fallback differ in a focused avatar region.\n");
    } else {
        output.push_str(
            "- [x] Shared and high-precision fallback match in focused avatar regions.\n",
        );
    }
    if pupil_no_mask_strong > 0.01 {
        output.push_str(
            "- [x] Shared vs no-mask shows measurable pupil/offscreen clipping difference.\n",
        );
    } else {
        output.push_str(
            "- [ ] Investigate: shared vs no-mask has no measurable pupil/offscreen difference.\n",
        );
    }
    output.push_str("- [x] Whole-image metrics are reported but should be interpreted with care because diagnostics overlay text changes between captures.\n\n");
    output
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CropRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug)]
struct DiffMetrics {
    mean: f32,
    max: f32,
    changed_soft_percent: f32,
    changed_strong_percent: f32,
}

fn crop_rect(
    image_width: u32,
    image_height: u32,
    x_pct: u32,
    y_pct: u32,
    width_pct: u32,
    height_pct: u32,
) -> CropRect {
    let width = ((image_width * width_pct) / 100).max(1);
    let height = ((image_height * height_pct) / 100).max(1);
    let mut x = (image_width * x_pct) / 100;
    let mut y = (image_height * y_pct) / 100;
    if x + width > image_width {
        x = image_width.saturating_sub(width);
    }
    if y + height > image_height {
        y = image_height.saturating_sub(height);
    }
    CropRect {
        x,
        y,
        width,
        height,
    }
}

fn format_crop_rect(crop: CropRect) -> String {
    format!("{}x{}+{}+{}", crop.width, crop.height, crop.x, crop.y)
}

fn diff_image(left: &RgbaImage, right: &RgbaImage) -> RgbaImage {
    let (width, height) = left.dimensions();
    let mut diff = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let left = left.get_pixel(x, y).0;
            let right = right.get_pixel(x, y).0;
            diff.put_pixel(
                x,
                y,
                Rgba([
                    left[0].abs_diff(right[0]),
                    left[1].abs_diff(right[1]),
                    left[2].abs_diff(right[2]),
                    255,
                ]),
            );
        }
    }
    diff
}

fn heat_image(diff: &RgbaImage) -> RgbaImage {
    let max_gray = diff
        .pixels()
        .map(|pixel| grayscale(pixel.0))
        .max()
        .unwrap_or(0)
        .max(1);
    let (width, height) = diff.dimensions();
    let mut heat = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let gray = grayscale(diff.get_pixel(x, y).0);
            let value = ((u16::from(gray) * 255) / u16::from(max_gray)) as u8;
            heat.put_pixel(x, y, Rgba([value, value, value, 255]));
        }
    }
    heat
}

fn diff_metrics(diff: &RgbaImage, crop: Option<CropRect>) -> DiffMetrics {
    let (image_width, image_height) = diff.dimensions();
    let crop = crop.unwrap_or(CropRect {
        x: 0,
        y: 0,
        width: image_width,
        height: image_height,
    });
    let mut total = 0.0f32;
    let mut max = 0.0f32;
    let mut changed_soft = 0u64;
    let mut changed_strong = 0u64;
    let mut count = 0u64;
    let end_y = (crop.y + crop.height).min(image_height);
    let end_x = (crop.x + crop.width).min(image_width);
    for y in crop.y..end_y {
        for x in crop.x..end_x {
            let gray = f32::from(grayscale(diff.get_pixel(x, y).0)) / 255.0;
            total += gray;
            max = max.max(gray);
            if gray > 0.02 {
                changed_soft += 1;
            }
            if gray > 0.10 {
                changed_strong += 1;
            }
            count += 1;
        }
    }
    if count == 0 {
        return DiffMetrics {
            mean: 0.0,
            max: 0.0,
            changed_soft_percent: 0.0,
            changed_strong_percent: 0.0,
        };
    }
    DiffMetrics {
        mean: total / count as f32,
        max,
        changed_soft_percent: changed_soft as f32 * 100.0 / count as f32,
        changed_strong_percent: changed_strong as f32 * 100.0 / count as f32,
    }
}

fn grayscale(rgba: [u8; 4]) -> u8 {
    ((u16::from(rgba[0]) + u16::from(rgba[1]) + u16::from(rgba[2])) / 3) as u8
}

fn model_label(model: &str) -> &str {
    match model {
        "0" => "Default model",
        other => other,
    }
}

fn render_regression_report_markdown(root: &Path, output_dir: &Path) -> String {
    let mut report = String::new();
    report.push_str("# vtube-studio-rs Render Regression Report\n\n");
    report.push_str(&format!("- Generated: {}\n", generated_stamp()));
    report.push_str(&format!("- Root: `{}`\n", root.display()));
    report.push_str(&format!(
        "- Output: `{}`\n\n",
        relative_path(root, output_dir)
    ));
    report.push_str(
        "\
## Manual Checklist

- [ ] Default model looks complete and correctly layered.
- [ ] Mao shared/high-precision/no-mask screenshots show expected clipping differences.
- [ ] Ren shared/high-precision/no-mask screenshots preserve offscreen composites.
- [ ] Rice optional stress screenshots, when present, do not reveal new additive, inverted-mask, or translucent artifacts.
- [ ] Mipmaps-on screenshots do not show obvious atlas island bleed.
- [ ] Mipmaps-on-aniso8 screenshots do not introduce blur, haloing, or atlas bleed.
- [ ] Mipmaps-off screenshots remain crisp without severe shimmer in static capture.
- [ ] Notes:

",
    );
    report.push_str(&contact_sheet(root, output_dir));
    report.push_str(&manual_review_record());
    report.push_str(&latest_table(
        root,
        output_dir,
        "Risk Model Sweep",
        output_dir,
        "Default model plus high-risk sample captures. Use these for broad regressions.",
    ));
    report.push_str(&latest_table(
        root,
        output_dir,
        "Mao Mask Matrix",
        &output_dir.join("mask-matrix"),
        "Compare shared, high-precision, and no-mask clipping behavior.",
    ));
    report.push_str(&latest_table(
        root,
        output_dir,
        "Ren Offscreen Matrix",
        &output_dir.join("offscreen-matrix"),
        "Compare shared, high-precision fallback, and no-mask offscreen behavior.",
    ));
    report.push_str(&latest_table(
        root,
        output_dir,
        "Rice Optional Stress Matrix",
        &output_dir.join("rice-stress"),
        "Optional stress coverage for additive, inverted-mask, and translucent drawable behavior.",
    ));
    report.push_str(&latest_table(
        root,
        output_dir,
        "Quality Matrix",
        &output_dir.join("quality-matrix"),
        "Compare texture atlas mipmaps off/on and anisotropy 1/8 for shimmer, oblique sampling, and atlas bleed.",
    ));
    report.push_str(&review_focus(root, output_dir));
    report.push_str(&probe_summary(root, output_dir));
    report.push_str(&fallback_summary(root, output_dir));
    report.push_str(&msaa_summary(root, output_dir));
    report.push_str(&retina_resize_summary(root, output_dir));
    report.push_str(&audit_summary(
        root,
        &output_dir.join("mao-mask-audit.md"),
        "Mao Mask Matrix Audit",
        "No Mao mask audit found. Run `cargo xtask mao-mask-audit`.",
        SummaryMode::UntilHeading("## Raw Probe"),
    ));
    report.push_str(&audit_summary(
        root,
        &output_dir.join("ren-offscreen-audit.md"),
        "Ren Offscreen Audit",
        "No Ren audit found. Run `cargo xtask ren-offscreen-audit`.",
        SummaryMode::UntilHeading("## Raw Probe"),
    ));
    report.push_str(&audit_summary(
        root,
        &output_dir.join("ren-visual-diff.md"),
        "Ren Visual Diff",
        "No Ren visual diff found. Run `cargo xtask ren-visual-diff`.",
        SummaryMode::RenVisualDiff,
    ));
    report.push_str(&audit_summary(
        root,
        &output_dir.join("quality-visual-diff.md"),
        "Quality Visual Diff",
        "No quality visual diff found. Run `cargo xtask quality-visual-diff`.",
        SummaryMode::UntilHeading("## Mao"),
    ));
    report.push_str(&audit_summary(
        root,
        &output_dir.join("rice-stress-audit.md"),
        "Rice Optional Stress Audit",
        "No Rice stress audit found. Run `cargo xtask rice-stress-audit`.",
        SummaryMode::UntilHeading("## Raw Probe"),
    ));
    report.push_str(&capture_log_summary(root, output_dir));
    report.push_str(
        "\
## Interpretation Guide

- Mask regressions usually show up first in the Mao matrix.
- Offscreen and extended blend regressions usually show up first in the Ren matrix.
- Mipmap regressions usually appear as blurry details or color bleed from neighboring atlas islands.
- If a capture is missing, run the corresponding `cargo xtask capture-*` command and regenerate this report.
",
    );
    report
}

fn latest_table(
    root: &Path,
    output_dir: &Path,
    title: &str,
    directory: &Path,
    note: &str,
) -> String {
    let mut output = String::new();
    output.push_str(&format!("## {title}\n\n{note}\n\n"));
    output.push_str("| Preview | Screenshot | Path |\n");
    output.push_str("| --- | --- | --- |\n");
    let images = latest_pngs(directory);
    if images.is_empty() {
        output.push_str(&format!(
            "|  | _No latest screenshots found_ | `{}` |\n",
            relative_path(root, directory)
        ));
    } else {
        for image in images {
            let name = image
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("latest.png");
            output.push_str(&format!(
                "| <img src=\"{}\" width=\"160\"> | {} | `{}` |\n",
                report_image_path(root, output_dir, &image),
                name,
                relative_path(root, &image)
            ));
        }
    }
    output.push('\n');
    output
}

fn contact_sheet(root: &Path, output_dir: &Path) -> String {
    let mut output = String::new();
    output.push_str("## Visual Contact Sheet\n\n");
    output.push_str(
        "Use this section for fast visual triage before opening individual PNG files.\n\n",
    );
    for (title, directory, note) in [
        (
            "Risk Model Sweep",
            output_dir.to_path_buf(),
            "Broad baseline captures for the default model, Mao, and Ren.",
        ),
        (
            "Mao Mask Matrix",
            output_dir.join("mask-matrix"),
            "Shared, high-precision, and no-mask clipping comparison.",
        ),
        (
            "Ren Offscreen Matrix",
            output_dir.join("offscreen-matrix"),
            "Shared, high-precision fallback, and no-mask offscreen comparison.",
        ),
        (
            "Rice Optional Stress Matrix",
            output_dir.join("rice-stress"),
            "Optional stress coverage for additive, inverted-mask, and translucent drawable behavior.",
        ),
        (
            "Quality Matrix",
            output_dir.join("quality-matrix"),
            "Texture atlas mipmaps off/on and anisotropy 1/8 comparison.",
        ),
    ] {
        output.push_str(&contact_sheet_group(
            root, output_dir, title, &directory, note,
        ));
    }
    output
}

fn contact_sheet_group(
    root: &Path,
    output_dir: &Path,
    title: &str,
    directory: &Path,
    note: &str,
) -> String {
    let mut output = String::new();
    output.push_str(&format!("### {title}\n\n{note}\n\n"));
    let images = latest_pngs(directory);
    if images.is_empty() {
        output.push_str(&format!(
            "_No latest screenshots found in `{}`._\n\n",
            relative_path(root, directory)
        ));
        return output;
    }
    for image in images {
        let name = image
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("latest.png");
        output.push_str(&format!(
            "<figure style=\"display:inline-block;margin:0 12px 18px 0;vertical-align:top;width:180px;\">\n  <img src=\"{}\" width=\"180\">\n  <figcaption><code>{}</code></figcaption>\n</figure>\n",
            report_image_path(root, output_dir, &image),
            name
        ));
    }
    output.push('\n');
    output
}

fn manual_review_record() -> String {
    "\
## Manual Review Record

Fill this section after reviewing the contact sheet and latest screenshots.

| Area | Decision | Notes |
| --- | --- | --- |
| Default baseline | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Mao clipping | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Ren offscreen/extended blend | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Rice optional stress | [ ] Pass [ ] Investigate [ ] Rerun [ ] Not available |  |
| Texture quality/mipmaps | [ ] Pass [ ] Investigate [ ] Rerun |  |
| Texture quality/anisotropy | [ ] Pass [ ] Investigate [ ] Rerun |  |

Overall decision:

- [ ] Proceed with renderer changes.
- [ ] Investigate renderer regression before continuing.
- [ ] Rerun full matrix before deciding.

"
    .to_string()
}

fn review_focus(root: &Path, output_dir: &Path) -> String {
    let probe_path = output_dir.join("probe.txt");
    let probe = fs::read_to_string(&probe_path).ok();
    let mut output = String::new();
    output.push_str("## Review Focus\n\n");
    output.push_str("| Area | Why It Matters | Screenshots To Check |\n");
    output.push_str("| --- | --- | --- |\n");

    if let Some(probe) = probe.as_deref() {
        let models = parse_probe_models(probe);
        push_model_focus(
            &mut output,
            &models,
            "Mao.model3.json",
            "Mao clipping",
            "High-mask sample model present in probe.",
            "`target/render-regression/mask-matrix/latest-Mao-shared.png`<br>`target/render-regression/mask-matrix/latest-Mao-high-precision.png`<br>`target/render-regression/mask-matrix/latest-Mao-no-mask.png`",
        );
        push_model_focus(
            &mut output,
            &models,
            "Ren.model3.json",
            "Ren offscreen/extended blend",
            "Offscreen sample model present in probe.",
            "`target/render-regression/offscreen-matrix/latest-Ren-shared.png`<br>`target/render-regression/offscreen-matrix/latest-Ren-high-precision.png`<br>`target/render-regression/offscreen-matrix/latest-Ren-no-mask.png`",
        );
        push_model_focus(
            &mut output,
            &models,
            "Rice.model3.json",
            "Rice optional stress",
            "Optional Rice stress model present in probe.",
            "`target/render-regression/rice-stress/latest-Rice-shared.png`<br>`target/render-regression/rice-stress/latest-Rice-high-precision.png`<br>`target/render-regression/rice-stress/latest-Rice-no-mask.png`",
        );
        push_model_focus(
            &mut output,
            &models,
            "public/model/0.model3.json",
            "Default model baseline",
            "Local default model baseline.",
            "`target/render-regression/latest-0.png`<br>`target/render-regression/quality-matrix/latest-0-mipmaps-off.png`<br>`target/render-regression/quality-matrix/latest-0-mipmaps-on.png`<br>`target/render-regression/quality-matrix/latest-0-mipmaps-on-aniso8.png`",
        );
    }

    let fallback_events = fallback_event_lines(root, output_dir, false);
    if !fallback_events.is_empty() {
        output.push_str(&format!(
            "| High precision mask fallback | {} | `target/render-regression/offscreen-matrix/latest-Ren-high-precision.png`<br>`target/render-regression/fallback-diagnostics-smoke/report.md` |\n",
            fallback_events.join("<br>")
        ));
    }
    let quality_images = latest_png_names(&output_dir.join("quality-matrix"));
    if quality_images
        .iter()
        .any(|name| name.contains("-mipmaps-on"))
    {
        output.push_str("| Texture sampling | Mipmaps-on captures are present; check for atlas island bleed or unexpected blur. | `target/render-regression/quality-matrix/latest-0-mipmaps-on.png`<br>`target/render-regression/quality-matrix/latest-Mao-mipmaps-on.png`<br>`target/render-regression/quality-matrix/latest-Ren-mipmaps-on.png` |\n");
    }
    if quality_images
        .iter()
        .any(|name| name.contains("-mipmaps-on-aniso8"))
    {
        output.push_str("| Texture anisotropy | Anisotropy-8 captures are present; compare against mipmaps-on for oblique sampling blur, halos, and atlas bleed. | `target/render-regression/quality-matrix/latest-0-mipmaps-on-aniso8.png`<br>`target/render-regression/quality-matrix/latest-Mao-mipmaps-on-aniso8.png`<br>`target/render-regression/quality-matrix/latest-Ren-mipmaps-on-aniso8.png`<br>`target/render-regression/quality-visual-diff.md` |\n");
    }
    if let Some(msaa) = msaa_overview(output_dir) {
        let why = if msaa.max_sample_count > 1 {
            format!(
                "{}x MSAA captures are present; check transparent window edges and avatar mesh edges for stair-step artifacts.",
                msaa.max_sample_count
            )
        } else {
            "MSAA appears disabled or unsupported in capture logs; inspect transparent edges more carefully.".to_string()
        };
        output.push_str(&format!(
            "| Transparent edge antialiasing | {} | `target/render-regression/latest-0.png`<br>`target/render-regression/report.md#msaa--edge-quality` |\n",
            escape_markdown_table_cell(&why)
        ));
    }
    if let Some(resize) = retina_resize_overview(output_dir) {
        let why = format!(
            "{} drawable size event(s), {} mask texture event(s), max physical size {} px.",
            resize.drawable_size_events, resize.mask_texture_events, resize.max_physical_size
        );
        output.push_str(&format!(
            "| Retina / window resize stability | {} Check mask and offscreen edges after resize or backing-scale changes. | `target/render-regression/report.md#retina--resize-stability` |\n",
            escape_markdown_table_cell(&why)
        ));
    }
    if probe.is_none() {
        output.push_str("| Probe missing | Run `cargo xtask probe-risk-models` or `cargo xtask capture-risk-models` to populate model-specific review focus. | `target/render-regression/probe.txt` |\n");
    }
    output.push('\n');
    output
}

fn push_model_focus(
    output: &mut String,
    models: &[ProbeModel],
    needle: &str,
    area: &str,
    fallback: &str,
    screenshots: &str,
) {
    if let Some(model) = models.iter().find(|model| model.path.contains(needle)) {
        let reasons = if model.reasons == "No specific risk lines." {
            fallback
        } else {
            &model.reasons
        };
        output.push_str(&format!("| {area} | {reasons} | {screenshots} |\n"));
    }
}

fn probe_summary(root: &Path, output_dir: &Path) -> String {
    let probe_path = output_dir.join("probe.txt");
    let mut output = String::new();
    output.push_str("## Model Risk Probe\n\n");
    match fs::read_to_string(&probe_path) {
        Ok(probe) => {
            output.push_str(&format!(
                "Source: `{}`\n\n",
                relative_path(root, &probe_path)
            ));
            output.push_str("```text\n");
            output.push_str(&first_lines(&probe, 220));
            output.push_str("```\n\n");
        }
        Err(_) => {
            output.push_str(
                "No probe output found. Run `cargo xtask probe-risk-models` or `cargo xtask capture-risk-models`.\n\n",
            );
        }
    }
    output
}

fn fallback_summary(root: &Path, output_dir: &Path) -> String {
    let mut output = String::new();
    output.push_str("## Renderer Fallbacks\n\n");
    output.push_str("| Log | Fallback Events |\n");
    output.push_str("| --- | --- |\n");
    let mut found = false;
    for log_path in capture_logs(output_dir) {
        let events =
            file_lines_containing(&log_path, "renderer_event=high_precision_mask_fallback");
        if events.is_empty() {
            continue;
        }
        found = true;
        output.push_str(&format!(
            "| `{}` | {} |\n",
            relative_path(root, &log_path),
            events
                .into_iter()
                .map(|line| escape_markdown_table_cell(&line))
                .collect::<Vec<_>>()
                .join("<br>")
        ));
    }
    if !found {
        output.push_str(
            "| _No fallback events found_ | No `high_precision_mask_fallback` events recorded in capture logs. |\n",
        );
    }
    output.push('\n');
    output
}

#[derive(Debug, Default)]
struct MsaaOverview {
    max_sample_count: u64,
    initialized_logs: usize,
    resize_events: usize,
}

fn msaa_overview(output_dir: &Path) -> Option<MsaaOverview> {
    let mut overview = MsaaOverview::default();
    for log_path in capture_logs(output_dir) {
        let Ok(log) = fs::read_to_string(log_path) else {
            continue;
        };
        for line in log.lines() {
            if line.contains("renderer_event=metal_initialized") {
                overview.initialized_logs += 1;
                if let Some(sample_count) = renderer_event_u64(line, "sample_count") {
                    overview.max_sample_count = overview.max_sample_count.max(sample_count);
                }
            }
            if line.contains("renderer_event=msaa_texture_resized") {
                overview.resize_events += 1;
            }
        }
    }
    if overview.initialized_logs == 0 {
        None
    } else {
        Some(overview)
    }
}

fn msaa_summary(root: &Path, output_dir: &Path) -> String {
    let mut output = String::new();
    output.push_str("## MSAA / Edge Quality\n\n");
    output.push_str("| Log | Sample Count | MSAA Resize Events | Review |\n");
    output.push_str("| --- | ---: | ---: | --- |\n");

    let mut found = false;
    for log_path in capture_logs(output_dir) {
        let Ok(log) = fs::read_to_string(&log_path) else {
            continue;
        };
        let sample_count = log
            .lines()
            .find(|line| line.contains("renderer_event=metal_initialized"))
            .and_then(|line| renderer_event_u64(line, "sample_count"));
        let resize_events = log
            .lines()
            .filter(|line| line.contains("renderer_event=msaa_texture_resized"))
            .count();
        if sample_count.is_none() && resize_events == 0 {
            continue;
        }

        found = true;
        let sample_count = sample_count.unwrap_or(1);
        let review = if sample_count > 1 {
            "MSAA active; inspect transparent edges for remaining stair-step artifacts."
        } else {
            "MSAA unavailable or disabled; transparent edges need closer manual review."
        };
        output.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            relative_path(root, &log_path),
            sample_count,
            resize_events,
            review
        ));
    }

    if !found {
        output.push_str("| _No MSAA events found_ |  |  | Run a capture command first. |\n");
    }
    output.push('\n');
    output
}

fn renderer_event_u64(line: &str, key: &str) -> Option<u64> {
    let marker = format!("{key}=");
    let (_, rest) = line.split_once(&marker)?;
    first_token(rest)?.parse().ok()
}

#[derive(Debug, Default)]
struct RetinaResizeOverview {
    contents_scale_events: usize,
    drawable_size_events: usize,
    mask_texture_events: usize,
    offscreen_texture_events: usize,
    max_physical_size: u64,
    max_mask_texture_size: u64,
}

fn retina_resize_overview(output_dir: &Path) -> Option<RetinaResizeOverview> {
    let mut overview = RetinaResizeOverview::default();
    for log_path in capture_logs(output_dir) {
        let Ok(log) = fs::read_to_string(log_path) else {
            continue;
        };
        for line in log.lines() {
            if line.contains("renderer_event=contents_scale_changed") {
                overview.contents_scale_events += 1;
            }
            if line.contains("renderer_event=drawable_size_changed") {
                overview.drawable_size_events += 1;
                if let Some(physical) = renderer_event_u64(line, "physical") {
                    overview.max_physical_size = overview.max_physical_size.max(physical);
                }
            }
            if line.contains("renderer_event=mask_tile_size_changed") {
                overview.mask_texture_events += 1;
                if let Some(size) = renderer_event_u64(line, "new") {
                    overview.max_mask_texture_size = overview.max_mask_texture_size.max(size);
                }
            } else if line.contains("renderer_event=mask_atlas_resized")
                || line.contains("renderer_event=high_precision_mask_texture_size_changed")
            {
                overview.mask_texture_events += 1;
                if let Some(size) = renderer_event_u64(line, "texture_size")
                    .or_else(|| renderer_event_u64(line, "new"))
                {
                    overview.max_mask_texture_size = overview.max_mask_texture_size.max(size);
                }
            }
            if line.contains("renderer_event=offscreen_texture_size_changed")
                || line.contains("renderer_event=blend_snapshot_texture_size_changed")
            {
                overview.offscreen_texture_events += 1;
            }
        }
    }
    if overview.contents_scale_events == 0
        && overview.drawable_size_events == 0
        && overview.mask_texture_events == 0
        && overview.offscreen_texture_events == 0
    {
        None
    } else {
        Some(overview)
    }
}

fn retina_resize_summary(root: &Path, output_dir: &Path) -> String {
    let mut output = String::new();
    output.push_str("## Retina / Resize Stability\n\n");
    output.push_str("| Log | Contents Scale | Drawable Size | Mask Texture | Offscreen Texture | Max Physical | Max Mask Texture | Review |\n");
    output.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n");

    let mut found = false;
    for log_path in capture_logs(output_dir) {
        let Ok(log) = fs::read_to_string(&log_path) else {
            continue;
        };
        let mut contents_scale_events = 0;
        let mut drawable_size_events = 0;
        let mut mask_texture_events = 0;
        let mut offscreen_texture_events = 0;
        let mut max_physical_size = 0_u64;
        let mut max_mask_texture_size = 0_u64;
        for line in log.lines() {
            if line.contains("renderer_event=contents_scale_changed") {
                contents_scale_events += 1;
            }
            if line.contains("renderer_event=drawable_size_changed") {
                drawable_size_events += 1;
                if let Some(physical) = renderer_event_u64(line, "physical") {
                    max_physical_size = max_physical_size.max(physical);
                }
            }
            if line.contains("renderer_event=mask_tile_size_changed") {
                mask_texture_events += 1;
                if let Some(size) = renderer_event_u64(line, "new") {
                    max_mask_texture_size = max_mask_texture_size.max(size);
                }
            } else if line.contains("renderer_event=mask_atlas_resized")
                || line.contains("renderer_event=high_precision_mask_texture_size_changed")
            {
                mask_texture_events += 1;
                if let Some(size) = renderer_event_u64(line, "texture_size")
                    .or_else(|| renderer_event_u64(line, "new"))
                {
                    max_mask_texture_size = max_mask_texture_size.max(size);
                }
            }
            if line.contains("renderer_event=offscreen_texture_size_changed")
                || line.contains("renderer_event=blend_snapshot_texture_size_changed")
            {
                offscreen_texture_events += 1;
            }
        }
        if contents_scale_events == 0
            && drawable_size_events == 0
            && mask_texture_events == 0
            && offscreen_texture_events == 0
        {
            continue;
        }

        found = true;
        let review = if mask_texture_events > 0 || offscreen_texture_events > 0 {
            "Resize touched mask/offscreen textures; inspect clipped edges and offscreen composites."
        } else {
            "Drawable geometry changed; confirm avatar framing and Retina sharpness."
        };
        output.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} | {} | {} |\n",
            relative_path(root, &log_path),
            contents_scale_events,
            drawable_size_events,
            mask_texture_events,
            offscreen_texture_events,
            display_u64_or_dash(max_physical_size),
            display_u64_or_dash(max_mask_texture_size),
            review
        ));
    }

    if !found {
        output.push_str(
            "| _No resize events found_ |  |  |  |  |  |  | Run a capture or Space test first. |\n",
        );
    }
    output.push('\n');
    output
}

fn display_u64_or_dash(value: u64) -> String {
    if value == 0 {
        "-".to_string()
    } else {
        value.to_string()
    }
}

#[derive(Clone, Copy)]
enum SummaryMode {
    UntilHeading(&'static str),
    RenVisualDiff,
}

fn audit_summary(
    root: &Path,
    path: &Path,
    title: &str,
    missing_message: &str,
    mode: SummaryMode,
) -> String {
    let mut output = String::new();
    output.push_str(&format!("## {title}\n\n"));
    let Ok(content) = fs::read_to_string(path) else {
        output.push_str(missing_message);
        output.push_str("\n\n");
        return output;
    };
    output.push_str(&format!("Source: `{}`\n\n", relative_path(root, path)));
    match mode {
        SummaryMode::UntilHeading(heading) => output.push_str(&content_until(&content, heading)),
        SummaryMode::RenVisualDiff => {
            output.push_str(&content_until(&content, "## Shared vs No Mask"));
            if let Some(checks) = content_from(&content, "## Automatic Diff Checks") {
                output.push_str(checks);
            }
        }
    }
    output.push('\n');
    output
}

fn capture_log_summary(root: &Path, output_dir: &Path) -> String {
    let mut output = String::new();
    output.push_str("## Capture Log Summaries\n\n");
    output.push_str("| Directory | Last Renderer Events |\n");
    output.push_str("| --- | --- |\n");
    let logs = capture_logs(output_dir);
    if logs.is_empty() {
        output.push_str("| _No capture logs found_ | Run a capture command first. |\n\n");
        return output;
    }
    for log_path in logs {
        let events = tail_lines_containing(&log_path, "renderer_event=", 5);
        let events = if events.is_empty() {
            "No renderer_event lines recorded.".to_string()
        } else {
            events
                .into_iter()
                .map(|line| escape_markdown_table_cell(&line))
                .collect::<Vec<_>>()
                .join("<br>")
        };
        output.push_str(&format!(
            "| `{}` | {} |\n",
            relative_path(root, &log_path),
            events
        ));
    }
    output.push('\n');
    output
}

#[derive(Clone, Copy)]
enum OffscreenTableFilter {
    All,
    Masked,
    Nested,
    Extended,
}

fn offscreen_table(probe: &str, filter: OffscreenTableFilter) -> String {
    let mut output = String::new();
    match filter {
        OffscreenTableFilter::Nested => {
            output.push_str("| Offscreen | Owner | Depth | Render | Blend | Opacity |\n");
            output.push_str("| ---: | --- | ---: | ---: | --- | ---: |\n");
        }
        OffscreenTableFilter::Extended => {
            output.push_str("| Offscreen | Owner | Depth | Render | Blend | Opacity | Masks |\n");
            output.push_str("| ---: | --- | ---: | ---: | --- | ---: | ---: |\n");
        }
        OffscreenTableFilter::All | OffscreenTableFilter::Masked => {
            output.push_str(
                "| Offscreen | Owner | Depth | Render | Blend | Opacity | Masks | Inverted Mask |\n",
            );
            output.push_str("| ---: | --- | ---: | ---: | --- | ---: | ---: | --- |\n");
        }
    }

    for offscreen in probe.lines().filter_map(parse_offscreen_line) {
        let include = match filter {
            OffscreenTableFilter::All => true,
            OffscreenTableFilter::Masked => offscreen.masks > 0,
            OffscreenTableFilter::Nested => offscreen.depth > 1,
            OffscreenTableFilter::Extended => offscreen.blend.starts_with("Extended"),
        };
        if !include {
            continue;
        }

        match filter {
            OffscreenTableFilter::Nested => output.push_str(&format!(
                "| {} | `{}` | {} | {} | {} | {} |\n",
                offscreen.index,
                escape_markdown_table_cell(&offscreen.owner),
                offscreen.depth,
                offscreen.render_order,
                escape_markdown_table_cell(&offscreen.blend),
                offscreen.opacity
            )),
            OffscreenTableFilter::Extended => output.push_str(&format!(
                "| {} | `{}` | {} | {} | {} | {} | {} |\n",
                offscreen.index,
                escape_markdown_table_cell(&offscreen.owner),
                offscreen.depth,
                offscreen.render_order,
                escape_markdown_table_cell(&offscreen.blend),
                offscreen.opacity,
                offscreen.masks
            )),
            OffscreenTableFilter::All | OffscreenTableFilter::Masked => {
                output.push_str(&format!(
                    "| {} | `{}` | {} | {} | {} | {} | {} | {} |\n",
                    offscreen.index,
                    escape_markdown_table_cell(&offscreen.owner),
                    offscreen.depth,
                    offscreen.render_order,
                    escape_markdown_table_cell(&offscreen.blend),
                    offscreen.opacity,
                    offscreen.masks,
                    offscreen.inverted_mask
                ));
            }
        }
    }
    output
}

fn extended_drawable_table(probe: &str) -> String {
    let mut output = String::new();
    output.push_str("| Drawable | Part | Render | Blend | Opacity | Masks |\n");
    output.push_str("| ---: | --- | ---: | --- | ---: | ---: |\n");
    for drawable in probe.lines().filter_map(parse_drawable_line) {
        if drawable.kind != DrawableLineKind::Drawable || !drawable.blend.starts_with("Extended") {
            continue;
        }
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} |\n",
            drawable.index,
            escape_markdown_table_cell(&drawable.part),
            drawable.render_order,
            escape_markdown_table_cell(&drawable.blend),
            drawable.opacity,
            drawable.masks
        ));
    }
    output
}

fn offscreen_plan_timeline(probe: &str) -> String {
    let mut output = String::new();
    output.push_str("| Step | Plan Detail |\n");
    output.push_str("| ---: | --- |\n");
    for (index, line) in probe
        .lines()
        .filter_map(|line| line.strip_prefix("  offscreen_plan "))
        .enumerate()
    {
        output.push_str(&format!(
            "| {} | `{}` |\n",
            index + 1,
            escape_markdown_table_cell(&format!("offscreen_plan {line}"))
        ));
    }
    output
}

fn offscreen_plan_checks(probe: &str) -> String {
    let plan_lines: Vec<&str> = probe
        .lines()
        .filter(|line| line.starts_with("  offscreen_plan "))
        .collect();
    if plan_lines.is_empty() {
        return "- [ ] Investigate: no offscreen plan timeline was emitted by the model probe.\n"
            .to_string();
    }

    let mut output = String::new();
    output.push_str(if every_begun_offscreen_flushed(&plan_lines) {
        "- [x] Every begun offscreen has a matching flush.\n"
    } else {
        "- [ ] Investigate: at least one begun offscreen has no matching flush.\n"
    });
    output.push_str(if extended_drawables_have_snapshots(&plan_lines) {
        "- [x] Visible nonzero extended drawables have a snapshot before draw.\n"
    } else {
        "- [ ] Investigate: a visible nonzero extended drawable is missing a snapshot.\n"
    });
    output.push_str(if extended_offscreens_have_snapshots(&plan_lines) {
        "- [x] Nonzero extended offscreens have a snapshot before flush.\n"
    } else {
        "- [ ] Investigate: a nonzero extended offscreen is missing a snapshot.\n"
    });
    output.push_str(if masked_offscreens_flushed(&plan_lines) {
        "- [x] Masked offscreens are flushed through the compositor path.\n"
    } else {
        "- [ ] Investigate: a masked offscreen did not flush through the compositor path.\n"
    });
    output
}

fn every_begun_offscreen_flushed(lines: &[&str]) -> bool {
    let begun = collect_plan_ids(lines, "  offscreen_plan begin offscreen #");
    let flushed = collect_plan_ids(lines, "  offscreen_plan flush offscreen #");
    begun.iter().all(|id| flushed.contains(id))
}

fn extended_drawables_have_snapshots(lines: &[&str]) -> bool {
    let snapshots = collect_plan_ids(lines, "extended_drawable #");
    lines
        .iter()
        .filter(|line| {
            line.starts_with("  offscreen_plan draw drawable #")
                && line.contains(" blend Extended")
                && line.contains(" visible=true")
                && !line.contains(" opacity 0.000")
        })
        .all(|line| {
            id_after_marker(line, "drawable #")
                .map(|id| snapshots.iter().any(|snapshot| *snapshot == id))
                .unwrap_or(false)
        })
}

fn extended_offscreens_have_snapshots(lines: &[&str]) -> bool {
    let snapshots = collect_plan_ids(lines, "extended_offscreen #");
    lines
        .iter()
        .filter(|line| {
            line.starts_with("  offscreen_plan flush offscreen #")
                && line.contains(" blend Extended")
                && !line.contains(" opacity 0.000")
        })
        .all(|line| {
            id_after_marker(line, "offscreen #")
                .map(|id| snapshots.iter().any(|snapshot| *snapshot == id))
                .unwrap_or(false)
        })
}

fn masked_offscreens_flushed(lines: &[&str]) -> bool {
    let masked: Vec<&str> = lines
        .iter()
        .filter(|line| {
            line.starts_with("  offscreen_plan begin offscreen #")
                && value_after(line, " masks ")
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(0)
                    > 0
        })
        .filter_map(|line| id_after_marker(line, "offscreen #"))
        .collect();
    let flushed = collect_plan_ids(lines, "  offscreen_plan flush offscreen #");
    masked.iter().all(|id| flushed.contains(id))
}

fn collect_plan_ids<'a>(lines: &[&'a str], marker: &str) -> Vec<&'a str> {
    lines
        .iter()
        .filter_map(|line| id_after_marker(line, marker))
        .collect()
}

fn id_after_marker<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    line.split_once(marker)
        .and_then(|(_, rest)| rest.split_whitespace().next())
}

fn risk_lines_as_markdown(probe: &str) -> String {
    let mut output = String::new();
    for line in probe
        .lines()
        .filter_map(|line| line.strip_prefix("  risk "))
    {
        output.push_str("- ");
        output.push_str(line);
        output.push('\n');
    }
    output
}

fn masked_drawable_table(probe: &str) -> String {
    let mut output = String::new();
    output.push_str("| Drawable | Part | Render | Blend | Opacity | Masks |\n");
    output.push_str("| ---: | --- | ---: | --- | ---: | ---: |\n");
    for drawable in probe.lines().filter_map(parse_drawable_line) {
        if !matches!(
            drawable.kind,
            DrawableLineKind::Drawable | DrawableLineKind::Eye
        ) || drawable.masks == 0
        {
            continue;
        }
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} |\n",
            drawable.index,
            escape_markdown_table_cell(&drawable.part),
            drawable.render_order,
            escape_markdown_table_cell(&drawable.blend),
            drawable.opacity,
            drawable.masks
        ));
    }
    output
}

fn inverted_mask_table(probe: &str) -> String {
    let mut output = String::new();
    output.push_str("| Drawable | Part | Render | Opacity | Masks | Visible |\n");
    output.push_str("| ---: | --- | ---: | ---: | ---: | --- |\n");
    for drawable in probe.lines().filter_map(parse_drawable_line) {
        if drawable.kind != DrawableLineKind::Inverted {
            continue;
        }
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} |\n",
            drawable.index,
            escape_markdown_table_cell(&drawable.part),
            drawable.render_order,
            drawable.opacity,
            drawable.masks,
            drawable.visible.as_deref().unwrap_or("-")
        ));
    }
    output
}

fn eye_mask_table(probe: &str) -> String {
    let mut output = String::new();
    output.push_str(
        "| Drawable | Part | Render | Opacity | Masks | Mask IDs | Inverted | Visible |\n",
    );
    output.push_str("| ---: | --- | ---: | ---: | ---: | --- | --- | --- |\n");
    for drawable in probe.lines().filter_map(parse_drawable_line) {
        if drawable.kind != DrawableLineKind::Eye {
            continue;
        }
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | `{}` | {} | {} |\n",
            drawable.index,
            escape_markdown_table_cell(&drawable.part),
            drawable.render_order,
            drawable.opacity,
            drawable.masks,
            escape_markdown_table_cell(drawable.mask_ids.as_deref().unwrap_or("[]")),
            drawable.inverted.as_deref().unwrap_or("-"),
            drawable.visible.as_deref().unwrap_or("-")
        ));
    }
    output
}

fn capture_references(
    root: &Path,
    output_dir: &Path,
    matrix_dir: &str,
    model_name: &str,
) -> String {
    let mut output = String::new();
    output.push_str("| Mode | Screenshot | Log |\n");
    output.push_str("| --- | --- | --- |\n");
    let log_path = output_dir.join(matrix_dir).join("capture.log");
    for mode in ["shared", "high-precision", "no-mask"] {
        let image = output_dir
            .join(matrix_dir)
            .join(format!("latest-{model_name}-{mode}.png"));
        if image.is_file() {
            output.push_str(&format!(
                "| {mode} | `{}` | `{}` |\n",
                relative_path(root, &image),
                relative_path(root, &log_path)
            ));
        } else {
            output.push_str(&format!(
                "| {mode} | _Missing_ | `{}` |\n",
                relative_path(root, &log_path)
            ));
        }
    }
    output
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DrawableLineKind {
    Drawable,
    Eye,
    Inverted,
    Sampled,
}

#[derive(Debug)]
struct DrawableLine {
    kind: DrawableLineKind,
    index: String,
    part: String,
    render_order: String,
    blend: String,
    opacity: String,
    masks: u32,
    mask_ids: Option<String>,
    inverted: Option<String>,
    sample_alpha: Option<String>,
    visible: Option<String>,
}

#[derive(Debug)]
struct OffscreenLine {
    index: String,
    owner: String,
    depth: u32,
    render_order: String,
    blend: String,
    opacity: String,
    masks: u32,
    inverted_mask: String,
}

fn parse_offscreen_line(line: &str) -> Option<OffscreenLine> {
    let rest = line.strip_prefix("  offscreen #")?;
    let (index, rest) = rest.split_once(" owner ")?;
    let (owner, rest) = rest.split_once(" depth ")?;
    let (depth, rest) = rest.split_once(" render ")?;
    let (render_order, rest) = rest.split_once(" blend ")?;
    let (blend, rest) = rest.split_once(" opacity ")?;
    let (opacity, rest) = rest.split_once(" multiply ")?;
    let masks = value_after(rest, " masks ")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let inverted_mask = value_after(rest, " inverted_mask=")
        .unwrap_or("-")
        .to_string();

    Some(OffscreenLine {
        index: index.to_string(),
        owner: owner.to_string(),
        depth: depth.parse().ok()?,
        render_order: render_order.to_string(),
        blend: blend.to_string(),
        opacity: opacity.to_string(),
        masks,
        inverted_mask,
    })
}

fn parse_drawable_line(line: &str) -> Option<DrawableLine> {
    let (kind, rest) = if let Some(rest) = line.strip_prefix("  eye drawable #") {
        (DrawableLineKind::Eye, rest)
    } else if let Some(rest) = line.strip_prefix("  inverted drawable #") {
        (DrawableLineKind::Inverted, rest)
    } else if let Some(rest) = line.strip_prefix("  sampled drawable #") {
        (DrawableLineKind::Sampled, rest)
    } else if let Some(rest) = line.strip_prefix("  drawable #") {
        (DrawableLineKind::Drawable, rest)
    } else {
        return None;
    };

    let (head, rest) = rest.split_once(" part ")?;
    let index = head.split_whitespace().next()?.to_string();
    let (part, rest) = rest.split_once(" render ")?;
    let (render_order, rest) = rest.split_once(" blend ")?;
    let (blend, tail) = rest
        .split_once(" tex ")
        .or_else(|| rest.split_once(" opacity "))?;
    let opacity = value_after(tail, " opacity ")
        .or_else(|| first_token(tail))
        .unwrap_or("-")
        .to_string();
    let masks = value_after(tail, " masks ")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mask_ids = value_between(tail, " mask_ids ", " inverted_mask=")
        .or_else(|| value_between(tail, " mask_ids ", " visible="))
        .or_else(|| value_after(tail, " mask_ids ").map(str::to_string));
    let inverted = value_between(tail, " inverted_mask=", " visible=");
    let sample_alpha = sample_alpha_from_line(line);
    let visible = value_after(tail, " visible=").map(str::to_string);

    Some(DrawableLine {
        kind,
        index,
        part: part.to_string(),
        render_order: render_order.to_string(),
        blend: blend.to_string(),
        opacity,
        masks,
        mask_ids,
        inverted,
        sample_alpha,
        visible,
    })
}

fn sample_alpha_from_line(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("avg rgba [")?;
    let (rgba, _) = rest.split_once(']')?;
    rgba.split(", ").nth(3).map(str::to_string)
}

fn value_after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    text.split_once(marker)
        .and_then(|(_, rest)| first_token(rest))
}

fn value_between(text: &str, start: &str, end: &str) -> Option<String> {
    let (_, rest) = text.split_once(start)?;
    let (value, _) = rest.split_once(end)?;
    Some(value.trim().to_string())
}

fn first_token(text: &str) -> Option<&str> {
    text.split_whitespace().next()
}

fn escape_markdown_table_cell(value: &str) -> String {
    value.replace('|', "\\|")
}

#[derive(Debug)]
struct ProbeModel {
    path: String,
    status: String,
    masks: u32,
    max_mask: u32,
    extended_blends: u32,
    offscreens: u32,
    reasons: String,
}

fn parse_probe_models(probe: &str) -> Vec<ProbeModel> {
    let mut models = Vec::new();
    let mut current: Option<ProbeModel> = None;

    for line in probe.lines() {
        if let Some(model) = parse_probe_model_line(line) {
            if let Some(model) = current.replace(model) {
                models.push(model);
            }
            continue;
        }

        if let Some(reason) = line.strip_prefix("  risk ") {
            if let Some(model) = current.as_mut() {
                if model.reasons == "No specific risk lines." {
                    model.reasons.clear();
                }
                if !model.reasons.is_empty() {
                    model.reasons.push_str("<br>");
                }
                model.reasons.push_str(&reason.replace('|', "\\|"));
            }
        }
    }

    if let Some(model) = current {
        models.push(model);
    }

    models.sort_by(|left, right| {
        risk_rank(&right.status)
            .cmp(&risk_rank(&left.status))
            .then_with(|| right.offscreens.cmp(&left.offscreens))
            .then_with(|| right.extended_blends.cmp(&left.extended_blends))
            .then_with(|| right.max_mask.cmp(&left.max_mask))
            .then_with(|| right.masks.cmp(&left.masks))
    });
    models
}

fn parse_probe_model_line(line: &str) -> Option<ProbeModel> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 12 || !fields.first()?.ends_with(".model3.json") {
        return None;
    }

    Some(ProbeModel {
        path: fields[0].to_string(),
        status: fields.last()?.to_string(),
        masks: fields.get(4)?.parse().ok()?,
        max_mask: fields.get(5)?.parse().ok()?,
        extended_blends: fields.get(8)?.parse().ok()?,
        offscreens: fields.get(10)?.parse().ok()?,
        reasons: "No specific risk lines.".to_string(),
    })
}

fn risk_rank(value: &str) -> u8 {
    match value {
        "risk:high" => 3,
        "risk:medium" => 2,
        "risk:low" => 1,
        _ => 0,
    }
}

fn first_lines(text: &str, limit: usize) -> String {
    let mut output = String::new();
    for line in text.lines().take(limit) {
        output.push_str(line);
        output.push('\n');
    }
    output
}

fn project_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask manifest directory has no parent".into())
}

fn remove_path(path: PathBuf) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn copy_dir_replace(source: &Path, target: &Path) -> Result<()> {
    remove_path(target.to_path_buf())?;
    copy_dir_recursive(source, target)
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else {
            fs::copy(&source_path, &target_path)?;
        }
    }
    Ok(())
}

fn terminate_app_processes(root: &Path) {
    let binary = root.join("target/debug/vtube-studio-rs");
    let canonical_binary = binary.canonicalize().unwrap_or_else(|_| binary.clone());
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);

    for process in system.processes().values() {
        if process_matches_binary(process, &binary, &canonical_binary) {
            if process.kill_with(Signal::Term).unwrap_or(false) {
                continue;
            }
            let _ = process.kill();
        }
    }
}

fn process_matches_binary(process: &Process, binary: &Path, canonical_binary: &Path) -> bool {
    if process
        .exe()
        .is_some_and(|path| path_matches_binary(path, binary, canonical_binary))
    {
        return true;
    }

    if process.name() == "vtube-studio-rs" {
        return true;
    }

    let binary_text = binary.to_string_lossy();
    process.cmd().iter().any(|argument| {
        path_matches_binary(Path::new(argument), binary, canonical_binary)
            || argument.to_string_lossy().contains(binary_text.as_ref())
    })
}

fn path_matches_binary(path: &Path, binary: &Path, canonical_binary: &Path) -> bool {
    path == binary
        || path == canonical_binary
        || path
            .canonicalize()
            .is_ok_and(|canonical_path| canonical_path == canonical_binary)
}

fn run_status(command: &mut Command) -> Result<()> {
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command failed with status {status}").into())
    }
}

fn host_arch_lib_dir() -> Result<&'static str> {
    match env::consts::ARCH {
        "aarch64" => Ok("arm64"),
        "x86_64" => Ok("x86_64"),
        arch => Err(format!("unsupported macOS architecture: {arch}").into()),
    }
}

fn require_file(path: &Path, message: &str) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("{message} at: {}", path.display()).into())
    }
}

fn latest_pngs(directory: &Path) -> Vec<PathBuf> {
    let mut images = Vec::new();
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if path.is_file() && name.starts_with("latest-") && name.ends_with(".png") {
                images.push(path);
            }
        }
    }
    images.sort();
    images
}

fn latest_png_names(directory: &Path) -> Vec<String> {
    latest_pngs(directory)
        .into_iter()
        .filter_map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .collect()
}

fn capture_logs(output_dir: &Path) -> Vec<PathBuf> {
    let mut logs = Vec::new();
    collect_capture_logs(output_dir, 0, &mut logs);
    logs.sort();
    logs
}

fn collect_capture_logs(directory: &Path, depth: usize, logs: &mut Vec<PathBuf>) {
    if depth > 2 {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_capture_logs(&path, depth + 1, logs);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("capture.log") {
            logs.push(path);
        }
    }
}

fn fallback_event_lines(root: &Path, output_dir: &Path, include_log: bool) -> Vec<String> {
    let mut events = Vec::new();
    for log_path in capture_logs(output_dir) {
        for line in file_lines_containing(&log_path, "renderer_event=high_precision_mask_fallback")
        {
            if include_log {
                events.push(format!("{}: {line}", relative_path(root, &log_path)));
            } else {
                events.push(escape_markdown_table_cell(&line));
            }
        }
    }
    events
}

fn file_lines_containing(path: &Path, needle: &str) -> Vec<String> {
    fs::read_to_string(path)
        .map(|content| {
            content
                .lines()
                .filter(|line| line.contains(needle))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn tail_lines_containing(path: &Path, needle: &str, count: usize) -> Vec<String> {
    let mut lines = file_lines_containing(path, needle);
    let drain_count = lines.len().saturating_sub(count);
    lines.drain(0..drain_count);
    lines
}

fn content_until(content: &str, heading: &str) -> String {
    let mut output = String::new();
    for line in content.lines() {
        if line == heading {
            break;
        }
        output.push_str(line);
        output.push('\n');
    }
    output
}

fn content_from<'a>(content: &'a str, heading: &str) -> Option<&'a str> {
    content.find(heading).map(|index| &content[index..])
}

fn report_image_path(root: &Path, output_dir: &Path, path: &Path) -> String {
    path.strip_prefix(output_dir)
        .or_else(|_| path.strip_prefix(root))
        .unwrap_or(path)
        .display()
        .to_string()
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn generated_stamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => format!("unix:{}", duration.as_secs()),
        Err(_) => "unix:0".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_sorts_probe_models_by_risk_shape() {
        let probe = "\
model                                                                     params parts  draw  mask maxMask  add mult  ext  inv     off status
public/Haru/Haru.model3.json                                                  42    19    84    10       1    0    0    0    0       0 ok risk:medium
public/Ren/Ren.model3.json                                                    78    30   216     4       3    0    0   29    3      24 ok risk:high
  risk offscreen objects: 24 require render-target compositing
  risk extended blend objects: 29 use snapshot compositing
public/Mao/Mao.model3.json                                                    72    22   162    37      12   15    8    0   10       0 ok risk:high
  risk dense clipping: max 12 masks in one drawable/offscreen
";

        let models = parse_probe_models(probe);

        assert_eq!(models.len(), 3);
        assert_eq!(models[0].path, "public/Ren/Ren.model3.json");
        assert_eq!(models[1].path, "public/Mao/Mao.model3.json");
        assert_eq!(models[2].path, "public/Haru/Haru.model3.json");
        assert_eq!(models[2].reasons, "No specific risk lines.");
    }

    #[test]
    fn compatibility_recommendations_count_risk_lines() {
        let probe = "\
public/Ren/Ren.model3.json 78 30 216 4 3 0 0 29 3 24 ok risk:high
  risk offscreen objects: 24 require render-target compositing
  risk extended blend objects: 29 use snapshot compositing
public/Mao/Mao.model3.json 72 22 162 37 12 15 8 0 10 0 ok risk:high
  risk dense clipping: max 12 masks in one drawable/offscreen
";

        let recommendations = compatibility_recommendations(probe);

        assert!(recommendations.contains("High-risk models found: 2."));
        assert!(recommendations.contains("Models with offscreen objects: 1."));
        assert!(recommendations.contains("Models with extended blend objects: 1."));
        assert!(recommendations.contains("Models with dense clipping: 1."));
    }

    #[test]
    fn parses_probe_drawable_lines_for_mask_audit_tables() {
        let eye = parse_drawable_line(
            "  eye drawable #213 ArtMesh287 part PartEye render 123 blend Normal tex 0 opacity 0.300 masks 1 mask_ids [212] inverted_mask=true visible=true",
        )
        .expect("eye drawable should parse");
        assert_eq!(eye.kind, DrawableLineKind::Eye);
        assert_eq!(eye.index, "213");
        assert_eq!(eye.part, "PartEye");
        assert_eq!(eye.render_order, "123");
        assert_eq!(eye.opacity, "0.300");
        assert_eq!(eye.masks, 1);
        assert_eq!(eye.mask_ids.as_deref(), Some("[212]"));
        assert_eq!(eye.inverted.as_deref(), Some("true"));
        assert_eq!(eye.visible.as_deref(), Some("true"));

        let inverted = parse_drawable_line(
            "  inverted drawable #25 ArtMesh160 part PartWandB render 150 blend Normal tex 0 opacity 1.000 masks 1 mask_ids [24] visible=true",
        )
        .expect("inverted drawable should parse");
        assert_eq!(inverted.kind, DrawableLineKind::Inverted);
        assert_eq!(inverted.blend, "Normal");
        assert_eq!(inverted.mask_ids.as_deref(), Some("[24]"));
        assert_eq!(inverted.visible.as_deref(), Some("true"));

        let drawable = parse_drawable_line(
            "  drawable #51 ArtMesh202 part PartRobe render 57 blend Multiplicative opacity 1.000 multiply [1.0, 1.0, 1.0, 1.0] screen [0.0, 0.0, 0.0, 1.0] masks 11",
        )
        .expect("drawable should parse");
        assert_eq!(drawable.kind, DrawableLineKind::Drawable);
        assert_eq!(drawable.blend, "Multiplicative");
        assert_eq!(drawable.masks, 11);
    }

    #[test]
    fn mao_mask_tables_include_expected_rows() {
        let probe = concat!(
            "  risk dense clipping: max 12 masks in one drawable/offscreen\n",
            "  eye drawable #213 ArtMesh287 part PartEye render 123 blend Normal tex 0 opacity 0.300 masks 1 mask_ids [212] inverted_mask=true visible=true\n",
            "  inverted drawable #25 ArtMesh160 part PartWandB render 150 blend Normal tex 0 opacity 1.000 masks 1 mask_ids [24] visible=true\n",
            "  drawable #51 ArtMesh202 part PartRobe render 57 blend Multiplicative opacity 1.000 multiply [1.0, 1.0, 1.0, 1.0] screen [0.0, 0.0, 0.0, 1.0] masks 11\n",
        );

        assert!(risk_lines_as_markdown(probe).contains("- dense clipping"));
        assert!(
            masked_drawable_table(probe)
                .contains("| 51 | `PartRobe` | 57 | Multiplicative | 1.000 | 11 |")
        );
        assert!(
            inverted_mask_table(probe).contains("| 25 | `PartWandB` | 150 | 1.000 | 1 | true |")
        );
        assert!(
            eye_mask_table(probe)
                .contains("| 213 | `PartEye` | 123 | 0.300 | 1 | `[212]` | true | true |")
        );
    }

    #[test]
    fn parses_offscreen_lines_for_ren_audit_tables() {
        let offscreen = parse_offscreen_line(
            "  offscreen #14 owner PartHairShadowOut(38) depth 3 render 154 blend Extended(raw 512, color Normal, alpha Out) opacity 1.000 multiply [1.0, 1.0, 1.0, 1.0] screen [0.0, 0.0, 0.0, 1.0] masks 2 inverted_mask=true",
        )
        .expect("offscreen should parse");

        assert_eq!(offscreen.index, "14");
        assert_eq!(offscreen.owner, "PartHairShadowOut(38)");
        assert_eq!(offscreen.depth, 3);
        assert_eq!(offscreen.render_order, "154");
        assert_eq!(
            offscreen.blend,
            "Extended(raw 512, color Normal, alpha Out)"
        );
        assert_eq!(offscreen.opacity, "1.000");
        assert_eq!(offscreen.masks, 2);
        assert_eq!(offscreen.inverted_mask, "true");
    }

    #[test]
    fn offscreen_plan_checks_detect_required_snapshots_and_flushes() {
        let probe = concat!(
            "  offscreen_plan begin offscreen #0 owner PartAll(0) depth 1 render 0 blend Normal opacity 1.000 masks 0\n",
            "  offscreen_plan begin offscreen #14 owner PartHairShadowOut(38) depth 3 render 154 blend Extended(raw 512, color Normal, alpha Out) opacity 1.000 masks 1\n",
            "  offscreen_plan snapshot target #14 reason extended_drawable #193 render 157 blend Extended(raw 512, color Normal, alpha Out)\n",
            "  offscreen_plan draw drawable #193 part PartHairShadowOut target #14 render 157 blend Extended(raw 512, color Normal, alpha Out) opacity 1.000 masks 0 visible=true\n",
            "  offscreen_plan snapshot target #0 reason extended_offscreen #14 render 154 blend Extended(raw 512, color Normal, alpha Out)\n",
            "  offscreen_plan flush offscreen #14 parent #0 reason drawable_left_owner_part render 154 blend Extended(raw 512, color Normal, alpha Out) opacity 1.000 masks 1\n",
            "  offscreen_plan flush offscreen #0 parent main reason end_of_render_order render 0 blend Normal opacity 1.000 masks 0\n",
        );

        let checks = offscreen_plan_checks(probe);

        assert!(checks.contains("[x] Every begun offscreen has a matching flush."));
        assert!(
            checks.contains("[x] Visible nonzero extended drawables have a snapshot before draw.")
        );
        assert!(checks.contains("[x] Nonzero extended offscreens have a snapshot before flush."));
        assert!(checks.contains("[x] Masked offscreens are flushed through the compositor path."));
    }

    #[test]
    fn offscreen_plan_checks_flag_missing_extended_snapshot() {
        let probe = concat!(
            "  offscreen_plan begin offscreen #0 owner PartAll(0) depth 1 render 0 blend Normal opacity 1.000 masks 0\n",
            "  offscreen_plan draw drawable #193 part PartHairShadowOut target #0 render 157 blend Extended(raw 512, color Normal, alpha Out) opacity 1.000 masks 0 visible=true\n",
            "  offscreen_plan flush offscreen #0 parent main reason end_of_render_order render 0 blend Normal opacity 1.000 masks 0\n",
        );

        let checks = offscreen_plan_checks(probe);

        assert!(checks.contains("missing a snapshot"));
    }

    #[test]
    fn parses_sampled_drawable_alpha_for_rice_audit() {
        let drawable = parse_drawable_line(
            "  sampled drawable #1 ArtMesh32 part PartRibbon render 7 blend Normal tex 0 opacity 1.000 avg rgba [0.10, 0.20, 0.30, 0.42] masks 0",
        )
        .expect("sampled drawable should parse");

        assert_eq!(drawable.kind, DrawableLineKind::Sampled);
        assert_eq!(drawable.index, "1");
        assert_eq!(drawable.part, "PartRibbon");
        assert_eq!(drawable.sample_alpha.as_deref(), Some("0.42"));
    }

    #[test]
    fn rice_audit_tables_include_additive_and_translucent_rows() {
        let probe = concat!(
            "  risk inverted masks: 7 object(s)\n",
            "  drawable #12 ArtMeshGlow part PartEffect render 30 blend Additive opacity 0.800 multiply [1.0, 1.0, 1.0, 1.0] screen [0.0, 0.0, 0.0, 1.0] masks 2\n",
            "  sampled drawable #1 ArtMesh32 part PartRibbon render 7 blend Normal tex 0 opacity 1.000 avg rgba [0.10, 0.20, 0.30, 0.42] masks 0\n",
        );

        assert!(additive_drawable_table(probe).contains("| 12 | `PartEffect` | 30 | 0.800 | 2 |"));
        assert!(inverted_mask_summary(probe).contains("- inverted masks: 7 object(s)"));
        assert!(
            translucent_drawable_table(probe)
                .contains("| 1 | `PartRibbon` | 7 | Normal | 1.000 | 0.42 | 0 |")
        );
    }

    #[test]
    fn quality_crop_matches_percentage_geometry() {
        assert_eq!(
            crop_rect(1000, 800, 38, 20, 24, 22),
            CropRect {
                x: 380,
                y: 160,
                width: 240,
                height: 176,
            }
        );
        assert_eq!(
            format_crop_rect(crop_rect(10, 10, 95, 95, 20, 20)),
            "2x2+8+8"
        );
    }

    #[test]
    fn quality_diff_metrics_count_threshold_pixels() {
        let left = RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 255]));
        let mut right = RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 255]));
        right.put_pixel(1, 0, Rgba([30, 30, 30, 255]));

        let diff = diff_image(&left, &right);
        let metrics = diff_metrics(&diff, None);

        assert_eq!(diff.get_pixel(1, 0).0, [30, 30, 30, 255]);
        assert!(metrics.mean > 0.05 && metrics.mean < 0.07);
        assert!(metrics.max > 0.11 && metrics.max < 0.12);
        assert_eq!(metrics.changed_soft_percent, 50.0);
        assert_eq!(metrics.changed_strong_percent, 50.0);
    }

    #[test]
    fn quality_heat_image_auto_levels_nonzero_diff() {
        let mut diff = RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 255]));
        diff.put_pixel(1, 0, Rgba([20, 20, 20, 255]));

        let heat = heat_image(&diff);

        assert_eq!(heat.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(heat.get_pixel(1, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn review_focus_links_anisotropy_quality_captures_when_present() {
        let root = env::temp_dir().join(format!(
            "vtube-studio-rs-xtask-test-{}",
            timestamp_for_filename()
        ));
        let output_dir = root.join("target/render-regression");
        let quality_dir = output_dir.join("quality-matrix");
        fs::create_dir_all(&quality_dir).expect("quality dir should be created");
        fs::write(quality_dir.join("latest-0-mipmaps-on.png"), [])
            .expect("mipmaps-on capture marker should be written");
        fs::write(quality_dir.join("latest-0-mipmaps-on-aniso8.png"), [])
            .expect("anisotropy capture marker should be written");

        let focus = review_focus(&root, &output_dir);

        assert!(focus.contains("Texture sampling"));
        assert!(focus.contains("Texture anisotropy"));
        assert!(focus.contains("latest-0-mipmaps-on-aniso8.png"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn msaa_summary_reports_sample_count_and_resize_events() {
        let root = env::temp_dir().join(format!(
            "vtube-studio-rs-xtask-msaa-test-{}",
            timestamp_for_filename()
        ));
        let output_dir = root.join("target/render-regression");
        let capture_dir = output_dir.join("quality-matrix");
        fs::create_dir_all(&capture_dir).expect("capture dir should be created");
        fs::write(
            capture_dir.join("capture.log"),
            concat!(
                "renderer_event=metal_initialized device=\"Apple\" textures=2 sample_count=4 masks_disabled=false\n",
                "renderer_event=msaa_texture_resized width=720 height=960 sample_count=4\n",
            ),
        )
        .expect("capture log should be written");

        let summary = msaa_summary(&root, &output_dir);
        let overview = msaa_overview(&output_dir).expect("msaa overview should be present");

        assert_eq!(overview.max_sample_count, 4);
        assert_eq!(overview.initialized_logs, 1);
        assert_eq!(overview.resize_events, 1);
        assert!(summary.contains("MSAA active"));
        assert!(summary.contains("| 4 | 1 |"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn retina_resize_summary_reports_geometry_and_texture_events() {
        let root = env::temp_dir().join(format!(
            "vtube-studio-rs-xtask-retina-test-{}",
            timestamp_for_filename()
        ));
        let output_dir = root.join("target/render-regression");
        let capture_dir = output_dir.join("mask-matrix");
        fs::create_dir_all(&capture_dir).expect("capture dir should be created");
        fs::write(
            capture_dir.join("capture.log"),
            concat!(
                "renderer_event=contents_scale_changed old=1.00 new=2.00\n",
                "renderer_event=drawable_size_changed logical=512.0 physical=1024 contents_scale=2.00\n",
                "renderer_event=mask_tile_size_changed old=512 new=1024 physical=1024\n",
                "renderer_event=mask_atlas_resized contexts=37 textures=2 texture_size=1024\n",
                "renderer_event=offscreen_texture_size_changed old=0x0 new=1024x1024 count=2\n",
            ),
        )
        .expect("capture log should be written");

        let summary = retina_resize_summary(&root, &output_dir);
        let overview =
            retina_resize_overview(&output_dir).expect("resize overview should be present");
        let focus = review_focus(&root, &output_dir);

        assert_eq!(overview.contents_scale_events, 1);
        assert_eq!(overview.drawable_size_events, 1);
        assert_eq!(overview.mask_texture_events, 2);
        assert_eq!(overview.offscreen_texture_events, 1);
        assert_eq!(overview.max_physical_size, 1024);
        assert_eq!(overview.max_mask_texture_size, 1024);
        assert!(summary.contains("Retina / Resize Stability"));
        assert!(summary.contains("Resize touched mask/offscreen textures"));
        assert!(focus.contains("Retina / window resize stability"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ren_visual_diff_rows_match_shell_geometry() {
        let rows = ren_visual_diff_rows(1000, 800);

        assert_eq!(rows[0], ("whole image", None));
        assert_eq!(
            rows[4],
            (
                "pupil offscreens",
                Some(CropRect {
                    x: 400,
                    y: 192,
                    width: 200,
                    height: 144,
                })
            )
        );
    }

    #[test]
    fn ren_automatic_checks_detect_mask_difference() {
        let shared_high = RgbaImage::from_pixel(100, 100, Rgba([0, 0, 0, 255]));
        let mut shared_no_mask = RgbaImage::from_pixel(100, 100, Rgba([0, 0, 0, 255]));
        for y in 24..30 {
            for x in 40..46 {
                shared_no_mask.put_pixel(x, y, Rgba([80, 80, 80, 255]));
            }
        }

        let checks = ren_automatic_diff_checks(100, 100, &shared_high, &shared_no_mask);

        assert!(checks.contains("[x] Shared and high-precision fallback match"));
        assert!(checks.contains("[x] Shared vs no-mask shows measurable"));
    }

    #[test]
    fn set_toml_value_replaces_existing_key_and_preserves_indent() {
        let content = "[renderer]\n  disable_masks = false\nother = true\n";

        let updated = set_toml_value(content, "disable_masks", "true");

        assert!(updated.contains("  disable_masks = true\n"));
        assert!(updated.contains("other = true\n"));
    }

    #[test]
    fn set_toml_value_appends_missing_key() {
        let updated = set_toml_value("[renderer]\n", "atlas_mipmaps", "true");

        assert!(updated.ends_with("atlas_mipmaps = true\n"));
    }

    #[test]
    fn set_toml_section_value_replaces_key_only_in_target_section() {
        let content = "[other]\npath = \"keep\"\n\n[model]\n  path = \"old\"\n[renderer]\n";

        let updated = set_toml_section_value(content, "model", "path", "\"new\"");

        assert!(updated.contains("[other]\npath = \"keep\"\n"));
        assert!(updated.contains("[model]\n  path = \"new\"\n[renderer]\n"));
    }

    #[test]
    fn set_toml_section_value_inserts_missing_key_before_next_section() {
        let content = "[model]\n# no path yet\n\n[renderer]\ndisable_masks = false\n";

        let updated =
            set_toml_section_value(content, "model", "path", "\"public/model/0.model3.json\"");

        assert!(updated.contains(
            "[model]\n# no path yet\n\npath = \"public/model/0.model3.json\"\n[renderer]\n"
        ));
    }

    #[test]
    fn set_toml_section_value_appends_missing_section() {
        let updated =
            set_toml_section_value("[renderer]\n", "model", "path", "\"avatar.model3.json\"");

        assert!(updated.ends_with("\n[model]\npath = \"avatar.model3.json\"\n"));
    }

    #[test]
    fn set_toml_section_values_updates_multiple_keys_in_target_section() {
        let updated = set_toml_section_values(
            "[input.camera]\nenabled = false\npose_mode = \"mouse\"\n",
            "input.camera",
            &[
                ("enabled", "true".to_string()),
                ("pose_mode", "\"camera_when_available\"".to_string()),
                ("mouth_gain", "1.4".to_string()),
            ],
        );

        assert!(updated.contains("[input.camera]\nenabled = true\n"));
        assert!(updated.contains("pose_mode = \"camera_when_available\"\n"));
        assert!(updated.contains("mouth_gain = 1.4\n"));
    }

    #[test]
    fn remove_toml_section_removes_legacy_output_block() {
        let updated = remove_toml_section(
            r#"[app]
window_level = "screen_saver"

[output]
mode = "syphon"
syphon_name = "VTubeStudioRS"

[output.internal]
width = 1080.0

[renderer]
atlas_anisotropy = 1
"#,
            "output",
        );

        assert!(updated.contains("[app]\n"));
        assert!(updated.contains("[renderer]\n"));
        assert!(!updated.contains("[output]"));
        assert!(!updated.contains("[output.internal]"));
        assert!(!updated.contains("syphon_name"));
    }

    #[test]
    fn select_model_args_choose_dev_or_build_config() {
        let (target, model_path) =
            parse_select_model_args(vec!["public/model/0.model3.json".to_string()])
                .expect("default target should parse");
        assert!(matches!(target, SelectModelTarget::Development));
        assert_eq!(model_path, "public/model/0.model3.json");

        let (target, model_path) = parse_select_model_args(vec![
            "--build".to_string(),
            "public/model/0.model3.json".to_string(),
        ])
        .expect("build target should parse");
        assert!(matches!(target, SelectModelTarget::Build));
        assert_eq!(model_path, "public/model/0.model3.json");
    }

    #[test]
    fn obs_recording_args_default_to_build_config() {
        let (target, placement) =
            parse_obs_recording_args(Vec::new()).expect("default obs target should parse");
        assert!(matches!(target, SelectModelTarget::Build));
        assert!(matches!(placement, ObsWindowPlacement::Desktop));

        let (target, placement) = parse_obs_recording_args(vec!["--dev".to_string()])
            .expect("dev obs target should parse");
        assert!(matches!(target, SelectModelTarget::Development));
        assert!(matches!(placement, ObsWindowPlacement::Desktop));

        let (target, placement) =
            parse_obs_recording_args(vec!["--build".to_string(), "--offscreen".to_string()])
                .expect("build obs target should parse");
        assert!(matches!(target, SelectModelTarget::Build));
        assert!(matches!(placement, ObsWindowPlacement::Offscreen));

        assert!(parse_obs_recording_args(vec!["--release".to_string()]).is_err());
    }

    #[test]
    fn internal_output_args_default_to_build_config() {
        let target = parse_internal_output_args(Vec::new())
            .expect("default internal output target should parse");
        assert!(matches!(target, SelectModelTarget::Build));

        let target = parse_internal_output_args(vec!["--dev".to_string()])
            .expect("dev internal output target should parse");
        assert!(matches!(target, SelectModelTarget::Development));

        let target = parse_internal_output_args(vec!["--build".to_string()])
            .expect("build internal output target should parse");
        assert!(matches!(target, SelectModelTarget::Build));

        assert!(parse_internal_output_args(vec!["--release".to_string()]).is_err());
    }

    #[test]
    fn virtual_camera_readiness_args_default_to_build_config() {
        let target = parse_virtual_camera_readiness_args(Vec::new())
            .expect("default virtual camera target should parse");
        assert!(matches!(target, SelectModelTarget::Build));

        let target = parse_virtual_camera_readiness_args(vec!["--dev".to_string()])
            .expect("dev virtual camera target should parse");
        assert!(matches!(target, SelectModelTarget::Development));

        let target = parse_virtual_camera_readiness_args(vec!["--build".to_string()])
            .expect("build virtual camera target should parse");
        assert!(matches!(target, SelectModelTarget::Build));

        assert!(parse_virtual_camera_readiness_args(vec!["--release".to_string()]).is_err());
    }

    #[test]
    fn camera_extension_plan_args_default_to_build_config() {
        let target =
            parse_camera_extension_plan_args(Vec::new()).expect("default plan target should parse");
        assert!(matches!(target, SelectModelTarget::Build));

        let target = parse_camera_extension_plan_args(vec!["--dev".to_string()])
            .expect("dev plan target should parse");
        assert!(matches!(target, SelectModelTarget::Development));

        let target = parse_camera_extension_plan_args(vec!["--build".to_string()])
            .expect("build plan target should parse");
        assert!(matches!(target, SelectModelTarget::Build));

        assert!(parse_camera_extension_plan_args(vec!["--release".to_string()]).is_err());
    }

    #[test]
    fn provision_camera_profiles_args_parse_source_and_force() {
        let options = parse_provision_camera_profiles_args(vec![
            "--from".to_string(),
            "/tmp/profiles".to_string(),
            "--force".to_string(),
        ])
        .expect("provision args should parse");
        assert_eq!(options.source_dirs, vec![PathBuf::from("/tmp/profiles")]);
        assert!(options.force);

        let options = parse_provision_camera_profiles_args(vec![
            "--from".to_string(),
            "/tmp/one".to_string(),
            "--from".to_string(),
            "/tmp/two".to_string(),
        ])
        .expect("multiple source dirs should parse");
        assert_eq!(
            options.source_dirs,
            vec![PathBuf::from("/tmp/one"), PathBuf::from("/tmp/two")]
        );

        let options =
            parse_provision_camera_profiles_args(Vec::new()).expect("default args should parse");
        assert!(options.source_dirs.len() >= 2);
        assert!(parse_provision_camera_profiles_args(vec!["--from".to_string()]).is_err());
        assert!(parse_provision_camera_profiles_args(vec!["--bad".to_string()]).is_err());
    }

    #[test]
    fn collect_provisioning_profile_paths_accepts_expected_extensions() {
        let root = env::temp_dir().join(format!(
            "vtube-studio-rs-profile-scan-test-{}",
            timestamp_for_filename()
        ));
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("test directories should be created");
        fs::write(root.join("one.provisionprofile"), "").expect("profile should be written");
        fs::write(nested.join("two.mobileprovision"), "").expect("profile should be written");
        fs::write(root.join("ignore.txt"), "").expect("ignored file should be written");

        let mut paths = Vec::new();
        collect_provisioning_profile_paths(&root, &mut paths)
            .expect("profile paths should collect");
        paths.sort();
        assert_eq!(paths.len(), 2);
        assert!(
            paths
                .iter()
                .any(|path| path.ends_with("one.provisionprofile"))
        );
        assert!(
            paths
                .iter()
                .any(|path| path.ends_with("two.mobileprovision"))
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn camera_extension_templates_include_coremediaio_identifiers() {
        let root = env::temp_dir().join(format!(
            "vtube-studio-rs-camera-extension-plan-test-{}",
            timestamp_for_filename()
        ));
        let plan = camera_extension_plan_markdown(SelectModelTarget::Build, &root);
        assert!(plan.contains("objc2-core-media-io"));
        assert!(plan.contains("VTube Studio RS Camera"));
        assert!(plan.contains("CMIOExtensionStreamSource"));

        let info = camera_extension_info_plist();
        assert!(info.contains("CMIOExtensionMachServiceName"));
        assert!(info.contains(VIRTUAL_CAMERA_EXTENSION_BUNDLE_ID));

        let entitlements = camera_container_app_entitlements();
        assert!(entitlements.contains("com.apple.developer.system-extension.install"));
        assert!(entitlements.contains(VIRTUAL_CAMERA_APP_GROUP));
    }

    #[test]
    fn validates_container_provisioning_profile_contract() {
        let value = serde_json::json!({
            "Name": "VTube Studio RS Container",
            "TeamIdentifier": ["TEAM123456"],
            "Entitlements": {
                "application-identifier": "TEAM123456.rs.vtube-studio.dev",
                "com.apple.developer.system-extension.install": true,
                "com.apple.security.application-groups": ["group.rs.vtube-studio.dev"]
            }
        });
        let summary =
            provisioning_profile_summary_from_json(&value).expect("profile summary should parse");
        validate_provisioning_profile_summary(&summary, ProvisioningProfileKind::ContainerApp)
            .expect("container profile should validate");
    }

    #[test]
    fn rejects_container_profile_without_system_extension_entitlement() {
        let value = serde_json::json!({
            "Name": "VTube Studio RS Container",
            "TeamIdentifier": ["TEAM123456"],
            "Entitlements": {
                "application-identifier": "TEAM123456.rs.vtube-studio.dev",
                "com.apple.security.application-groups": ["group.rs.vtube-studio.dev"]
            }
        });
        let summary =
            provisioning_profile_summary_from_json(&value).expect("profile summary should parse");
        let error =
            validate_provisioning_profile_summary(&summary, ProvisioningProfileKind::ContainerApp)
                .expect_err("missing system extension entitlement should be rejected");
        assert!(
            error
                .to_string()
                .contains("com.apple.developer.system-extension.install")
        );
    }

    #[test]
    fn validates_camera_extension_provisioning_profile_contract() {
        let value = serde_json::json!({
            "Name": "VTube Studio RS Camera Extension",
            "TeamIdentifier": ["TEAM123456"],
            "Entitlements": {
                "application-identifier": "TEAM123456.rs.vtube-studio.dev.CameraExtension",
                "com.apple.security.application-groups": ["group.rs.vtube-studio.dev"]
            }
        });
        let summary =
            provisioning_profile_summary_from_json(&value).expect("profile summary should parse");
        validate_provisioning_profile_summary(&summary, ProvisioningProfileKind::CameraExtension)
            .expect("extension profile should validate");
    }

    #[test]
    fn rejects_provisioning_profile_for_wrong_bundle_id() {
        let value = serde_json::json!({
            "Name": "Wrong Profile",
            "TeamIdentifier": ["TEAM123456"],
            "Entitlements": {
                "application-identifier": "TEAM123456.com.example.other",
                "com.apple.developer.system-extension.install": true,
                "com.apple.security.application-groups": ["group.rs.vtube-studio.dev"]
            }
        });
        let summary =
            provisioning_profile_summary_from_json(&value).expect("profile summary should parse");
        let error =
            validate_provisioning_profile_summary(&summary, ProvisioningProfileKind::ContainerApp)
                .expect_err("wrong bundle id should be rejected");
        assert!(error.to_string().contains("application-identifier"));
    }

    #[test]
    fn virtual_camera_readiness_report_reads_iosurface_manifest() {
        let root = env::temp_dir().join(format!(
            "vtube-studio-rs-virtual-camera-test-{}",
            timestamp_for_filename()
        ));
        let manifest_path = root.join("target/internal-output/iosurface.json");
        fs::create_dir_all(manifest_path.parent().expect("manifest parent"))
            .expect("manifest dir should be created");
        fs::write(
            &manifest_path,
            r#"{
  "iosurface_id": 42,
  "width": 1080,
  "height": 1080,
  "pixel_format": "BGRA8Unorm",
  "frame_rate": 60,
  "updated_unix_ms": 123456,
  "frames": 120
}"#,
        )
        .expect("manifest should be written");
        let config_path = root.join(BUILD_CONFIG_PATH);
        fs::write(
            &config_path,
            r#"
[output]
mode = "internal"

[output.internal]
producer = "iosurface"
manifest_path = "target/internal-output/iosurface.json"
activate_virtual_camera = true
"#,
        )
        .expect("config should be written");

        let report =
            build_virtual_camera_readiness_report(&root, SelectModelTarget::Build, &config_path)
                .expect("readiness report should build");

        assert!(report.markdown.contains("IOSurface id"));
        assert!(report.markdown.contains("`42`"));
        assert!(report.markdown.contains("`120`"));
        assert!(report.markdown.contains("`1080x1080`"));
        assert!(report.markdown.contains("`BGRA8Unorm`"));
        assert!(report.markdown.contains("`60 fps`"));
        assert!(report.markdown.contains("OBS-specific plugin"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tune_input_args_choose_target_input_and_preset() {
        let (target, input, preset) =
            parse_tune_input_args(vec!["camera".to_string(), "expressive".to_string()])
                .expect("default tune target should parse");
        assert!(matches!(target, SelectModelTarget::Development));
        assert_eq!(input, TuneInputTarget::Camera);
        assert_eq!(preset, TunePreset::Expressive);

        let (target, input, preset) = parse_tune_input_args(vec![
            "--build".to_string(),
            "mouth".to_string(),
            "soft".to_string(),
        ])
        .expect("build tune target should parse");
        assert!(matches!(target, SelectModelTarget::Build));
        assert_eq!(input, TuneInputTarget::Mouth);
        assert_eq!(preset, TunePreset::Soft);

        assert!(parse_tune_input_args(vec!["camera".to_string()]).is_err());
        assert!(parse_tune_input_args(vec!["camera".to_string(), "wild".to_string()]).is_err());
    }

    #[test]
    fn tune_input_presets_write_expected_config_values() {
        let (_, mouse) = TuneInputTarget::Mouse.preset_updates(TunePreset::Expressive);
        assert!(mouse.contains(&("enabled", "true".to_string())));
        assert!(mouse.contains(&("eye_x_range", "1.35".to_string())));
        assert!(mouse.contains(&("angle_z_degrees", "-16.2".to_string())));

        let (_, mouth) = TuneInputTarget::Mouth.preset_updates(TunePreset::Soft);
        assert!(mouth.contains(&("parameter", "\"ParamMouthOpenY\"".to_string())));
        assert!(mouth.contains(&("gain", "6.5".to_string())));
        assert!(mouth.contains(&("max_open", "0.75".to_string())));

        let (_, camera) = TuneInputTarget::Camera.preset_updates(TunePreset::Expressive);
        assert!(camera.contains(&("pose_mode", "\"camera_when_available\"".to_string())));
        assert!(camera.contains(&("angle_x_degrees", "37.5".to_string())));
        assert!(camera.contains(&("mouth_gain", "1.89".to_string())));
    }

    #[test]
    fn run_metal_args_support_release_and_optional_model_path() {
        let options = parse_run_metal_args(Vec::new()).expect("default run-metal should parse");
        assert!(!options.release);
        assert_eq!(options.model_path, None);

        let options = parse_run_metal_args(vec!["--release".to_string()])
            .expect("release run-metal should parse");
        assert!(options.release);
        assert_eq!(options.model_path, None);

        let options = parse_run_metal_args(vec![
            "--release".to_string(),
            "public/model/0.model3.json".to_string(),
        ])
        .expect("release run-metal with model path should parse");
        assert!(options.release);
        assert_eq!(
            options.model_path.as_deref(),
            Some("public/model/0.model3.json")
        );

        let options = parse_run_metal_args(vec![
            "public/model/0.model3.json".to_string(),
            "--release".to_string(),
        ])
        .expect("release flag after model path should parse");
        assert!(options.release);
        assert_eq!(
            options.model_path.as_deref(),
            Some("public/model/0.model3.json")
        );
    }

    #[test]
    fn build_app_args_support_dev_and_release_profiles() {
        let options = parse_build_app_args(Vec::new()).expect("default build-app should parse");
        assert!(!options.release);

        let options = parse_build_app_args(vec!["--release".to_string()])
            .expect("release build-app should parse");
        assert!(options.release);

        let options =
            parse_build_app_args(vec!["--dev".to_string()]).expect("dev build-app should parse");
        assert!(!options.release);
    }

    #[test]
    fn build_app_args_reject_unknown_options_and_paths() {
        assert!(parse_build_app_args(vec!["--fast".to_string()]).is_err());
        assert!(parse_build_app_args(vec!["public/model/0.model3.json".to_string()]).is_err());
        assert!(
            parse_build_app_args(vec!["--release".to_string(), "--release".to_string()]).is_err()
        );
    }

    #[test]
    fn run_metal_args_reject_unknown_options_and_extra_paths() {
        assert!(parse_run_metal_args(vec!["--fast".to_string()]).is_err());
        assert!(
            parse_run_metal_args(vec![
                "public/model/a.model3.json".to_string(),
                "public/model/b.model3.json".to_string()
            ])
            .is_err()
        );
    }

    #[test]
    fn camera_dev_info_plist_declares_privacy_usage() {
        let plist = dev_camera_info_plist("vtube-studio-rs");
        assert!(plist.contains("<string>vtube-studio-rs</string>"));
        assert!(plist.contains("NSCameraUsageDescription"));
        assert!(plist.contains("NSMicrophoneUsageDescription"));
        assert!(plist.contains("rs.vtube-studio.dev"));
    }

    #[test]
    fn parses_preferred_codesign_identity_from_security_output() {
        let output = r#"  1) ABCDEF1234567890 "Apple Development: Local Dev (TEAMID)"
  2) 0123456789ABCDEF "Developer ID Application: Example (TEAMID)"
     2 valid identities found"#;

        assert_eq!(
            find_codesign_identity_line(output, "Apple Development").as_deref(),
            Some("Apple Development: Local Dev (TEAMID)")
        );
    }

    #[test]
    fn codesign_identity_detection_accepts_distribution_identities() {
        let output = r#"  1) ABCDEF1234567890 "Apple Distribution: Example Team (TEAMID)"
  2) 0123456789ABCDEF "3rd Party Mac Developer Application: Example Team (TEAMID)"
     2 valid identities found"#;

        assert_eq!(
            find_codesign_identity_line(output, "Apple Distribution").as_deref(),
            Some("Apple Distribution: Example Team (TEAMID)")
        );
        assert_eq!(
            find_codesign_identity_line(output, "3rd Party Mac Developer Application").as_deref(),
            Some("3rd Party Mac Developer Application: Example Team (TEAMID)")
        );
    }

    #[test]
    fn detects_untrusted_codesign_identity_from_security_output() {
        let output = r#"Policy: X.509 Basic
  Matching identities
  1) FB1BC6B1A70D9D27086E5EA13F26170C35118C69 "Apple Development: Local Dev (TEAMID)" (CSSMERR_TP_NOT_TRUSTED)
     1 identities found

  Valid identities only
     0 valid identities found"#;

        assert_eq!(
            find_untrusted_codesign_identity_line(output).as_deref(),
            Some(
                r#"1) FB1BC6B1A70D9D27086E5EA13F26170C35118C69 "Apple Development: Local Dev (TEAMID)" (CSSMERR_TP_NOT_TRUSTED)"#
            )
        );
    }

    #[test]
    fn doctor_config_parses_model_path() {
        let config: DoctorConfig = toml::from_str(
            r#"
[app]
window_width = 540.0
window_height = 720.0

[input.mouse]
enabled = true
coordinate_space = "screen"

[input.microphone]
enabled = false

[input.camera]
enabled = true
pose_mode = "camera_when_available"
mouth_combine = "max"

[renderer]
atlas_anisotropy = 8
debug_texture_mode = "uv"

[motion]
blink_interval = 3.8
blink_duration = 0.18

[model]
path = "public/model/0.model3.json"
"#,
        )
        .expect("doctor config should parse");

        assert_eq!(config.app.window_width, Some(540.0));
        assert_eq!(config.input.mouse.enabled, Some(true));
        assert_eq!(
            config.input.camera.pose_mode.as_deref(),
            Some("camera_when_available")
        );
        assert_eq!(config.renderer.atlas_anisotropy, Some(8));
        assert_eq!(config.motion.blink_interval, Some(3.8));
        assert_eq!(
            config.model.path.as_deref(),
            Some("public/model/0.model3.json")
        );
    }

    #[test]
    fn doctor_window_dimension_validation_matches_runtime_bounds() {
        assert!(valid_doctor_window_dimension(96.0));
        assert!(valid_doctor_window_dimension(2400.0));
        assert!(!valid_doctor_window_dimension(95.9));
        assert!(!valid_doctor_window_dimension(2400.1));
        assert!(!valid_doctor_window_dimension(f64::NAN));
    }

    #[test]
    fn doctor_input_mode_validation_accepts_runtime_aliases() {
        assert_eq!(normalized_debug_texture_mode("none"), Some("none"));
        assert_eq!(normalized_debug_texture_mode("texture"), Some("rgb"));
        assert_eq!(normalized_debug_texture_mode("alpha"), Some("alpha"));
        assert_eq!(normalized_debug_texture_mode("depth"), None);

        assert_eq!(normalized_mouse_coordinate_space("screen"), Some("screen"));
        assert_eq!(normalized_mouse_coordinate_space("window"), Some("window"));
        assert_eq!(normalized_mouse_coordinate_space("global"), None);

        assert_eq!(
            normalized_camera_pose_mode("camera_when_available"),
            Some("camera_when_available")
        );
        assert_eq!(normalized_camera_pose_mode("face"), Some("camera"));
        assert_eq!(normalized_camera_pose_mode("mouse"), Some("mouse"));
        assert_eq!(normalized_camera_pose_mode("bad"), None);

        assert_eq!(normalized_mouth_combine_mode("max"), Some("max"));
        assert_eq!(normalized_mouth_combine_mode("mic"), Some("microphone"));
        assert_eq!(normalized_mouth_combine_mode("camera"), Some("camera"));
        assert_eq!(normalized_mouth_combine_mode("average"), None);
    }

    #[test]
    fn doctor_input_range_validation_counts_invalid_values() {
        let target = SelectModelTarget::Development;
        assert_eq!(
            check_optional_range(target, "field", Some(0.5), 0.0, 1.0),
            0
        );
        assert_eq!(
            check_optional_range(target, "field", Some(-0.1), 0.0, 1.0),
            1
        );
        assert_eq!(
            check_optional_range(target, "field", Some(f64::NAN), 0.0, 1.0),
            1
        );
    }

    #[test]
    fn toml_string_literal_escapes_quotes_and_backslashes() {
        assert_eq!(
            toml_string_literal(r#"public/model/"avatar"\0.model3.json"#),
            r#""public/model/\"avatar\"\\0.model3.json""#
        );
    }

    #[test]
    fn model_name_strips_model3_suffix() {
        assert_eq!(
            model_name_from_path("public/CubismSdkForNative/Samples/Resources/Ren/Ren.model3.json"),
            "Ren"
        );
        assert_eq!(model_name_from_path("/tmp/avatar.json"), "avatar.json");
    }

    #[test]
    fn model_manifest_summary_counts_resources() {
        let root = env::temp_dir().join(format!(
            "vtube-studio-rs-model-list-test-{}",
            timestamp_for_filename()
        ));
        fs::create_dir_all(&root).expect("test dir should be created");
        let manifest_path = root.join("Avatar.model3.json");
        fs::write(
            &manifest_path,
            r#"
{
  "Version": 3,
  "FileReferences": {
    "Moc": "Avatar.moc3",
    "Textures": ["texture_00.png", "texture_01.png"],
    "Physics": "Avatar.physics3.json",
    "DisplayInfo": "Avatar.cdi3.json",
    "Motions": {
      "Idle": [{ "File": "idle_00.motion3.json" }],
      "TapBody": [{ "File": "tap_00.motion3.json" }, { "File": "tap_01.motion3.json" }]
    },
    "Expressions": [
      { "Name": "smile", "File": "smile.exp3.json" }
    ]
  }
}
"#,
        )
        .expect("manifest should be written");

        let summary = ModelManifestSummary::load(&manifest_path).expect("manifest should parse");

        assert_eq!(summary.name, "Avatar");
        assert_eq!(summary.texture_count, 2);
        assert_eq!(summary.motion_count, 3);
        assert_eq!(summary.expression_count, 1);
        assert!(summary.has_physics);
        assert!(summary.has_display_info);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn renderer_event_count_counts_exact_event_names() {
        let log = concat!(
            "renderer_event=long_frame_gap frames=1\n",
            "renderer_event=long_frame_gap frames=2\n",
            "renderer_event=long_frame_gap_extra\n",
        );

        assert_eq!(renderer_event_count(log, "long_frame_gap"), 2);
    }

    #[test]
    fn recent_renderer_events_returns_last_matching_lines() {
        let log = concat!(
            "not an event\n",
            "renderer_event=first\n",
            "renderer_event=second\n",
            "renderer_event=third\n",
        );

        assert_eq!(
            recent_renderer_events(log, 2),
            "renderer_event=second\nrenderer_event=third"
        );
        assert_eq!(
            recent_renderer_events("no events", 2),
            "No renderer_event lines were recorded."
        );
    }
}
