#![cfg(feature = "cubism-core")]

use crate::cubism::{CubismDrawableFrame, CubismDrawableInfo, CubismModelRuntime};
use crate::live2d_model::Live2dModel;
use image::{Rgba, RgbaImage};

const OUTPUT_SIZE: u32 = 512;

pub struct SoftwareRenderer {
    textures: Vec<RgbaImage>,
    output: RgbaImage,
}

impl SoftwareRenderer {
    pub fn load(model: &Live2dModel) -> Result<Self, String> {
        let mut textures = Vec::with_capacity(model.textures.len());
        for texture_path in &model.textures {
            let texture = image::open(texture_path)
                .map_err(|error| {
                    format!("Failed to load texture {}: {error}", texture_path.display())
                })?
                .to_rgba8();
            textures.push(texture);
        }

        Ok(Self {
            textures,
            output: RgbaImage::new(OUTPUT_SIZE, OUTPUT_SIZE),
        })
    }

    pub fn render(&mut self, runtime: &CubismModelRuntime) -> &[u8] {
        clear(&mut self.output);

        let mut drawables = runtime.drawables();
        drawables.sort_by_key(|drawable| drawable.render_order);
        let mut draw_items = Vec::new();
        let mut bounds = Bounds::empty();

        for drawable in drawables {
            if !drawable.flags.visible || drawable.opacity <= 0.0 {
                continue;
            }

            if self
                .textures
                .get(drawable.texture_index.max(0) as usize)
                .is_none()
            {
                continue;
            }
            let Some(frame) = runtime.drawable_frame_by_index(drawable.index) else {
                continue;
            };

            for position in &frame.positions {
                bounds.include(*position);
            }
            draw_items.push(DrawItem { drawable, frame });
        }

        let Some(transform) = bounds.fit_transform(OUTPUT_SIZE as f32, 24.0) else {
            return self.output.as_raw();
        };

        for item in draw_items {
            let Some(texture) = self
                .textures
                .get(item.drawable.texture_index.max(0) as usize)
            else {
                continue;
            };

            for triangle in item.frame.indices.chunks_exact(3) {
                let i0 = triangle[0] as usize;
                let i1 = triangle[1] as usize;
                let i2 = triangle[2] as usize;
                if i0 >= item.frame.positions.len()
                    || i1 >= item.frame.positions.len()
                    || i2 >= item.frame.positions.len()
                {
                    continue;
                }

                rasterize_triangle(
                    &mut self.output,
                    texture,
                    [
                        vertex(item.frame.positions[i0], item.frame.uvs[i0], transform),
                        vertex(item.frame.positions[i1], item.frame.uvs[i1], transform),
                        vertex(item.frame.positions[i2], item.frame.uvs[i2], transform),
                    ],
                    item.drawable.opacity,
                );
            }
        }

        self.output.as_raw()
    }
}

struct DrawItem {
    drawable: CubismDrawableInfo,
    frame: CubismDrawableFrame,
}

#[derive(Clone, Copy)]
struct Vertex {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
}

#[derive(Clone, Copy)]
struct FitTransform {
    min_x: f32,
    min_y: f32,
    scale: f32,
    offset_x: f32,
    offset_y: f32,
}

#[derive(Clone, Copy)]
struct Bounds {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl Bounds {
    fn empty() -> Self {
        Self {
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
        }
    }

    fn include(&mut self, position: [f32; 2]) {
        self.min_x = self.min_x.min(position[0]);
        self.min_y = self.min_y.min(position[1]);
        self.max_x = self.max_x.max(position[0]);
        self.max_y = self.max_y.max(position[1]);
    }

