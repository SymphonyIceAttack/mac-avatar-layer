#[cfg(target_os = "macos")]
mod audio_input;
#[cfg(target_os = "macos")]
mod config;
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
    let args = std::env::args().skip(1).collect::<Vec<_>>();

    if args.first().is_some_and(|arg| arg == "--probe-models") {
        let roots = if args.len() > 1 {
            args[1..].to_vec()
        } else {
            vec!["public".to_string()]
        };
        if let Err(error) = probe_models(&roots) {
            eprintln!("vtube-studio-rs model probe failed: {error}");
            std::process::exit(1);
        }
        return;
    }

    let model_path = args
        .first()
        .cloned()
        .unwrap_or_else(|| "public/model/0.model3.json".to_string());

    let _instance_guard = match AppInstanceGuard::acquire() {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("vtube-studio-rs failed to start: {error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = macos_app::run(&model_path) {
        eprintln!("vtube-studio-rs failed to start: {error}");
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
struct AppInstanceGuard {
    path: std::path::PathBuf,
    _file: std::fs::File,
}

#[cfg(target_os = "macos")]
impl AppInstanceGuard {
    fn acquire() -> Result<Self, String> {
        if std::env::var("VTUBE_RS_ALLOW_DUPLICATE_INSTANCE").is_ok_and(|value| value == "1") {
            let path =
                std::env::temp_dir().join(format!("vtube-studio-rs-{}.pid", std::process::id()));
            let file = std::fs::File::create(&path)
                .map_err(|error| format!("Failed to create temporary instance guard: {error}"))?;
            return Ok(Self { path, _file: file });
        }

        let target_dir = std::env::current_dir()
            .map_err(|error| format!("Failed to resolve current directory: {error}"))?
            .join("target");
        std::fs::create_dir_all(&target_dir)
            .map_err(|error| format!("Failed to create target directory: {error}"))?;
        let path = target_dir.join("vtube-studio-rs.pid");

        for attempt in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    use std::io::Write;
                    writeln!(file, "{}", std::process::id())
                        .map_err(|error| format!("Failed to write instance guard: {error}"))?;
                    println!(
                        "renderer_event=instance_guard_acquired pid={} path=\"{}\"",
                        std::process::id(),
                        path.display()
                    );
                    return Ok(Self { path, _file: file });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let existing_pid = read_pid_file(&path);
                    if let Some(pid) = existing_pid.filter(|pid| process_is_alive(*pid)) {
                        return Err(format!(
                            "another vtube-studio-rs instance is already running (pid {pid}). Close it first, or set VTUBE_RS_ALLOW_DUPLICATE_INSTANCE=1 for debugging."
                        ));
                    }
                    let _ = std::fs::remove_file(&path);
                    if attempt == 0 {
                        continue;
                    }
                    return Err(format!(
                        "stale instance guard exists and could not be replaced: {}",
                        path.display()
                    ));
                }
                Err(error) => {
                    return Err(format!("Failed to create instance guard: {error}"));
                }
            }
        }

        Err("Failed to acquire instance guard".to_string())
    }
}

#[cfg(target_os = "macos")]
impl Drop for AppInstanceGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        println!(
            "renderer_event=instance_guard_released path=\"{}\"",
            self.path.display()
        );
    }
}

#[cfg(target_os = "macos")]
fn read_pid_file(path: &std::path::Path) -> Option<u32> {
    let value = std::fs::read_to_string(path).ok()?;
    value.trim().parse().ok()
}

