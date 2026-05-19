#![cfg(feature = "metal-renderer")]

use crate::config::RendererConfig;
use crate::cubism::{CubismBlendMode, CubismDrawableFrame, CubismDrawableInfo, CubismModelRuntime};
use crate::live2d_model::Live2dModel;
use core_graphics_types::geometry::CGSize;
use image::RgbaImage;
use metal::foreign_types::ForeignType;
use metal::{
    Buffer, CommandBufferRef, CommandQueue, CompileOptions, Device, Library, MTLBlendFactor,
    MTLBlendOperation, MTLClearColor, MTLCullMode, MTLIndexType, MTLLoadAction, MTLPixelFormat,
    MTLPrimitiveType, MTLRegion, MTLResourceOptions, MTLSamplerAddressMode, MTLSamplerMinMagFilter,
    MTLSamplerMipFilter, MTLScissorRect, MTLStoreAction, MTLTextureType, MTLTextureUsage,
    MTLViewport, MTLWinding, MetalLayer, NSRange, RenderPassDescriptor, RenderPipelineDescriptor,
    RenderPipelineState, SamplerDescriptor, SamplerState, Texture, TextureDescriptor,
};
use std::collections::HashMap;
use std::ffi::c_void;

const METAL_SHADER: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct Vertex {
    float2 position;
    float2 model_position;
    float2 uv;
};

struct VertexOut {
    float4 position [[position]];
    float2 uv;
    float2 model_position;
};

vertex VertexOut live2d_vertex(uint vertex_id [[vertex_id]],
                               constant Vertex *vertices [[buffer(0)]]) {
    VertexOut out;
    out.position = float4(vertices[vertex_id].position, 0.0, 1.0);
    out.uv = float2(vertices[vertex_id].uv.x, 1.0 - vertices[vertex_id].uv.y);
    out.model_position = vertices[vertex_id].model_position;
    return out;
}

struct MaskVertexParams {
    float2 mask_x;
    float2 mask_y;
};

float2 apply_affine(float2 x_axis, float2 y_axis, float2 position) {
    return float2(dot(x_axis, float2(position.x, 1.0)),
                  dot(y_axis, float2(position.y, 1.0)));
}

vertex VertexOut live2d_mask_vertex(uint vertex_id [[vertex_id]],
                                    constant Vertex *vertices [[buffer(0)]],
                                    constant MaskVertexParams &params [[buffer(1)]]) {
    VertexOut out;
    float2 clip_position = apply_affine(params.mask_x, params.mask_y, vertices[vertex_id].model_position);
    out.position = float4(clip_position, 0.0, 1.0);
    out.uv = float2(vertices[vertex_id].uv.x, 1.0 - vertices[vertex_id].uv.y);
    out.model_position = vertices[vertex_id].model_position;
    return out;
}

struct FragmentParams {
    float opacity;
    uint has_mask;
    uint inverted_mask;
    uint _padding;
    float2 draw_x;
    float2 draw_y;
    float4 layout_bounds;
    uint mask_channel_index;
    uint3 _padding2;
    float4 multiply_color;
    float4 screen_color;
};

float select_channel(float4 value, uint channel_index) {
    if (channel_index == 1) { return value.g; }
    if (channel_index == 2) { return value.b; }
    if (channel_index == 3) { return value.a; }
    return value.r;
}

fragment float4 live2d_fragment(VertexOut in [[stage_in]],
                                texture2d<float> atlas [[texture(0)]],
                                texture2d<float> mask_texture [[texture(1)]],
                                sampler atlas_sampler [[sampler(0)]],
                                sampler mask_sampler [[sampler(1)]],
                                constant FragmentParams &params [[buffer(0)]]) {
    float4 color = atlas.sample(atlas_sampler, in.uv);
    color.rgb = min(color.rgb * params.multiply_color.rgb + params.screen_color.rgb * color.a, 1.0);
    color.a *= params.opacity;
    if (params.has_mask != 0) {
        float2 mask_uv = apply_affine(params.draw_x, params.draw_y, in.model_position);
        float inside =
            step(params.layout_bounds.x, mask_uv.x) * step(params.layout_bounds.y, mask_uv.y) *
            step(mask_uv.x, params.layout_bounds.z) * step(mask_uv.y, params.layout_bounds.w);
        float4 mask_color = mask_texture.sample(mask_sampler, float2(mask_uv.x, 1.0 - mask_uv.y));
        float mask = select_channel(mask_color, params.mask_channel_index) * inside;
        if (params.inverted_mask != 0) {
            mask = 1.0 - mask;
        }
        color.a *= mask;
    }
    color.rgb *= color.a;
    return color;
}

struct MaskParams {
    float opacity;
    uint channel_index;
    uint2 _padding;
};