    fn fit_transform(&self, output_size: f32, padding: f32) -> Option<FitTransform> {
        if !self.min_x.is_finite() {
            return None;
        }

        let width = self.max_x - self.min_x;
        let height = self.max_y - self.min_y;
        if width <= f32::EPSILON || height <= f32::EPSILON {
            return None;
        }

        let drawable_size = (output_size - padding * 2.0).max(1.0);
        let scale = (drawable_size / width).min(drawable_size / height);
        let rendered_width = width * scale;
        let rendered_height = height * scale;

        Some(FitTransform {
            min_x: self.min_x,
            min_y: self.min_y,
            scale,
            offset_x: (output_size - rendered_width) * 0.5,
            offset_y: (output_size - rendered_height) * 0.5,
        })
    }
}

fn clear(image: &mut RgbaImage) {
    for pixel in image.pixels_mut() {
        *pixel = Rgba([0, 0, 0, 0]);
    }
}

fn vertex(position: [f32; 2], uv: [f32; 2], transform: FitTransform) -> Vertex {
    Vertex {
        x: transform.offset_x + (position[0] - transform.min_x) * transform.scale,
        y: OUTPUT_SIZE as f32
            - (transform.offset_y + (position[1] - transform.min_y) * transform.scale),
        u: uv[0],
        v: uv[1],
    }
}

fn rasterize_triangle(
    target: &mut RgbaImage,
    texture: &RgbaImage,
    vertices: [Vertex; 3],
    opacity: f32,
) {
    let min_x = vertices
        .iter()
        .map(|vertex| vertex.x)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as u32;
    let max_x = vertices
        .iter()
        .map(|vertex| vertex.x)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min((OUTPUT_SIZE - 1) as f32) as u32;
    let min_y = vertices
        .iter()
        .map(|vertex| vertex.y)
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as u32;
    let max_y = vertices
        .iter()
        .map(|vertex| vertex.y)
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil()
        .min((OUTPUT_SIZE - 1) as f32) as u32;

    let area = edge(vertices[0], vertices[1], vertices[2].x, vertices[2].y);
    if area.abs() <= f32::EPSILON {
        return;
    }

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = edge(vertices[1], vertices[2], px, py) / area;
            let w1 = edge(vertices[2], vertices[0], px, py) / area;
            let w2 = edge(vertices[0], vertices[1], px, py) / area;

            let same_winding =
                (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0);
            if !same_winding {
                continue;
            }

            let u = w0 * vertices[0].u + w1 * vertices[1].u + w2 * vertices[2].u;
            let v = w0 * vertices[0].v + w1 * vertices[1].v + w2 * vertices[2].v;
            let source = sample(texture, u, v);
            blend(target.get_pixel_mut(x, y), source, opacity);
        }
    }
}

fn edge(a: Vertex, b: Vertex, x: f32, y: f32) -> f32 {
    (x - a.x) * (b.y - a.y) - (y - a.y) * (b.x - a.x)
}

fn sample(texture: &RgbaImage, u: f32, v: f32) -> Rgba<u8> {
    let x = (u.clamp(0.0, 1.0) * (texture.width() - 1) as f32).round() as u32;
    let y = (v.clamp(0.0, 1.0) * (texture.height() - 1) as f32).round() as u32;
    *texture.get_pixel(x, y)
}

fn blend(target: &mut Rgba<u8>, source: Rgba<u8>, opacity: f32) {
    let source_alpha = (source[3] as f32 / 255.0) * opacity.clamp(0.0, 1.0);
    if source_alpha <= 0.0 {
        return;
    }

    let target_alpha = target[3] as f32 / 255.0;
    let out_alpha = source_alpha + target_alpha * (1.0 - source_alpha);
    if out_alpha <= 0.0 {
        return;
    }

    for channel in 0..3 {
        let source_value = source[channel] as f32 / 255.0;
        let target_value = target[channel] as f32 / 255.0;
        let out = (source_value * source_alpha
            + target_value * target_alpha * (1.0 - source_alpha))
            / out_alpha;
        target[channel] = (out * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    target[3] = (out_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
}

#[cfg(test)]
mod tests {
    use super::SoftwareRenderer;
    use crate::{cubism, live2d_model::Live2dModel};

    #[test]
    fn renders_non_empty_frame_from_public_model() {
        let model =
            Live2dModel::load("public/model/0.model3.json").expect("public model should load");
        let mut runtime = cubism::load_runtime(&model).expect("Cubism runtime should load");
        runtime.update();

        let mut renderer = SoftwareRenderer::load(&model).expect("textures should load");
        let frame = renderer.render(&runtime);
        let opaque_pixels = frame.chunks_exact(4).filter(|pixel| pixel[3] > 0).count();

        assert!(
            opaque_pixels > 0,
            "software renderer produced an empty frame"
        );
    }
}
