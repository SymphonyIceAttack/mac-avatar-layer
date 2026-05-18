#![cfg(feature = "metal-renderer")]

use crate::cubism::CubismModelRuntime;
use crate::live2d_model::Live2dModel;
use metal::foreign_types::ForeignType;
use metal::{CommandQueue, Device};

pub struct MetalRenderer {
    device: Device,
    command_queue: CommandQueue,
    texture_count: usize,
}

impl MetalRenderer {
    pub fn load(model: &Live2dModel) -> Result<Self, String> {
        let device = Device::system_default()
            .ok_or_else(|| "Metal device is not available on this Mac".to_string())?;
        let command_queue = device.new_command_queue();

        Ok(Self {
            device,
            command_queue,
            texture_count: model.textures.len(),
        })
    }

    pub fn render_probe(&self, runtime: &CubismModelRuntime) -> MetalRenderProbe {
        let drawables = runtime.drawables();
        let drawable_count = drawables.len();
        let triangle_count = drawables
            .iter()
            .map(|drawable| drawable.index_count.max(0) as usize / 3)
            .sum();

        MetalRenderProbe {
            device_name: self.device.name().to_string(),
            texture_count: self.texture_count,
            drawable_count,
            triangle_count,
            has_command_queue: !self.command_queue.as_ptr().is_null(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetalRenderProbe {
    pub device_name: String,
    pub texture_count: usize,
    pub drawable_count: usize,
    pub triangle_count: usize,
    pub has_command_queue: bool,
}

#[cfg(test)]
mod tests {
    use super::MetalRenderer;
    use crate::{cubism, live2d_model::Live2dModel};

    #[test]
    fn creates_metal_device_and_counts_drawables() {
        let model =
            Live2dModel::load("public/model/0.model3.json").expect("public model should load");
        let runtime = cubism::load_runtime(&model).expect("Cubism runtime should load");
        let renderer = MetalRenderer::load(&model).expect("Metal renderer should initialize");
        let probe = renderer.render_probe(&runtime);

        assert!(probe.has_command_queue);
        assert!(!probe.device_name.is_empty());
        assert_eq!(probe.texture_count, model.textures.len());
        assert!(probe.drawable_count > 0);
        assert!(probe.triangle_count > 0);
    }
}