fragment float4 mask_fragment(VertexOut in [[stage_in]],
                              texture2d<float> atlas [[texture(0)]],
                              sampler atlas_sampler [[sampler(0)]],
                              constant MaskParams &params [[buffer(0)]]) {
    float alpha = atlas.sample(atlas_sampler, in.uv).a * params.opacity;
    if (params.channel_index == 1) { return float4(0.0, alpha, 0.0, 0.0); }
    if (params.channel_index == 2) { return float4(0.0, 0.0, alpha, 0.0); }
    if (params.channel_index == 3) { return float4(0.0, 0.0, 0.0, alpha); }
    return float4(alpha, 0.0, 0.0, 0.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct MetalVertex {
    position: [f32; 2],
    model_position: [f32; 2],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FragmentParams {
    opacity: f32,
    has_mask: u32,
    inverted_mask: u32,
    _padding: u32,
    draw_x: [f32; 2],
    draw_y: [f32; 2],
    layout_bounds: [f32; 4],
    mask_channel_index: u32,
    _padding2: [u32; 3],
    multiply_color: [f32; 4],
    screen_color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MaskParams {
    opacity: f32,
    channel_index: u32,
    _padding: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MaskVertexParams {
    mask_x: [f32; 2],
    mask_y: [f32; 2],
}

pub struct MetalRenderer {
    device: Device,
    command_queue: CommandQueue,
    layer: MetalLayer,
    normal_pipeline_state: RenderPipelineState,
    additive_pipeline_state: RenderPipelineState,
    multiplicative_pipeline_state: RenderPipelineState,
    mask_pipeline_state: RenderPipelineState,
    atlas_sampler: SamplerState,
    mask_sampler: SamplerState,
    textures: Vec<Texture>,
    white_mask_texture: Texture,
    mask_atlas_texture: Option<Texture>,
    mask_atlas_layout: Option<MaskAtlasLayout>,
    drawable_buffers: Vec<DrawableGpuBuffers>,
    vertex_ring: DynamicVertexRing,
    mask_tile_size: u64,
    drawable_size: f64,
    disable_masks: bool,
}

impl MetalRenderer {
    pub fn load(model: &Live2dModel, config: &RendererConfig) -> Result<Self, String> {
        let device = Device::system_default()
            .ok_or_else(|| "Metal device is not available on this Mac".to_string())?;
        let command_queue = device.new_command_queue();
        let library = compile_library(&device)?;
        let normal_pipeline_state =
            create_pipeline_state(&device, &library, PipelineBlendMode::Normal)?;
        let additive_pipeline_state =
            create_pipeline_state(&device, &library, PipelineBlendMode::Additive)?;
        let multiplicative_pipeline_state =
            create_pipeline_state(&device, &library, PipelineBlendMode::Multiplicative)?;
        let mask_pipeline_state = create_mask_pipeline_state(&device, &library)?;
        let atlas_sampler = create_atlas_sampler(&device);
        let mask_sampler = create_mask_sampler(&device);
        let textures = load_textures(&device, &command_queue, model)?;
        let white_mask_texture = create_white_mask_texture(&device);

        let layer = MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        layer.set_presents_with_transaction(false);
        layer.set_framebuffer_only(true);
        layer.set_opaque(false);
        layer.set_contents_scale(2.0);
        layer.set_drawable_size(CGSize::new(512.0, 512.0));

        Ok(Self {
            device,
            command_queue,
            layer,
            normal_pipeline_state,
            additive_pipeline_state,
            multiplicative_pipeline_state,
            mask_pipeline_state,
            atlas_sampler,
            mask_sampler,
            textures,
            white_mask_texture,
            mask_atlas_texture: None,
            mask_atlas_layout: None,
            drawable_buffers: Vec::new(),
            vertex_ring: DynamicVertexRing::new(),
            mask_tile_size: 512,
            drawable_size: 512.0,
            disable_masks: config.disable_masks,
        })
    }

    pub fn layer_ptr(&self) -> *mut c_void {
        self.layer.as_ptr().cast()
    }

    pub fn set_drawable_size(&mut self, width: f64, height: f64) {
        let size = width.max(1.0).min(height.max(1.0));
        self.drawable_size = size;
        let mask_tile_size = (size * 2.0).round().max(1.0) as u64;
        if self.mask_tile_size != mask_tile_size {
            self.mask_tile_size = mask_tile_size;
            self.mask_atlas_texture = None;
            self.mask_atlas_layout = None;
        }
        self.layer
            .set_drawable_size(CGSize::new(size * 2.0, size * 2.0));
    }

    pub fn render(&mut self, runtime: &CubismModelRuntime) -> Result<(), String> {
        let draw_items = collect_draw_items(runtime);
        let transform = bounds_for(&draw_items).and_then(|bounds| {
            bounds.fit_transform(
                self.drawable_size as f32,
                (self.drawable_size as f32) * 0.04,
            )
        });

        if let Some(transform) = transform {
            prepare_drawable_buffers(
                &self.device,
                &mut self.vertex_ring,
                &mut self.drawable_buffers,
                &draw_items,
                transform,
            );
            let mask_contexts = unique_mask_contexts(&draw_items, self.disable_masks);
            let mask_lookup = mask_set_lookup(&mask_contexts);
            let mask_layout =
                MaskAtlasLayout::for_mask_count(mask_contexts.len(), self.mask_tile_size);
            self.ensure_mask_atlas(&mask_layout);
            let vertex_buffer = self
                .vertex_ring
                .current_buffer()
                .ok_or_else(|| "Metal vertex ring did not allocate an active buffer".to_string())?;
            let command_buffer = self.command_queue.new_command_buffer();
            if !mask_contexts.is_empty() {
                render_mask_atlas(
                    command_buffer,
                    &self.mask_pipeline_state,
                    self.mask_atlas_texture
                        .as_ref()
                        .expect("mask atlas ensured before render"),
                    &mask_layout,
                    &draw_items,
                    &self.drawable_buffers,
                    &self.textures,
                    &self.atlas_sampler,
                    vertex_buffer,
                    &mask_contexts,
                )?;
            }

            let Some(drawable) = self.layer.next_drawable() else {
                return Ok(());
            };
            let render_pass_descriptor = RenderPassDescriptor::new();
            let color_attachment = render_pass_descriptor
                .color_attachments()
                .object_at(0)
                .ok_or_else(|| "Metal render pass has no color attachment".to_string())?;
            color_attachment.set_texture(Some(drawable.texture()));
            color_attachment.set_load_action(MTLLoadAction::Clear);
            color_attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
            color_attachment.set_store_action(MTLStoreAction::Store);

            let encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
            encoder.set_cull_mode(MTLCullMode::None);
            encoder.set_fragment_sampler_state(0, Some(&self.atlas_sampler));
            encoder.set_fragment_sampler_state(1, Some(&self.mask_sampler));
            let mut state_cache = MainPassStateCache::default();

            for item in draw_items {
                if !item.drawable.flags.visible || item.drawable.opacity <= 0.0 {
                    continue;
                }

                let Some(texture) = self
                    .textures
                    .get(item.drawable.texture_index.max(0) as usize)
                else {
                    continue;
                };

                let Some(buffers) = self.drawable_buffers.get(item.drawable.index) else {
                    continue;
                };
                if !buffers.is_ready() {
                    continue;
                }
                let mask_index = mask_lookup.get(&item.drawable.masks).copied();
                let mask_texture = mask_index
                    .and(self.mask_atlas_texture.as_ref())
                    .unwrap_or(&self.white_mask_texture);
                let mask_context = mask_index.and_then(|index| mask_contexts.get(index));
                let draw_matrix = mask_context
                    .map(|context| context.matrix_for_draw)
                    .unwrap_or_else(Affine2::identity);
                let layout_bounds = mask_context
                    .map(MaskContext::shader_layout_bounds)
                    .unwrap_or([0.0, 0.0, 1.0, 1.0]);
                let mask_channel = mask_context
                    .map(|context| context.channel.index())
                    .unwrap_or_else(|| MaskChannel::Red.index());
                let has_mask = mask_index
                    .and_then(|index| mask_contexts.get(index))
                    .is_some();
                let fragment_params = FragmentParams {
                    opacity: item.drawable.opacity.clamp(0.0, 1.0),
                    has_mask: u32::from(has_mask),
                    inverted_mask: u32::from(item.drawable.flags.inverted_mask),
                    _padding: 0,
                    draw_x: draw_matrix.x,
                    draw_y: draw_matrix.y,
                    layout_bounds,
                    mask_channel_index: mask_channel,
                    _padding2: [0; 3],
                    multiply_color: item.drawable.multiply_color,
                    screen_color: item.drawable.screen_color,
                };
                let cull_mode = drawable_cull_mode(&item.drawable);
                let front_winding = drawable_front_winding(&item.frame, transform);

                state_cache.bind_pipeline(
                    encoder,
                    item.drawable.blend_mode,
                    self.pipeline_state(item.drawable.blend_mode),
                );
                state_cache.bind_cull_state(encoder, cull_mode, front_winding);
                encoder.set_vertex_buffer(0, Some(vertex_buffer), buffers.vertex_offset);
                state_cache.bind_atlas_texture(encoder, texture);
                state_cache.bind_mask_texture(encoder, mask_texture);
                encoder.set_fragment_bytes(
                    0,
                    std::mem::size_of::<FragmentParams>() as u64,
                    (&raw const fragment_params).cast(),
                );
                encoder.draw_indexed_primitives(
                    MTLPrimitiveType::Triangle,
                    buffers.index_count as u64,
                    MTLIndexType::UInt16,
                    buffers.index_buffer.as_ref().expect("checked by is_ready"),
                    0,
                );
            }

            encoder.end_encoding();
            command_buffer.present_drawable(drawable);
            command_buffer.commit();
        } else {
            let Some(drawable) = self.layer.next_drawable() else {
                return Ok(());
            };
            let command_buffer = self.command_queue.new_command_buffer();
            let render_pass_descriptor = RenderPassDescriptor::new();
            let color_attachment = render_pass_descriptor
                .color_attachments()
                .object_at(0)
                .ok_or_else(|| "Metal render pass has no color attachment".to_string())?;
            color_attachment.set_texture(Some(drawable.texture()));
            color_attachment.set_load_action(MTLLoadAction::Clear);
            color_attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
            color_attachment.set_store_action(MTLStoreAction::Store);
            let encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
            encoder.end_encoding();
            command_buffer.present_drawable(drawable);
            command_buffer.commit();
        }
        Ok(())
    }

    pub fn render_probe(&self, runtime: &CubismModelRuntime) -> MetalRenderProbe {
        let drawables = runtime.drawables();
        let drawable_count = drawables.len();
        let triangle_count = drawables
            .iter()
            .map(|drawable| drawable.index_count.max(0) as usize / 3)
            .sum();
        let additive_count = drawables
            .iter()
            .filter(|drawable| drawable.blend_mode == CubismBlendMode::Additive)
            .count();
        let multiplicative_count = drawables
            .iter()
            .filter(|drawable| drawable.blend_mode == CubismBlendMode::Multiplicative)
            .count();
        let masked_count = drawables
            .iter()
            .filter(|drawable| !drawable.masks.is_empty())
            .count();

        MetalRenderProbe {
            device_name: self.device.name().to_string(),
            texture_count: self.textures.len(),
            drawable_count,
            triangle_count,
            additive_count,
            multiplicative_count,
            masked_count,
            has_command_queue: !self.command_queue.as_ptr().is_null(),
        }
    }

    fn pipeline_state(&self, blend_mode: CubismBlendMode) -> &RenderPipelineState {
        match blend_mode {
            CubismBlendMode::Additive => &self.additive_pipeline_state,
            CubismBlendMode::Multiplicative => &self.multiplicative_pipeline_state,
            CubismBlendMode::Normal | CubismBlendMode::Unknown(_) => &self.normal_pipeline_state,
        }
    }

    fn ensure_mask_atlas(&mut self, layout: &MaskAtlasLayout) {
        if layout.mask_count == 0 {
            return;
        }

        let needs_new_texture = self
            .mask_atlas_layout
            .as_ref()
            .is_none_or(|current| current.texture_size != layout.texture_size);
        if needs_new_texture {
            self.mask_atlas_texture = Some(create_mask_texture(&self.device, layout.texture_size));
        }
        self.mask_atlas_layout = Some(*layout);
    }
}

#[derive(Debug, Clone)]
pub struct MetalRenderProbe {
    pub device_name: String,
    pub texture_count: usize,
    pub drawable_count: usize,
    pub triangle_count: usize,
    pub additive_count: usize,
    pub multiplicative_count: usize,
    pub masked_count: usize,
    pub has_command_queue: bool,
}

struct DrawItem {
    drawable: CubismDrawableInfo,
    frame: CubismDrawableFrame,
}

struct MaskContext {
    masks: Vec<i32>,
    bounds: Bounds,
    channel: MaskChannel,
    layout_bounds: LayoutBounds,
    matrix_for_mask: Affine2,
    matrix_for_draw: Affine2,
}

impl MaskContext {
    fn shader_layout_bounds(&self) -> [f32; 4] {
        [
            self.layout_bounds.x,
            self.layout_bounds.y,
            self.layout_bounds.right(),
            self.layout_bounds.bottom(),
        ]
    }

    fn rebuild_matrices(&mut self) {
        self.matrix_for_mask = Affine2::for_mask(self.bounds, self.layout_bounds);
        self.matrix_for_draw = Affine2::for_draw(self.bounds, self.layout_bounds);
    }
}

#[derive(Clone, Copy)]
enum MaskChannel {
    Red,
    Green,
    Blue,
    Alpha,
}

impl MaskChannel {
    fn from_index(index: usize) -> Self {
        match index {
            0 => Self::Red,
            1 => Self::Green,
            2 => Self::Blue,
            _ => Self::Alpha,
        }
    }

    fn index(self) -> u32 {
        match self {
            Self::Red => 0,
            Self::Green => 1,
            Self::Blue => 2,
            Self::Alpha => 3,
        }
    }
}

#[derive(Clone, Copy)]
struct LayoutBounds {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl LayoutBounds {
    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }
}

#[derive(Clone, Copy)]
struct Affine2 {
    x: [f32; 2],
    y: [f32; 2],
}

impl Affine2 {
    fn identity() -> Self {
        Self {
            x: [1.0, 0.0],
            y: [1.0, 0.0],
        }
    }

    fn for_mask(bounds: Bounds, layout: LayoutBounds) -> Self {
        let draw = Self::for_draw(bounds, layout);
        Self {
            x: [draw.x[0] * 2.0, draw.x[1] * 2.0 - 1.0],
            y: [draw.y[0] * 2.0, draw.y[1] * 2.0 - 1.0],
        }
    }

    fn for_draw(bounds: Bounds, layout: LayoutBounds) -> Self {
        let width = bounds.width().max(f32::EPSILON);
        let height = bounds.height().max(f32::EPSILON);
        let scale_x = layout.width / width;
        let scale_y = layout.height / height;
        Self {
            x: [scale_x, layout.x - bounds.min_x * scale_x],
            y: [scale_y, layout.y - bounds.min_y * scale_y],
        }
    }
}

#[derive(Default)]
struct DrawableGpuBuffers {
    index_buffer: Option<Buffer>,
    index_capacity: usize,
    index_cache: Vec<u16>,
    vertex_offset: u64,
    vertex_count: usize,
    index_count: usize,
}

#[derive(Clone, Copy)]
struct MaskAtlasLayout {
    mask_count: usize,
    texture_size: u64,
}

impl MaskAtlasLayout {
    fn for_mask_count(mask_count: usize, tile_size: u64) -> Self {
        if mask_count == 0 {
            return Self {
                mask_count,
                texture_size: tile_size.max(1),
            };
        }

        Self {
            mask_count,
            texture_size: tile_size.max(1),
        }
    }
}

impl DrawableGpuBuffers {
    fn is_ready(&self) -> bool {
        self.index_buffer.is_some() && self.vertex_count > 0 && self.index_count > 0
    }
}

struct DynamicVertexRing {
    buffers: Vec<Buffer>,
    capacity: usize,
    active_index: usize,
    write_offset: usize,
}

impl DynamicVertexRing {
    const BUFFER_COUNT: usize = 3;
    const ALIGNMENT: usize = 256;

    fn new() -> Self {
        Self {
            buffers: Vec::new(),
            capacity: 0,
            active_index: 0,
            write_offset: 0,
        }
    }

    fn begin_frame(&mut self, device: &Device, required_bytes: usize) {
        let required_bytes = align_up(required_bytes.max(1), Self::ALIGNMENT);
        if self.buffers.len() != Self::BUFFER_COUNT || self.capacity < required_bytes {
            self.capacity = required_bytes.next_power_of_two();
            self.buffers = (0..Self::BUFFER_COUNT)
                .map(|_| {
                    device.new_buffer(
                        self.capacity as u64,
                        MTLResourceOptions::CPUCacheModeDefaultCache
                            | MTLResourceOptions::StorageModeShared,
                    )
                })
                .collect();
            self.active_index = 0;
        } else {
            self.active_index = (self.active_index + 1) % self.buffers.len();
        }
        self.write_offset = 0;
    }

    fn write_vertices(&mut self, vertices: &[MetalVertex]) -> Option<u64> {
        let byte_len = std::mem::size_of_val(vertices);
        if byte_len == 0 || self.buffers.is_empty() {
            return None;
        }

        let offset = align_up(self.write_offset, Self::ALIGNMENT);
        if offset + byte_len > self.capacity {
            return None;
        }

        let buffer = &self.buffers[self.active_index];
        unsafe {
            std::ptr::copy_nonoverlapping(
                vertices.as_ptr().cast::<u8>(),
                buffer.contents().cast::<u8>().add(offset),
                byte_len,
            );
        }
        buffer.did_modify_range(NSRange::new(offset as u64, byte_len as u64));
        self.write_offset = offset + byte_len;
        Some(offset as u64)
    }

    fn current_buffer(&self) -> Option<&metal::BufferRef> {
        self.buffers.get(self.active_index).map(|buffer| &**buffer)
    }
}

#[derive(Default)]
struct MainPassStateCache {
    blend_mode: Option<CubismBlendMode>,
    cull_mode: Option<MTLCullMode>,
    front_winding: Option<MTLWinding>,
    atlas_texture: Option<*mut c_void>,
    mask_texture: Option<*mut c_void>,
}

impl MainPassStateCache {
    fn bind_pipeline(
        &mut self,
        encoder: &metal::RenderCommandEncoderRef,
        blend_mode: CubismBlendMode,
        pipeline_state: &RenderPipelineState,
    ) {
        if self.blend_mode == Some(blend_mode) {
            return;
        }

        encoder.set_render_pipeline_state(pipeline_state);
        self.blend_mode = Some(blend_mode);
    }

    fn bind_cull_state(
        &mut self,
        encoder: &metal::RenderCommandEncoderRef,
        cull_mode: MTLCullMode,
        front_winding: MTLWinding,
    ) {
        if self.front_winding != Some(front_winding) {
            encoder.set_front_facing_winding(front_winding);
            self.front_winding = Some(front_winding);
        }

        if self.cull_mode != Some(cull_mode) {
            encoder.set_cull_mode(cull_mode);
            self.cull_mode = Some(cull_mode);
        }
    }

    fn bind_atlas_texture(&mut self, encoder: &metal::RenderCommandEncoderRef, texture: &Texture) {
        let ptr = texture.as_ptr().cast::<c_void>();
        if self.atlas_texture == Some(ptr) {
            return;
        }

        encoder.set_fragment_texture(0, Some(texture));
        self.atlas_texture = Some(ptr);
    }

    fn bind_mask_texture(&mut self, encoder: &metal::RenderCommandEncoderRef, texture: &Texture) {
        let ptr = texture.as_ptr().cast::<c_void>();
        if self.mask_texture == Some(ptr) {
            return;
        }

        encoder.set_fragment_texture(1, Some(texture));
        self.mask_texture = Some(ptr);
    }
}

#[derive(Clone, Copy)]
struct FitTransform {
    min_x: f32,
    min_y: f32,
    scale: f32,
    offset_x: f32,
    offset_y: f32,
    output_size: f32,
}

impl FitTransform {
    fn identity() -> Self {
        Self {
            min_x: 0.0,
            min_y: 0.0,
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            output_size: 2.0,
        }
    }

    fn ndc_position(self, position: [f32; 2]) -> [f32; 2] {
        let x = self.offset_x + (position[0] - self.min_x) * self.scale;
        let y = self.output_size - (self.offset_y + (position[1] - self.min_y) * self.scale);
        [
            (x / self.output_size) * 2.0 - 1.0,
            1.0 - (y / self.output_size) * 2.0,
        ]
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

#[derive(Clone, Copy)]
enum PipelineBlendMode {
    Normal,
    Additive,
    Multiplicative,
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

    fn unit() -> Self {
        Self {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 1.0,
            max_y: 1.0,
        }
    }

    fn include(&mut self, position: [f32; 2]) {
        self.min_x = self.min_x.min(position[0]);
        self.min_y = self.min_y.min(position[1]);
        self.max_x = self.max_x.max(position[0]);
        self.max_y = self.max_y.max(position[1]);
    }

    fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    fn height(&self) -> f32 {
        self.max_y - self.min_y
    }

    fn expanded_by_fraction(&self, fraction: f32) -> Self {
        let expand_x = self.width().max(0.0) * fraction;
        let expand_y = self.height().max(0.0) * fraction;
        Self {
            min_x: self.min_x - expand_x,
            min_y: self.min_y - expand_y,
            max_x: self.max_x + expand_x,
            max_y: self.max_y + expand_y,
        }
    }

    fn fit_transform(&self, output_size: f32, padding: f32) -> Option<FitTransform> {
        if !self.min_x.is_finite() {
            return None;
        }

        let width = self.width();
        let height = self.height();
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
            output_size,
        })
    }
}

fn compile_library(device: &Device) -> Result<Library, String> {
    let options = CompileOptions::new();
    options.set_fast_math_enabled(true);
    device
        .new_library_with_source(METAL_SHADER, &options)
        .map_err(|error| format!("Failed to compile Metal shader: {error}"))
}

fn create_pipeline_state(
    device: &Device,
    library: &Library,
    blend_mode: PipelineBlendMode,
) -> Result<RenderPipelineState, String> {
    let vertex = library
        .get_function("live2d_vertex", None)
        .map_err(|error| format!("Failed to load Metal vertex shader: {error}"))?;
    let fragment = library
        .get_function("live2d_fragment", None)
        .map_err(|error| format!("Failed to load Metal fragment shader: {error}"))?;

    let descriptor = RenderPipelineDescriptor::new();
    descriptor.set_vertex_function(Some(&vertex));
    descriptor.set_fragment_function(Some(&fragment));
    let attachment = descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| "Metal pipeline has no color attachment".to_string())?;
    attachment.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    attachment.set_blending_enabled(true);
    attachment.set_rgb_blend_operation(MTLBlendOperation::Add);
    attachment.set_alpha_blend_operation(MTLBlendOperation::Add);
    match blend_mode {
        PipelineBlendMode::Normal => {
            attachment.set_source_rgb_blend_factor(MTLBlendFactor::One);
            attachment.set_source_alpha_blend_factor(MTLBlendFactor::One);
            attachment.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
            attachment.set_destination_alpha_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
        }
        PipelineBlendMode::Additive => {
            attachment.set_source_rgb_blend_factor(MTLBlendFactor::One);
            attachment.set_source_alpha_blend_factor(MTLBlendFactor::Zero);
            attachment.set_destination_rgb_blend_factor(MTLBlendFactor::One);
            attachment.set_destination_alpha_blend_factor(MTLBlendFactor::One);
        }
        PipelineBlendMode::Multiplicative => {
            attachment.set_source_rgb_blend_factor(MTLBlendFactor::DestinationColor);
            attachment.set_source_alpha_blend_factor(MTLBlendFactor::Zero);
            attachment.set_destination_rgb_blend_factor(MTLBlendFactor::OneMinusSourceAlpha);
            attachment.set_destination_alpha_blend_factor(MTLBlendFactor::One);
        }
    }

    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| format!("Failed to create Metal render pipeline: {error}"))
}

fn create_mask_pipeline_state(
    device: &Device,
    library: &Library,
) -> Result<RenderPipelineState, String> {
    let vertex = library
        .get_function("live2d_mask_vertex", None)
        .map_err(|error| format!("Failed to load Metal mask vertex shader: {error}"))?;
    let fragment = library
        .get_function("mask_fragment", None)
        .map_err(|error| format!("Failed to load Metal mask fragment shader: {error}"))?;

    let descriptor = RenderPipelineDescriptor::new();
    descriptor.set_vertex_function(Some(&vertex));
    descriptor.set_fragment_function(Some(&fragment));
    let attachment = descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| "Metal mask pipeline has no color attachment".to_string())?;
    attachment.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
    attachment.set_blending_enabled(true);
    attachment.set_rgb_blend_operation(MTLBlendOperation::Add);
    attachment.set_alpha_blend_operation(MTLBlendOperation::Add);
    attachment.set_source_rgb_blend_factor(MTLBlendFactor::One);
    attachment.set_source_alpha_blend_factor(MTLBlendFactor::One);
    attachment.set_destination_rgb_blend_factor(MTLBlendFactor::One);
    attachment.set_destination_alpha_blend_factor(MTLBlendFactor::One);

    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| format!("Failed to create Metal mask pipeline: {error}"))
}

fn create_atlas_sampler(device: &Device) -> SamplerState {
    let descriptor = SamplerDescriptor::new();
    descriptor.set_min_filter(MTLSamplerMinMagFilter::Linear);
    descriptor.set_mag_filter(MTLSamplerMinMagFilter::Linear);
    descriptor.set_mip_filter(MTLSamplerMipFilter::Linear);
    descriptor.set_address_mode_s(MTLSamplerAddressMode::ClampToEdge);
    descriptor.set_address_mode_t(MTLSamplerAddressMode::ClampToEdge);
    descriptor.set_max_anisotropy(8);
    device.new_sampler(&descriptor)
}

fn create_mask_sampler(device: &Device) -> SamplerState {
    let descriptor = SamplerDescriptor::new();
    descriptor.set_min_filter(MTLSamplerMinMagFilter::Linear);
    descriptor.set_mag_filter(MTLSamplerMinMagFilter::Linear);
    descriptor.set_mip_filter(MTLSamplerMipFilter::NotMipmapped);
    descriptor.set_address_mode_s(MTLSamplerAddressMode::ClampToEdge);
    descriptor.set_address_mode_t(MTLSamplerAddressMode::ClampToEdge);
    device.new_sampler(&descriptor)
}

fn load_textures(
    device: &Device,
    command_queue: &CommandQueue,
    model: &Live2dModel,
) -> Result<Vec<Texture>, String> {
    let mut textures = Vec::with_capacity(model.textures.len());
    for texture_path in &model.textures {
        let image = image::open(texture_path)
            .map_err(|error| format!("Failed to load texture {}: {error}", texture_path.display()))?
            .to_rgba8();
        textures.push(upload_texture(device, command_queue, &image));
    }
    Ok(textures)
}

fn create_mask_texture(device: &Device, size: u64) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
    descriptor.set_width(size);
    descriptor.set_height(size);
    descriptor.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    descriptor.set_resource_options(
        MTLResourceOptions::CPUCacheModeDefaultCache | MTLResourceOptions::StorageModePrivate,
    );
    device.new_texture(&descriptor)
}