#[cfg(target_os = "macos")]
fn process_is_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "macos")]
fn probe_models(roots: &[String]) -> Result<(), String> {
    let mut model_paths = Vec::new();
    for root in roots {
        collect_model3_paths(std::path::Path::new(root), &mut model_paths)?;
    }
    model_paths.sort();

    if model_paths.is_empty() {
        return Err(format!(
            "No .model3.json files found under: {}",
            roots.join(", ")
        ));
    }

    println!(
        "Probing {} Live2D model(s) under {}",
        model_paths.len(),
        roots.join(", ")
    );
    println!(
        "{:<72} {:>7} {:>5} {:>5} {:>5} {:>7} {:>4} {:>4} {:>4} {:>4} {:>7} status",
        "model", "params", "parts", "draw", "mask", "maxMask", "add", "mult", "ext", "inv", "off"
    );

    for path in model_paths {
        let display_path = path.display().to_string();
        match live2d_model::Live2dModel::load(&path)
            .and_then(|model| probe_model(&model).map(|summary| (model, summary)))
        {
            Ok((_model, summary)) => {
                println!(
                    "{:<72} {:>7} {:>5} {:>5} {:>5} {:>7} {:>4} {:>4} {:>4} {:>4} {:>7} ok {}",
                    display_path,
                    summary.parameter_count,
                    summary.part_count,
                    summary.drawable_count,
                    summary.masked_drawable_count,
                    summary.max_mask_count,
                    summary.additive_count,
                    summary.multiplicative_count,
                    summary.extended_blend_count,
                    summary.inverted_mask_count,
                    summary.offscreen_count,
                    summary.risk_label()
                );
                for detail in &summary.risk_details {
                    println!("  {detail}");
                }
                for detail in &summary.offscreen_details {
                    println!("  {detail}");
                }
                for detail in &summary.drawable_details {
                    println!("  {detail}");
                }
            }
            Err(error) => {
                println!(
                    "{:<72} {:>7} {:>5} {:>5} {:>5} {:>7} {:>4} {:>4} {:>4} {:>4} {:>7} error: {}",
                    display_path, "-", "-", "-", "-", "-", "-", "-", "-", "-", "-", error
                );
            }
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct ModelProbeSummary {
    parameter_count: i32,
    part_count: i32,
    drawable_count: i32,
    masked_drawable_count: usize,
    max_mask_count: usize,
    additive_count: usize,
    multiplicative_count: usize,
    extended_blend_count: usize,
    inverted_mask_count: usize,
    offscreen_count: i32,
    risk_details: Vec<String>,
    offscreen_details: Vec<String>,
    drawable_details: Vec<String>,
}

#[cfg(target_os = "macos")]
impl ModelProbeSummary {
    fn risk_label(&self) -> &'static str {
        if self.offscreen_count > 0 || self.extended_blend_count > 0 || self.max_mask_count > 4 {
            "risk:high"
        } else if self.masked_drawable_count > 0
            || self.additive_count > 0
            || self.multiplicative_count > 0
            || self.inverted_mask_count > 0
        {
            "risk:medium"
        } else {
            "risk:low"
        }
    }
}

#[cfg(all(target_os = "macos", not(feature = "cubism-core")))]
#[allow(dead_code)]
impl ModelProbeSummary {
    fn unavailable() -> Self {
        Self {
            parameter_count: 0,
            part_count: 0,
            drawable_count: 0,
            masked_drawable_count: 0,
            max_mask_count: 0,
            additive_count: 0,
            multiplicative_count: 0,
            extended_blend_count: 0,
            inverted_mask_count: 0,
            offscreen_count: 0,
            risk_details: Vec::new(),
            offscreen_details: Vec::new(),
            drawable_details: Vec::new(),
        }
    }
}

#[cfg(all(target_os = "macos", feature = "cubism-core"))]
fn probe_model(model: &live2d_model::Live2dModel) -> Result<ModelProbeSummary, String> {
    let runtime = cubism::load_runtime(model)?;
    let info = runtime.info();
    let drawables = runtime.drawables();
    let parts = runtime.parts();
    let offscreens = runtime.offscreens();
    let texture_cache = load_probe_textures(model);
    let risk = summarize_render_risk(&drawables, &offscreens, &parts, model.textures.len());
    Ok(ModelProbeSummary {
        parameter_count: info.parameter_count.unwrap_or(0),
        part_count: info.part_count.unwrap_or(0),
        drawable_count: info.drawable_count.unwrap_or(0),
        masked_drawable_count: risk.masked_drawable_count,
        max_mask_count: risk.max_mask_count,
        additive_count: risk.additive_count,
        multiplicative_count: risk.multiplicative_count,
        extended_blend_count: risk.extended_blend_count,
        inverted_mask_count: risk.inverted_mask_count,
        offscreen_count: info.offscreen_count.unwrap_or(0),
        risk_details: risk.details,
        offscreen_details: offscreens
            .iter()
            .take(32)
            .map(|offscreen| {
                let owner = parts
                    .get(offscreen.owner_part_index.max(0) as usize)
                    .map(|part| format!("{}({})", part.id, part.index))
                    .unwrap_or_else(|| format!("-({})", offscreen.owner_part_index));
                let depth = offscreen_depth(offscreen.owner_part_index, &offscreens, &parts);
                format!(
                    "offscreen #{} owner {} depth {} render {} blend {} opacity {:.3} multiply {:?} screen {:?} masks {} inverted_mask={}",
                    offscreen.index,
                    owner,
                    depth,
                    offscreen.render_order,
                    offscreen.blend_mode.description(),
                    offscreen.opacity,
                    offscreen.multiply_color,
                    offscreen.screen_color,
                    offscreen.masks.len(),
                    offscreen.flags.inverted_mask
                )
            })
            .collect(),
        drawable_details: drawables
            .iter()
            .filter(|drawable| drawable.opacity > 0.001)
            .filter_map(|drawable| {
                let sample = sample_drawable_texture(&texture_cache, &runtime, drawable)?;
                (sample[3] > 0.05 && sample[0] + sample[1] + sample[2] > 0.2).then(|| {
                    format!(
                        "sampled drawable #{} {} part {} render {} blend {} tex {} opacity {:.3} avg rgba [{:.2}, {:.2}, {:.2}, {:.2}] masks {}",
                        drawable.index,
                        drawable.id,
                        drawable.parent_part_id.as_deref().unwrap_or("-"),
                        drawable.render_order,
                        drawable.blend_mode.description(),
                        drawable.texture_index,
                        drawable.opacity,
                        sample[0],
                        sample[1],
                        sample[2],
                        sample[3],
                        drawable.masks.len()
                    )
                })
            })
            .take(16)
            .chain(
                drawables
                    .iter()
                    .filter(|drawable| {
                        drawable.blend_mode != cubism::CubismBlendMode::Normal
                            || drawable.multiply_color != [1.0, 1.0, 1.0, 1.0]
                            || drawable.screen_color != [0.0, 0.0, 0.0, 1.0]
                    })
                    .take(24)
                    .map(|drawable| {
                        format!(
                            "drawable #{} {} part {} render {} blend {} opacity {:.3} multiply {:?} screen {:?} masks {}",
                            drawable.index,
                            drawable.id,
                            drawable.parent_part_id.as_deref().unwrap_or("-"),
                            drawable.render_order,
                            drawable.blend_mode.description(),
                            drawable.opacity,
                            drawable.multiply_color,
                            drawable.screen_color,
                            drawable.masks.len()
                        )
                    }),
            )
            .collect(),
    })
}

#[cfg(all(target_os = "macos", feature = "cubism-core"))]
#[derive(Debug, Default)]
struct RenderRiskSummary {
    masked_drawable_count: usize,
    max_mask_count: usize,
    additive_count: usize,
    multiplicative_count: usize,
    extended_blend_count: usize,
    masked_extended_drawable_count: usize,
    extended_offscreen_count: usize,
    masked_offscreen_count: usize,
    nested_offscreen_count: usize,
    max_offscreen_depth: usize,
    inverted_mask_count: usize,
    details: Vec<String>,
}

#[cfg(all(target_os = "macos", feature = "cubism-core"))]
fn summarize_render_risk(
    drawables: &[cubism::CubismDrawableInfo],
    offscreens: &[cubism::CubismOffscreenInfo],
    parts: &[cubism::CubismPartInfo],
    texture_count: usize,
) -> RenderRiskSummary {
    let masked_extended_drawable_count = drawables
        .iter()
        .filter(|drawable| {
            !drawable.masks.is_empty()
                && matches!(
                    drawable.blend_mode,
                    cubism::CubismBlendMode::Extended { .. }
                )
        })
        .count();
    let extended_offscreen_count = offscreens
        .iter()
        .filter(|offscreen| {
            matches!(
                offscreen.blend_mode,
                cubism::CubismBlendMode::Extended { .. }
            )
        })
        .count();
    let masked_offscreen_count = offscreens
        .iter()
        .filter(|offscreen| !offscreen.masks.is_empty())
        .count();
    let offscreen_depths = offscreens
        .iter()
        .map(|offscreen| offscreen_depth(offscreen.owner_part_index, offscreens, parts))
        .collect::<Vec<_>>();
    let nested_offscreen_count = offscreen_depths.iter().filter(|depth| **depth > 1).count();
    let max_offscreen_depth = offscreen_depths.into_iter().max().unwrap_or(0);

    let mut summary = RenderRiskSummary {
        masked_drawable_count: drawables
            .iter()
            .filter(|drawable| !drawable.masks.is_empty())
            .count(),
        max_mask_count: drawables
            .iter()
            .map(|drawable| drawable.masks.len())
            .chain(offscreens.iter().map(|offscreen| offscreen.masks.len()))
            .max()
            .unwrap_or(0),
        additive_count: drawables
            .iter()
            .filter(|drawable| drawable.blend_mode == cubism::CubismBlendMode::Additive)
            .count(),
        multiplicative_count: drawables
            .iter()
            .filter(|drawable| drawable.blend_mode == cubism::CubismBlendMode::Multiplicative)
            .count(),
        extended_blend_count: drawables
            .iter()
            .filter(|drawable| {
                matches!(
                    drawable.blend_mode,
                    cubism::CubismBlendMode::Extended { .. }
                )
            })
            .count()
            + offscreens
                .iter()
                .filter(|offscreen| {
                    matches!(
                        offscreen.blend_mode,
                        cubism::CubismBlendMode::Extended { .. }
                    )
                })
                .count(),
        masked_extended_drawable_count,
        extended_offscreen_count,
        masked_offscreen_count,
        nested_offscreen_count,
        max_offscreen_depth,
        inverted_mask_count: drawables
            .iter()
            .filter(|drawable| drawable.flags.inverted_mask)
            .count()
            + offscreens
                .iter()
                .filter(|offscreen| offscreen.flags.inverted_mask)
                .count(),
        details: Vec::new(),
    };

    let invalid_textures = drawables
        .iter()
        .filter(|drawable| {
            drawable.texture_index < 0 || drawable.texture_index as usize >= texture_count
        })
        .count();
    if invalid_textures > 0 {
        summary.details.push(format!(
            "risk invalid texture indices: {invalid_textures} drawable(s)"
        ));
    }
    if summary.max_mask_count > 4 {
        summary.details.push(format!(
            "risk dense clipping: max {} masks in one drawable/offscreen",
            summary.max_mask_count
        ));
    }
    if summary.additive_count > 0 || summary.multiplicative_count > 0 {
        summary.details.push(format!(
            "risk blend modes: {} additive, {} multiplicative drawable(s)",
            summary.additive_count, summary.multiplicative_count
        ));
    }
    if summary.masked_drawable_count > 32 {
        summary.details.push(format!(
            "risk many masked drawables: {} shared-mask contexts likely",
            summary.masked_drawable_count
        ));
    }
    if !offscreens.is_empty() {
        summary.details.push(format!(
            "risk offscreen objects: {} require render-target compositing",
            offscreens.len()
        ));
    }
    if summary.masked_offscreen_count > 0 {
        summary.details.push(format!(
            "risk masked offscreens: {} require clipping during offscreen draws",
            summary.masked_offscreen_count
        ));
    }
    if summary.extended_blend_count > 0 {
        summary.details.push(format!(
            "risk extended blend objects: {} use snapshot compositing",
            summary.extended_blend_count
        ));
    }
    if summary.masked_extended_drawable_count > 0 {
        summary.details.push(format!(
            "risk masked extended drawables: {} require mask + snapshot compositing",
            summary.masked_extended_drawable_count
        ));
    }
    if summary.extended_offscreen_count > 0 {
        summary.details.push(format!(
            "risk extended offscreens: {} require offscreen snapshot compositing",
            summary.extended_offscreen_count
        ));
    }
    if summary.nested_offscreen_count > 0 {
        summary.details.push(format!(
            "risk nested offscreens: {} object(s), max depth {}",
            summary.nested_offscreen_count, summary.max_offscreen_depth
        ));
    }
    if summary.inverted_mask_count > 0 {
        summary.details.push(format!(
            "risk inverted masks: {} object(s)",
            summary.inverted_mask_count
        ));
    }
    let translucent = drawables
        .iter()
        .filter(|drawable| drawable.opacity > 0.0 && drawable.opacity < 0.999)
        .count();
    if translucent > 0 {
        summary
            .details
            .push(format!("risk translucent drawables: {translucent}"));
    }

    summary
}

#[cfg(all(target_os = "macos", feature = "cubism-core"))]
fn offscreen_depth(
    owner_part_index: i32,
    offscreens: &[cubism::CubismOffscreenInfo],
    parts: &[cubism::CubismPartInfo],
) -> usize {
    let ancestor_count = offscreens
        .iter()
        .filter(|ancestor| {
            ancestor.owner_part_index != owner_part_index
                && part_is_descendant_of(owner_part_index, ancestor.owner_part_index, parts)
        })
        .count();
    ancestor_count + 1
}

#[cfg(all(target_os = "macos", feature = "cubism-core"))]
fn part_is_descendant_of(
    mut part_index: i32,
    ancestor_part_index: i32,
    parts: &[cubism::CubismPartInfo],
) -> bool {
    let mut guard = 0;
    while part_index >= 0 && guard <= parts.len() {
        if part_index == ancestor_part_index {
            return true;
        }
        let Some(part) = parts.get(part_index as usize) else {
            return false;
        };
        part_index = part.parent_part_index;
        guard += 1;
    }
    false
}

#[cfg(all(target_os = "macos", feature = "cubism-core"))]
fn load_probe_textures(model: &live2d_model::Live2dModel) -> Vec<Option<image::RgbaImage>> {
    model
        .textures
        .iter()
        .map(|texture_path| image::open(texture_path).ok().map(|image| image.to_rgba8()))
        .collect()
}

#[cfg(all(target_os = "macos", feature = "cubism-core"))]
fn sample_drawable_texture(
    textures: &[Option<image::RgbaImage>],
    runtime: &cubism::CubismModelRuntime,
    drawable: &cubism::CubismDrawableInfo,
) -> Option<[f32; 4]> {
    let texture = textures
        .get(drawable.texture_index.max(0) as usize)?
        .as_ref()?;
    let frame = runtime.drawable_frame_by_index(drawable.index)?;
    let mut total = [0.0; 4];
    let mut count = 0.0;
    for triangle in frame.indices.chunks_exact(3).take(32) {
        let uv0 = frame.uvs.get(triangle[0] as usize)?;
        let uv1 = frame.uvs.get(triangle[1] as usize)?;
        let uv2 = frame.uvs.get(triangle[2] as usize)?;
        let u = ((uv0[0] + uv1[0] + uv2[0]) / 3.0).clamp(0.0, 1.0);
        let v = (1.0 - ((uv0[1] + uv1[1] + uv2[1]) / 3.0)).clamp(0.0, 1.0);
        let x = (u * (texture.width().saturating_sub(1)) as f32).round() as u32;
        let y = (v * (texture.height().saturating_sub(1)) as f32).round() as u32;
        let pixel = texture.get_pixel(x, y);
        for channel in 0..4 {
            total[channel] += pixel[channel] as f32 / 255.0;
        }
        count += 1.0;
    }
    (count > 0.0).then(|| {
        [
            total[0] / count,
            total[1] / count,
            total[2] / count,
            total[3] / count,
        ]
    })
}

#[cfg(all(target_os = "macos", not(feature = "cubism-core")))]
fn probe_model(_model: &live2d_model::Live2dModel) -> Result<ModelProbeSummary, String> {
    Err("model probing requires --features cubism-core".to_string())
}

#[cfg(target_os = "macos")]
fn collect_model3_paths(
    root: &std::path::Path,
    paths: &mut Vec<std::path::PathBuf>,
) -> Result<(), String> {
    let metadata = std::fs::metadata(root)
        .map_err(|error| format!("Failed to inspect {}: {error}", root.display()))?;
    if metadata.is_file() {
        if root
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".model3.json"))
        {
            paths.push(root.to_path_buf());
        }
        return Ok(());
    }

    for entry in std::fs::read_dir(root)
        .map_err(|error| format!("Failed to read {}: {error}", root.display()))?
    {
        let entry = entry
            .map_err(|error| format!("Failed to read entry in {}: {error}", root.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
        if file_type.is_dir() {
            collect_model3_paths(&path, paths)?;
        } else if file_type.is_file()
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".model3.json"))
        {
            paths.push(path);
        }
    }

    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("vtube-studio-rs currently targets macOS because the first milestone uses AppKit.");
    std::process::exit(1);
}
