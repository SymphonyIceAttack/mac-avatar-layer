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

    if let Err(error) = macos_app::run(&model_path) {
        eprintln!("vtube-studio-rs failed to start: {error}");
        std::process::exit(1);
    }
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
        "{:<72} {:>7} {:>5} {:>5} {:>5} {:>8} {:>10} status",
        "model", "params", "parts", "draw", "mask", "offscreen", "textures"
    );

    for path in model_paths {
        let display_path = path.display().to_string();
        match live2d_model::Live2dModel::load(&path)
            .and_then(|model| probe_model(&model).map(|summary| (model, summary)))
        {
            Ok((model, summary)) => {
                println!(
                    "{:<72} {:>7} {:>5} {:>5} {:>5} {:>8} {:>10} ok",
                    display_path,
                    summary.parameter_count,
                    summary.part_count,
                    summary.drawable_count,
                    summary.masked_drawable_count,
                    summary.offscreen_count,
                    model.textures.len()
                );
                for detail in &summary.offscreen_details {
                    println!("  {detail}");
                }
                for detail in &summary.drawable_details {
                    println!("  {detail}");
                }
            }
            Err(error) => {
                println!(
                    "{:<72} {:>7} {:>5} {:>5} {:>5} {:>8} {:>10} error: {}",
                    display_path, "-", "-", "-", "-", "-", "-", error
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
    offscreen_count: i32,
    offscreen_details: Vec<String>,
    drawable_details: Vec<String>,
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
            offscreen_count: 0,
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
    Ok(ModelProbeSummary {
        parameter_count: info.parameter_count.unwrap_or(0),
        part_count: info.part_count.unwrap_or(0),
        drawable_count: info.drawable_count.unwrap_or(0),
        masked_drawable_count: drawables
            .iter()
            .filter(|drawable| !drawable.masks.is_empty())
            .count(),
        offscreen_count: info.offscreen_count.unwrap_or(0),
        offscreen_details: offscreens
            .iter()
            .take(12)
            .map(|offscreen| {
                let owner = parts
                    .get(offscreen.owner_part_index.max(0) as usize)
                    .map(|part| format!("{}({})", part.id, part.index))
                    .unwrap_or_else(|| format!("-({})", offscreen.owner_part_index));
                format!(
                    "offscreen #{} owner {} render {} blend {} opacity {:.3} multiply {:?} screen {:?} masks {} inverted_mask={}",
                    offscreen.index,
                    owner,
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
            .chain(drawables.iter()
            .filter(|drawable| {
                drawable.blend_mode != cubism::CubismBlendMode::Normal
                    || drawable.multiply_color != [1.0, 1.0, 1.0, 1.0]
                    || drawable.screen_color != [0.0, 0.0, 0.0, 1.0]
            })
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
            }))
            .collect(),
    })
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