fn create_white_mask_texture(device: &Device) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
    descriptor.set_width(1);
    descriptor.set_height(1);
    descriptor.set_usage(MTLTextureUsage::ShaderRead);
    descriptor.set_resource_options(
        MTLResourceOptions::CPUCacheModeDefaultCache | MTLResourceOptions::StorageModeShared,
    );

    let texture = device.new_texture(&descriptor);
    let white = [255_u8, 255, 255, 255];
    texture.replace_region(MTLRegion::new_2d(0, 0, 1, 1), 0, white.as_ptr().cast(), 4);
    texture
}

fn upload_texture(device: &Device, command_queue: &CommandQueue, image: &RgbaImage) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
    descriptor.set_width(image.width() as u64);
    descriptor.set_height(image.height() as u64);
    descriptor.set_mipmap_level_count(mipmap_level_count(image.width(), image.height()));
    descriptor.set_usage(MTLTextureUsage::ShaderRead);
    descriptor.set_resource_options(
        MTLResourceOptions::CPUCacheModeDefaultCache | MTLResourceOptions::StorageModeShared,
    );

    let texture = device.new_texture(&descriptor);
    texture.replace_region(
        MTLRegion::new_2d(0, 0, image.width() as u64, image.height() as u64),
        0,
        image.as_raw().as_ptr().cast(),
        (image.width() * 4) as u64,
    );
    generate_mipmaps(command_queue, &texture);
    texture
}

fn mipmap_level_count(width: u32, height: u32) -> u64 {
    let max_dimension = width.max(height).max(1);
    (u32::BITS - max_dimension.leading_zeros()) as u64
}

fn generate_mipmaps(command_queue: &CommandQueue, texture: &Texture) {
    if texture.mipmap_level_count() <= 1 {
        return;
    }

    let command_buffer = command_queue.new_command_buffer();
    let encoder = command_buffer.new_blit_command_encoder();
    encoder.generate_mipmaps(texture);
    encoder.end_encoding();
    command_buffer.commit();
    command_buffer.wait_until_completed();
}

fn prepare_drawable_buffers(
    device: &Device,
    vertex_ring: &mut DynamicVertexRing,
    drawable_buffers: &mut Vec<DrawableGpuBuffers>,
    draw_items: &[DrawItem],
    transform: FitTransform,
) {
    let max_index = draw_items
        .iter()
        .map(|item| item.drawable.index)
        .max()
        .unwrap_or(0);
    if drawable_buffers.len() <= max_index {
        drawable_buffers.resize_with(max_index + 1, DrawableGpuBuffers::default);
    }

    let required_vertex_bytes = draw_items
        .iter()
        .map(|item| {
            align_up(
                item.frame.positions.len() * std::mem::size_of::<MetalVertex>(),
                DynamicVertexRing::ALIGNMENT,
            )
        })
        .sum();
    vertex_ring.begin_frame(device, required_vertex_bytes);

    for item in draw_items {
        let vertices = metal_vertices(&item.frame, transform);
        let buffer = &mut drawable_buffers[item.drawable.index];
        buffer.vertex_count = vertices.len();
        buffer.index_count = item.frame.indices.len();

        if vertices.is_empty() || item.frame.indices.is_empty() {
            continue;
        }

        if let Some(vertex_offset) = vertex_ring.write_vertices(&vertices) {
            buffer.vertex_offset = vertex_offset;
        } else {
            buffer.vertex_count = 0;
        }
        update_index_buffer(device, buffer, &item.frame.indices);
    }
}

fn update_index_buffer(device: &Device, buffers: &mut DrawableGpuBuffers, indices: &[u16]) {
    if buffers.index_cache == indices && buffers.index_buffer.is_some() {
        return;
    }

    let byte_len = std::mem::size_of_val(indices);
    if buffers.index_buffer.is_none() || buffers.index_capacity < byte_len {
        buffers.index_buffer = Some(device.new_buffer(
            byte_len as u64,
            MTLResourceOptions::CPUCacheModeDefaultCache | MTLResourceOptions::StorageModeShared,
        ));
        buffers.index_capacity = byte_len;
    }

    let Some(index_buffer) = buffers.index_buffer.as_ref() else {
        return;
    };
    unsafe {
        std::ptr::copy_nonoverlapping(
            indices.as_ptr().cast::<u8>(),
            index_buffer.contents().cast::<u8>(),
            byte_len,
        );
    }
    index_buffer.did_modify_range(NSRange::new(0, byte_len as u64));
    buffers.index_cache.clear();
    buffers.index_cache.extend_from_slice(indices);
}

fn render_mask_atlas(
    command_buffer: &CommandBufferRef,
    mask_pipeline_state: &RenderPipelineState,
    mask_atlas_texture: &Texture,
    layout: &MaskAtlasLayout,
    draw_items: &[DrawItem],
    drawable_buffers: &[DrawableGpuBuffers],
    textures: &[Texture],
    atlas_sampler: &SamplerState,
    vertex_buffer: &metal::BufferRef,
    mask_contexts: &[MaskContext],
) -> Result<(), String> {
    let render_pass_descriptor = RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| "Metal mask render pass has no color attachment".to_string())?;
    color_attachment.set_texture(Some(mask_atlas_texture));
    color_attachment.set_load_action(MTLLoadAction::Clear);
    color_attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
    color_attachment.set_store_action(MTLStoreAction::Store);

    let encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    encoder.set_render_pipeline_state(mask_pipeline_state);
    encoder.set_cull_mode(MTLCullMode::None);
    encoder.set_fragment_sampler_state(0, Some(atlas_sampler));
    let mut cull_mode = None;
    let mut front_winding = None;

    for context in mask_contexts {
        let mask_vertex_params = MaskVertexParams {
            mask_x: context.matrix_for_mask.x,
            mask_y: context.matrix_for_mask.y,
        };
        encoder.set_viewport(MTLViewport {
            originX: 0.0,
            originY: 0.0,
            width: layout.texture_size as f64,
            height: layout.texture_size as f64,
            znear: 0.0,
            zfar: 1.0,
        });
        encoder.set_scissor_rect(MTLScissorRect {
            x: 0,
            y: 0,
            width: layout.texture_size,
            height: layout.texture_size,
        });
        encoder.set_vertex_bytes(
            1,
            std::mem::size_of::<MaskVertexParams>() as u64,
            (&raw const mask_vertex_params).cast(),
        );

        for mask_drawable_index in &context.masks {
            let Some(item) = draw_items
                .iter()
                .find(|item| item.drawable.index == *mask_drawable_index as usize)
            else {
                continue;
            };
            if !item.drawable.flags.visible {
                continue;
            }

            let Some(buffers) = drawable_buffers.get(item.drawable.index) else {
                continue;
            };
            if !buffers.is_ready() {
                continue;
            }

            let Some(texture) = textures.get(item.drawable.texture_index.max(0) as usize) else {
                continue;
            };
            let mask_params = MaskParams {
                opacity: item.drawable.opacity.clamp(0.0, 1.0),
                channel_index: context.channel.index(),
                _padding: [0; 2],
            };
            let next_cull_mode = drawable_cull_mode(&item.drawable);
            let next_front_winding = drawable_front_winding(&item.frame, FitTransform::identity());
            if front_winding != Some(next_front_winding) {
                encoder.set_front_facing_winding(next_front_winding);
                front_winding = Some(next_front_winding);
            }
            if cull_mode != Some(next_cull_mode) {
                encoder.set_cull_mode(next_cull_mode);
                cull_mode = Some(next_cull_mode);
            }

            encoder.set_vertex_buffer(0, Some(vertex_buffer), buffers.vertex_offset);
            encoder.set_fragment_texture(0, Some(texture));
            encoder.set_fragment_bytes(
                0,
                std::mem::size_of::<MaskParams>() as u64,
                (&raw const mask_params).cast(),
            );
            encoder.draw_indexed_primitives(
                MTLPrimitiveType::Triangle,
                buffers.index_count as u64,
                MTLIndexType::UInt16,
                buffers.index_buffer.as_ref().expect("checked by is_ready"),
                0,
            );
        }
    }

    encoder.end_encoding();
    Ok(())
}

fn collect_draw_items(runtime: &CubismModelRuntime) -> Vec<DrawItem> {
    let mut drawables = runtime.drawables();
    drawables.sort_by_key(|drawable| drawable.render_order);

    drawables
        .into_iter()
        .filter_map(|drawable| {
            runtime
                .drawable_frame_by_index(drawable.index)
                .map(|frame| DrawItem { drawable, frame })
        })
        .collect()
}

fn unique_mask_contexts(items: &[DrawItem], disable_masks: bool) -> Vec<MaskContext> {
    if disable_masks {
        return Vec::new();
    }

    let mut mask_contexts: Vec<MaskContext> = Vec::new();
    for item in items {
        if item.drawable.masks.is_empty() {
            continue;
        }

        if !mask_contexts
            .iter()
            .any(|context| context.masks.as_slice() == item.drawable.masks.as_slice())
        {
            let bounds = clipped_bounds_for_mask(items, &item.drawable.masks)
                .unwrap_or_else(|| bounds_for(items).unwrap_or_else(Bounds::unit))
                .expanded_by_fraction(0.05);
            mask_contexts.push(MaskContext {
                masks: item.drawable.masks.clone(),
                bounds,
                channel: MaskChannel::Red,
                layout_bounds: LayoutBounds {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                matrix_for_mask: Affine2::identity(),
                matrix_for_draw: Affine2::identity(),
            });
        }
    }
    assign_mask_layouts(&mut mask_contexts);
    mask_contexts
}

fn assign_mask_layouts(mask_contexts: &mut [MaskContext]) {
    let count = mask_contexts.len();
    if count == 0 {
        return;
    }

    if count > 36 {
        for context in mask_contexts {
            context.channel = MaskChannel::Red;
            context.layout_bounds = LayoutBounds {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            };
            context.rebuild_matrices();
        }
        return;
    }

    let div_count = count / 4;
    let mod_count = count % 4;
    let mut cursor = 0;

    for channel_index in 0..4 {
        let layout_count = div_count + usize::from(channel_index < mod_count);
        for slot in 0..layout_count {
            let Some(context) = mask_contexts.get_mut(cursor) else {
                return;
            };
            context.channel = MaskChannel::from_index(channel_index);
            context.layout_bounds = layout_bounds_for_slot(slot, layout_count);
            context.rebuild_matrices();
            cursor += 1;
        }
    }
}

fn layout_bounds_for_slot(slot: usize, layout_count: usize) -> LayoutBounds {
    if layout_count <= 1 {
        return LayoutBounds {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
    }

    if layout_count == 2 {
        return LayoutBounds {
            x: (slot % 2) as f32 * 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
    }

    if layout_count <= 4 {
        return LayoutBounds {
            x: (slot % 2) as f32 * 0.5,
            y: (slot / 2) as f32 * 0.5,
            width: 0.5,
            height: 0.5,
        };
    }

    LayoutBounds {
        x: (slot % 3) as f32 / 3.0,
        y: (slot / 3) as f32 / 3.0,
        width: 1.0 / 3.0,
        height: 1.0 / 3.0,
    }
}

fn mask_set_lookup(mask_contexts: &[MaskContext]) -> HashMap<Vec<i32>, usize> {
    mask_contexts
        .iter()
        .map(|context| context.masks.clone())
        .enumerate()
        .map(|(index, masks)| (masks, index))
        .collect()
}

fn clipped_bounds_for_mask(items: &[DrawItem], masks: &[i32]) -> Option<Bounds> {
    let mut bounds = Bounds::empty();
    for item in items {
        if item.drawable.masks.as_slice() != masks {
            continue;
        }
        if !item.drawable.flags.visible || item.drawable.opacity <= 0.0 {
            continue;
        }
        for position in &item.frame.positions {
            bounds.include(*position);
        }
    }

    if bounds.min_x.is_finite() {
        Some(bounds)
    } else {
        None
    }
}

fn align_up(value: usize, alignment: usize) -> usize {
    debug_assert!(alignment.is_power_of_two());
    (value + alignment - 1) & !(alignment - 1)
}

fn drawable_cull_mode(drawable: &CubismDrawableInfo) -> MTLCullMode {
    if drawable.flags.double_sided {
        MTLCullMode::None
    } else {
        MTLCullMode::Back
    }
}

fn drawable_front_winding(frame: &CubismDrawableFrame, transform: FitTransform) -> MTLWinding {
    for triangle in frame.indices.chunks_exact(3) {
        let Some(a) = frame.positions.get(triangle[0] as usize) else {
            continue;
        };
        let Some(b) = frame.positions.get(triangle[1] as usize) else {
            continue;
        };
        let Some(c) = frame.positions.get(triangle[2] as usize) else {
            continue;
        };

        let a = transform.ndc_position(*a);
        let b = transform.ndc_position(*b);
        let c = transform.ndc_position(*c);
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area > f32::EPSILON {
            return MTLWinding::CounterClockwise;
        }
        if area < -f32::EPSILON {
            return MTLWinding::Clockwise;
        }
    }

    MTLWinding::CounterClockwise
}

fn bounds_for(items: &[DrawItem]) -> Option<Bounds> {
    let mut bounds = Bounds::empty();
    for item in items {
        if !item.drawable.flags.visible || item.drawable.opacity <= 0.0 {
            continue;
        }
        for position in &item.frame.positions {
            bounds.include(*position);
        }
    }

    if bounds.min_x.is_finite() {
        Some(bounds)
    } else {
        None
    }
}

fn metal_vertices(frame: &CubismDrawableFrame, transform: FitTransform) -> Vec<MetalVertex> {
    frame
        .positions
        .iter()
        .zip(&frame.uvs)
        .map(|(position, uv)| MetalVertex {
            position: transform.ndc_position(*position),
            model_position: *position,
            uv: *uv,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::MetalRenderer;
    use crate::{config::RendererConfig, cubism, live2d_model::Live2dModel};

    #[test]
    fn creates_metal_device_and_counts_drawables() {
        let model =
            Live2dModel::load("public/model/0.model3.json").expect("public model should load");
        let runtime = cubism::load_runtime(&model).expect("Cubism runtime should load");
        let config = RendererConfig::default();
        let renderer =
            MetalRenderer::load(&model, &config).expect("Metal renderer should initialize");
        let probe = renderer.render_probe(&runtime);

        assert!(probe.has_command_queue);
        assert!(!probe.device_name.is_empty());
        assert_eq!(probe.texture_count, model.textures.len());
        assert!(probe.drawable_count > 0);
        assert!(probe.triangle_count > 0);
        assert!(probe.additive_count + probe.multiplicative_count <= probe.drawable_count);
        assert!(probe.masked_count <= probe.drawable_count);
    }
}
