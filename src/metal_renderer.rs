#![cfg(feature = "metal-renderer")]

use crate::config::RendererConfig;
use crate::cubism::{
    CubismBlendMode, CubismDrawableFrame, CubismDrawableInfo, CubismModelRuntime,
    CubismOffscreenInfo, CubismPartInfo,
};
use crate::live2d_model::Live2dModel;
use core_graphics_types::geometry::CGSize;
use image::RgbaImage;
use metal::foreign_types::ForeignType;
use metal::{
    Buffer, CommandBufferRef, CommandQueue, CompileOptions, Device, Library, MTLBlendFactor,
    MTLBlendOperation, MTLClearColor, MTLCullMode, MTLIndexType, MTLLoadAction, MTLOrigin,
    MTLPixelFormat, MTLPrimitiveType, MTLRegion, MTLResourceOptions, MTLSamplerAddressMode,
    MTLSamplerMinMagFilter, MTLSamplerMipFilter, MTLScissorRect, MTLSize, MTLStoreAction,
    MTLTextureType, MTLTextureUsage, MTLViewport, MTLWinding, MetalLayer, NSRange,
    RenderPassDescriptor, RenderPipelineDescriptor, RenderPipelineState, SamplerDescriptor,
    SamplerState, Texture, TextureDescriptor,
};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;

const DEFAULT_CONTENTS_SCALE: f64 = 2.0;
const MIN_MASK_TEXTURE_SIZE: u64 = 512;
const MAX_MASK_TEXTURE_SIZE: u64 = 2048;
const DEFAULT_MASK_CONTEXTS_PER_TEXTURE: usize = 36;
const MULTI_MASK_CONTEXTS_PER_TEXTURE: usize = 32;
const PREFERRED_MSAA_SAMPLE_COUNT: u64 = 4;

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
    float2 target_uv;
};

vertex VertexOut live2d_vertex(uint vertex_id [[vertex_id]],
                               constant Vertex *vertices [[buffer(0)]]) {
    VertexOut out;
    out.position = float4(vertices[vertex_id].position, 0.0, 1.0);
    out.uv = float2(vertices[vertex_id].uv.x, 1.0 - vertices[vertex_id].uv.y);
    out.model_position = vertices[vertex_id].model_position;
    out.target_uv = float2(out.position.x * 0.5 + 0.5, 0.5 - out.position.y * 0.5);
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
    out.target_uv = float2(out.position.x * 0.5 + 0.5, 0.5 - out.position.y * 0.5);
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
    uint _padding2_0;
    uint _padding2_1;
    uint _padding2_2;
    float4 multiply_color;
    float4 screen_color;
    uint color_blend;
    uint alpha_blend;
    uint _padding3_0;
    uint _padding3_1;
    uint debug_mode;
    uint _padding4_0;
    uint _padding4_1;
    uint _padding4_2;
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
    if (params.debug_mode == 1) { return float4(in.uv.x, in.uv.y, 0.0, 1.0); }
    if (params.debug_mode == 2) { return float4(color.rgb, 1.0); }
    if (params.debug_mode == 3) { return float4(color.a, color.a, color.a, 1.0); }
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

float3 extended_color_blend(float3 source, float3 destination, uint mode) {
    if (mode == 1 || mode == 3) { return min(source + destination, 1.0); }
    if (mode == 2 || mode == 6) { return source * destination; }
    if (mode == 5) { return min(source, destination); }
    if (mode == 9) { return max(source, destination); }
    if (mode == 10) { return source + destination - source * destination; }
    return source;
}

float4 overlap_rgba(float3 color, float3 source, float3 destination, float3 parameter) {
    return float4(
        color * parameter.x + source * parameter.y + destination * parameter.z,
        parameter.x + parameter.y + parameter.z
    );
}

float4 extended_alpha_blend(float3 color, float4 source, float4 destination, uint mode) {
    if (mode == 1) {
        float3 parameter = float3(source.a * destination.a, 0.0, destination.a * (1.0 - source.a));
        return overlap_rgba(color, source.rgb, destination.rgb, parameter);
    }
    if (mode == 2) {
        float3 parameter = float3(0.0, 0.0, destination.a * (1.0 - source.a));
        return overlap_rgba(color, source.rgb, destination.rgb, parameter);
    }
    if (mode == 3) {
        float3 parameter = float3(min(source.a, destination.a), max(source.a - destination.a, 0.0), max(destination.a - source.a, 0.0));
        return overlap_rgba(color, source.rgb, destination.rgb, parameter);
    }
    if (mode == 4) {
        float3 parameter = float3(max(source.a + destination.a - 1.0, 0.0), min(source.a, 1.0 - destination.a), min(destination.a, 1.0 - source.a));
        return overlap_rgba(color, source.rgb, destination.rgb, parameter);
    }
    float3 parameter = float3(source.a * destination.a, source.a * (1.0 - destination.a), destination.a * (1.0 - source.a));
    return overlap_rgba(color, source.rgb, destination.rgb, parameter);
}

fragment float4 live2d_extended_fragment(VertexOut in [[stage_in]],
                                         texture2d<float> atlas [[texture(0)]],
                                         texture2d<float> mask_texture [[texture(1)]],
                                         texture2d<float> destination_texture [[texture(2)]],
                                         sampler atlas_sampler [[sampler(0)]],
                                         sampler mask_sampler [[sampler(1)]],
                                         constant FragmentParams &params [[buffer(0)]]) {
    float4 source = atlas.sample(atlas_sampler, in.uv);
    if (params.debug_mode == 1) { return float4(in.uv.x, in.uv.y, 0.0, 1.0); }
    if (params.debug_mode == 2) { return float4(source.rgb, 1.0); }
    if (params.debug_mode == 3) { return float4(source.a, source.a, source.a, 1.0); }
    source.rgb = min(source.rgb * params.multiply_color.rgb + params.screen_color.rgb * source.a, 1.0);
    source.a *= params.opacity;
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
        source.a *= mask;
    }
    source.rgb *= source.a;
    float4 destination = destination_texture.sample(atlas_sampler, in.target_uv);
    float3 straight_source = source.a > 0.00001 ? source.rgb / source.a : float3(0.0);
    float3 straight_destination = destination.a > 0.00001 ? destination.rgb / destination.a : float3(0.0);
    float3 blended = extended_color_blend(straight_source, straight_destination, params.color_blend);
    return extended_alpha_blend(blended, source, destination, params.alpha_blend);
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
    color_blend: u32,
    alpha_blend: u32,
    _padding3: [u32; 2],
    debug_mode: u32,
    _padding4: [u32; 3],
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
    extended_pipeline_state: RenderPipelineState,
    mask_pipeline_state: RenderPipelineState,
    atlas_sampler: SamplerState,
    mask_sampler: SamplerState,
    textures: Vec<Texture>,
    white_mask_texture: Texture,
    msaa_color_texture: Option<Texture>,
    msaa_texture_size: [u64; 2],
    mask_atlas_textures: Vec<Texture>,
    mask_atlas_layout: Option<MaskAtlasLayout>,
    high_precision_mask_textures: Vec<Texture>,
    high_precision_mask_texture_size: u64,
    offscreen_textures: Vec<Texture>,
    offscreen_texture_size: [u64; 2],
    blend_snapshot_texture: Option<Texture>,
    blend_snapshot_texture_size: [u64; 2],
    quad_vertex_buffer: Buffer,
    quad_index_buffer: Buffer,
    drawable_buffers: Vec<DrawableGpuBuffers>,
    vertex_ring: DynamicVertexRing,
    mask_tile_size: u64,
    drawable_size: f64,
    physical_drawable_size: [u64; 2],
    contents_scale: f64,
    sample_count: u64,
    disable_masks: bool,
    high_precision_masks: bool,
    hidden_drawables: HashSet<String>,
    hidden_parts: HashSet<String>,
    only_drawables: HashSet<String>,
    only_parts: HashSet<String>,
    highlight_drawables: HashSet<String>,
    highlight_parts: HashSet<String>,
    debug_texture_mode: u32,
}

impl MetalRenderer {
    pub fn load(model: &Live2dModel, config: &RendererConfig) -> Result<Self, String> {
        let device = Device::system_default()
            .ok_or_else(|| "Metal device is not available on this Mac".to_string())?;
        let sample_count = supported_sample_count(&device);
        let command_queue = device.new_command_queue();
        let library = compile_library(&device)?;
        let normal_pipeline_state =
            create_pipeline_state(&device, &library, PipelineBlendMode::Normal, sample_count)?;
        let additive_pipeline_state =
            create_pipeline_state(&device, &library, PipelineBlendMode::Additive, sample_count)?;
        let multiplicative_pipeline_state = create_pipeline_state(
            &device,
            &library,
            PipelineBlendMode::Multiplicative,
            sample_count,
        )?;
        let extended_pipeline_state =
            create_extended_pipeline_state(&device, &library, sample_count)?;
        let mask_pipeline_state = create_mask_pipeline_state(&device, &library)?;
        let atlas_sampler = create_atlas_sampler(&device, config.atlas_mipmaps);
        let mask_sampler = create_mask_sampler(&device);
        let textures = load_textures(&device, &command_queue, model, config.atlas_mipmaps)?;
        let white_mask_texture = create_white_mask_texture(&device);
        let (quad_vertex_buffer, quad_index_buffer) = create_quad_buffers(&device);

        let layer = MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        layer.set_presents_with_transaction(false);
        layer.set_framebuffer_only(false);
        layer.set_opaque(false);
        layer.set_contents_scale(DEFAULT_CONTENTS_SCALE);
        layer.set_drawable_size(CGSize::new(512.0, 512.0));

        Ok(Self {
            device,
            command_queue,
            layer,
            normal_pipeline_state,
            additive_pipeline_state,
            multiplicative_pipeline_state,
            extended_pipeline_state,
            mask_pipeline_state,
            atlas_sampler,
            mask_sampler,
            textures,
            white_mask_texture,
            msaa_color_texture: None,
            msaa_texture_size: [0, 0],
            mask_atlas_textures: Vec::new(),
            mask_atlas_layout: None,
            high_precision_mask_textures: Vec::new(),
            high_precision_mask_texture_size: 0,
            offscreen_textures: Vec::new(),
            offscreen_texture_size: [0, 0],
            blend_snapshot_texture: None,
            blend_snapshot_texture_size: [0, 0],
            quad_vertex_buffer,
            quad_index_buffer,
            drawable_buffers: Vec::new(),
            vertex_ring: DynamicVertexRing::new(),
            mask_tile_size: MIN_MASK_TEXTURE_SIZE,
            drawable_size: 512.0,
            physical_drawable_size: [512, 512],
            contents_scale: DEFAULT_CONTENTS_SCALE,
            sample_count,
            disable_masks: config.disable_masks,
            high_precision_masks: config.high_precision_masks,
            hidden_drawables: config.hidden_drawables.iter().cloned().collect(),
            hidden_parts: config.hidden_parts.iter().cloned().collect(),
            only_drawables: config.only_drawables.iter().cloned().collect(),
            only_parts: config.only_parts.iter().cloned().collect(),
            highlight_drawables: config.highlight_drawables.iter().cloned().collect(),
            highlight_parts: config.highlight_parts.iter().cloned().collect(),
            debug_texture_mode: debug_texture_mode(config.debug_texture_mode.as_deref()),
        })
    }

    pub fn layer_ptr(&self) -> *mut c_void {
        self.layer.as_ptr().cast()
    }

    pub fn set_drawable_size(&mut self, width: f64, height: f64) {
        let logical_size = width.max(1.0).min(height.max(1.0));
        let physical_size = (logical_size * self.contents_scale).round().max(1.0);
        let physical_size_u64 = physical_size as u64;
        self.drawable_size = logical_size;
        self.physical_drawable_size = [physical_size_u64, physical_size_u64];
        let mask_tile_size = stable_mask_texture_size(physical_size_u64);
        if self.mask_tile_size != mask_tile_size {
            self.mask_tile_size = mask_tile_size;
            self.mask_atlas_textures.clear();
            self.mask_atlas_layout = None;
            self.offscreen_textures.clear();
            self.offscreen_texture_size = [0, 0];
            self.blend_snapshot_texture = None;
            self.blend_snapshot_texture_size = [0, 0];
        }
        self.layer
            .set_drawable_size(CGSize::new(physical_size, physical_size));
    }

    pub fn render(&mut self, runtime: &CubismModelRuntime) -> Result<(), String> {
        let mut draw_items = collect_draw_items(runtime);
        draw_items.retain(|item| self.should_render_drawable(&item.drawable));
        let transform = bounds_for(&draw_items).and_then(|bounds| {
            bounds.fit_transform(
                self.drawable_size as f32,
                (self.drawable_size as f32) * 0.04,
            )
        });

        if let Some(transform) = transform {
            let offscreen_items = collect_offscreen_items(runtime);
            prepare_drawable_buffers(
                &self.device,
                &mut self.vertex_ring,
                &mut self.drawable_buffers,
                &draw_items,
                transform,
            );
            let mask_contexts = unique_mask_contexts(
                &draw_items,
                &offscreen_items,
                self.disable_masks,
                self.mask_tile_size,
                runtime.info().pixels_per_unit,
                self.high_precision_masks,
            );
            let mask_lookup = mask_set_lookup(&mask_contexts);
            let mask_layout =
                MaskAtlasLayout::for_mask_count(mask_contexts.len(), self.mask_tile_size);
            if self.high_precision_masks {
                self.ensure_high_precision_mask_textures(mask_contexts.len());
            } else {
                self.ensure_mask_atlas(&mask_layout);
            }
            if !offscreen_items.is_empty() && !self.high_precision_masks {
                self.ensure_offscreen_textures(offscreen_items.len());
                self.ensure_blend_snapshot_texture();
            }
            self.ensure_msaa_texture();
            let Some(drawable) = self.layer.next_drawable() else {
                return Ok(());
            };
            let command_buffer = self.command_queue.new_command_buffer();
            let vertex_buffer = self
                .vertex_ring
                .current_buffer()
                .ok_or_else(|| "Metal vertex ring did not allocate an active buffer".to_string())?;
            if !mask_contexts.is_empty() && !self.high_precision_masks {
                render_mask_atlases(
                    command_buffer,
                    &self.mask_pipeline_state,
                    &self.mask_atlas_textures,
                    &mask_layout,
                    &draw_items,
                    &self.drawable_buffers,
                    &self.textures,
                    &self.atlas_sampler,
                    vertex_buffer,
                    &mask_contexts,
                )?;
            }
            if self.high_precision_masks {
                self.render_high_precision_drawables(
                    command_buffer,
                    drawable.texture(),
                    &draw_items,
                    &self.drawable_buffers,
                    vertex_buffer,
                    &mask_contexts,
                    &mask_lookup,
                )?;
                command_buffer.present_drawable(drawable);
                command_buffer.commit();
                return Ok(());
            }
            if !offscreen_items.is_empty() {
                self.render_with_offscreens(
                    command_buffer,
                    drawable.texture(),
                    &draw_items,
                    &offscreen_items,
                    &runtime.parts(),
                    vertex_buffer,
                    &mask_contexts,
                    &mask_lookup,
                )?;
                command_buffer.present_drawable(drawable);
                command_buffer.commit();
                return Ok(());
            }
            let render_pass_descriptor = RenderPassDescriptor::new();
            let color_attachment = render_pass_descriptor
                .color_attachments()
                .object_at(0)
                .ok_or_else(|| "Metal render pass has no color attachment".to_string())?;
            if self.sample_count > 1 {
                color_attachment
                    .set_texture(self.msaa_color_texture.as_ref().map(|texture| &**texture));
                color_attachment.set_resolve_texture(Some(drawable.texture()));
                color_attachment.set_store_action(MTLStoreAction::MultisampleResolve);
            } else {
                color_attachment.set_texture(Some(drawable.texture()));
                color_attachment.set_store_action(MTLStoreAction::Store);
            }
            color_attachment.set_load_action(MTLLoadAction::Clear);
            color_attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));

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
                let mask_texture = if self.high_precision_masks {
                    mask_index
                        .and_then(|index| self.high_precision_mask_textures.get(index))
                        .unwrap_or(&self.white_mask_texture)
                } else {
                    mask_index
                        .and_then(|index| {
                            mask_contexts.get(index).and_then(|context| {
                                self.mask_atlas_textures.get(context.buffer_index)
                            })
                        })
                        .unwrap_or(&self.white_mask_texture)
                };
                let mask_context = mask_index.and_then(|index| mask_contexts.get(index));
                let fragment_params = drawable_fragment_params(
                    &item.drawable,
                    mask_context,
                    self.is_highlighted_drawable(&item.drawable),
                    self.debug_texture_mode,
                );
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
        let drawables = runtime
            .drawables()
            .into_iter()
            .filter(|drawable| self.should_render_drawable(drawable))
            .collect::<Vec<_>>();
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
        let offscreens = runtime.offscreens();
        let extended_blend_count = drawables
            .iter()
            .filter(|drawable| matches!(drawable.blend_mode, CubismBlendMode::Extended { .. }))
            .count()
            + offscreens
                .iter()
                .filter(|offscreen| {
                    matches!(offscreen.blend_mode, CubismBlendMode::Extended { .. })
                })
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
            extended_blend_count,
            masked_count,
            has_command_queue: !self.command_queue.as_ptr().is_null(),
        }
    }

    fn pipeline_state(&self, blend_mode: CubismBlendMode) -> &RenderPipelineState {
        match blend_mode {
            CubismBlendMode::Additive => &self.additive_pipeline_state,
            CubismBlendMode::Multiplicative => &self.multiplicative_pipeline_state,
            CubismBlendMode::Extended { .. } => &self.extended_pipeline_state,
            CubismBlendMode::Normal | CubismBlendMode::Unknown(_) => &self.normal_pipeline_state,
        }
    }

    fn is_hidden_drawable(&self, drawable: &CubismDrawableInfo) -> bool {
        self.hidden_drawables.contains(&drawable.id)
            || drawable
                .parent_part_id
                .as_ref()
                .is_some_and(|part_id| self.hidden_parts.contains(part_id))
    }

    fn is_only_drawable(&self, drawable: &CubismDrawableInfo) -> bool {
        self.only_drawables.contains(&drawable.id)
            || drawable
                .parent_part_id
                .as_ref()
                .is_some_and(|part_id| self.only_parts.contains(part_id))
    }

    fn should_render_drawable(&self, drawable: &CubismDrawableInfo) -> bool {
        if self.is_hidden_drawable(drawable) {
            return false;
        }
        if self.only_drawables.is_empty() && self.only_parts.is_empty() {
            return true;
        }
        self.is_only_drawable(drawable)
    }

    fn is_highlighted_drawable(&self, drawable: &CubismDrawableInfo) -> bool {
        self.highlight_drawables.contains(&drawable.id)
            || drawable
                .parent_part_id
                .as_ref()
                .is_some_and(|part_id| self.highlight_parts.contains(part_id))
    }

    fn ensure_mask_atlas(&mut self, layout: &MaskAtlasLayout) {
        if layout.mask_count == 0 {
            self.mask_atlas_textures.clear();
            self.mask_atlas_layout = Some(*layout);
            return;
        }

        let needs_new_texture = self.mask_atlas_layout.as_ref().is_none_or(|current| {
            current.texture_size != layout.texture_size
                || current.render_texture_count != layout.render_texture_count
        });
        if needs_new_texture {
            self.mask_atlas_textures = (0..layout.render_texture_count)
                .map(|_| create_mask_texture(&self.device, layout.texture_size))
                .collect();
        } else if self.mask_atlas_textures.len() != layout.render_texture_count {
            self.mask_atlas_textures
                .resize_with(layout.render_texture_count, || {
                    create_mask_texture(&self.device, layout.texture_size)
                });
        }
        self.mask_atlas_layout = Some(*layout);
    }

    fn ensure_high_precision_mask_textures(&mut self, mask_count: usize) {
        if mask_count == 0 {
            self.high_precision_mask_textures.clear();
            self.high_precision_mask_texture_size = 0;
            return;
        }

        if self.high_precision_mask_texture_size != self.mask_tile_size {
            self.high_precision_mask_textures.clear();
            self.high_precision_mask_texture_size = self.mask_tile_size;
        }

        if self.high_precision_mask_textures.len() < mask_count {
            self.high_precision_mask_textures.extend(
                (self.high_precision_mask_textures.len()..mask_count).map(|_| {
                    create_mask_texture(&self.device, self.high_precision_mask_texture_size)
                }),
            );
        } else if self.high_precision_mask_textures.len() > mask_count {
            self.high_precision_mask_textures.truncate(mask_count);
        }
    }

    fn ensure_msaa_texture(&mut self) {
        if self.sample_count <= 1 {
            self.msaa_color_texture = None;
            self.msaa_texture_size = [0, 0];
            return;
        }

        let [width, height] = self.physical_drawable_size;
        let width = width.max(1);
        let height = height.max(1);
        let needs_new_texture = self
            .msaa_color_texture
            .as_ref()
            .is_none_or(|texture| texture.width() != width || texture.height() != height);
        if needs_new_texture {
            self.msaa_color_texture = Some(create_msaa_color_texture(
                &self.device,
                width,
                height,
                self.sample_count,
            ));
            self.msaa_texture_size = [width, height];
        }
    }

    fn ensure_offscreen_textures(&mut self, offscreen_count: usize) {
        if offscreen_count == 0 {
            self.offscreen_textures.clear();
            self.offscreen_texture_size = [0, 0];
            return;
        }

        let [width, height] = self.physical_drawable_size;
        let width = width.max(1);
        let height = height.max(1);
        if self.offscreen_texture_size != [width, height] {
            self.offscreen_textures.clear();
            self.offscreen_texture_size = [width, height];
        }

        if self.offscreen_textures.len() < offscreen_count {
            self.offscreen_textures.extend(
                (self.offscreen_textures.len()..offscreen_count)
                    .map(|_| create_offscreen_texture(&self.device, width, height)),
            );
        } else if self.offscreen_textures.len() > offscreen_count {
            self.offscreen_textures.truncate(offscreen_count);
        }
    }

    fn ensure_blend_snapshot_texture(&mut self) {
        let [width, height] = self.physical_drawable_size;
        let width = width.max(1);
        let height = height.max(1);
        if self.blend_snapshot_texture_size != [width, height] {
            self.blend_snapshot_texture = None;
            self.blend_snapshot_texture_size = [width, height];
        }
        if self.blend_snapshot_texture.is_none() {
            self.blend_snapshot_texture =
                Some(create_blend_snapshot_texture(&self.device, width, height));
        }
    }

    fn render_with_offscreens(
        &self,
        command_buffer: &CommandBufferRef,
        drawable_texture: &metal::TextureRef,
        draw_items: &[DrawItem],
        offscreen_items: &[OffscreenItem],
        parts: &[CubismPartInfo],
        vertex_buffer: &metal::BufferRef,
        mask_contexts: &[MaskContext],
        mask_lookup: &HashMap<Vec<i32>, usize>,
    ) -> Result<(), String> {
        let mut objects = render_objects(draw_items, offscreen_items);
        let draw_lookup = draw_item_lookup(draw_items);
        let mut active_offscreens = Vec::<usize>::new();
        let mut target_initialized = TargetInitialization::new(offscreen_items.len());

        clear_render_target(command_buffer, drawable_texture)?;
        target_initialized.main = true;

        for object in objects.drain(..) {
            match object {
                RenderObject::Drawable(drawable_index) => {
                    let Some(item_index) = draw_lookup.get(&drawable_index).copied() else {
                        continue;
                    };
                    while active_offscreens.last().is_some_and(|offscreen_index| {
                        !part_is_descendant_of(
                            draw_items[item_index].drawable.parent_part_index,
                            offscreen_items[*offscreen_index].offscreen.owner_part_index,
                            parts,
                        )
                    }) {
                        self.flush_offscreen(
                            command_buffer,
                            active_offscreens.pop().expect("checked by last"),
                            active_offscreens.last().copied(),
                            drawable_texture,
                            offscreen_items,
                            mask_contexts,
                            mask_lookup,
                            &mut target_initialized,
                        )?;
                    }

                    let target = active_offscreens.last().copied();
                    self.draw_item_to_target(
                        command_buffer,
                        target,
                        drawable_texture,
                        &draw_items[item_index],
                        vertex_buffer,
                        mask_contexts,
                        mask_lookup,
                        &mut target_initialized,
                    )?;
                }
                RenderObject::Offscreen(offscreen_index) => {
                    let owner_part = offscreen_items[offscreen_index].offscreen.owner_part_index;
                    while active_offscreens.last().is_some_and(|active_index| {
                        !part_is_descendant_of(
                            owner_part,
                            offscreen_items[*active_index].offscreen.owner_part_index,
                            parts,
                        )
                    }) {
                        self.flush_offscreen(
                            command_buffer,
                            active_offscreens.pop().expect("checked by last"),
                            active_offscreens.last().copied(),
                            drawable_texture,
                            offscreen_items,
                            mask_contexts,
                            mask_lookup,
                            &mut target_initialized,
                        )?;
                    }
                    active_offscreens.push(offscreen_index);
                }
            }
        }

        while let Some(offscreen_index) = active_offscreens.pop() {
            self.flush_offscreen(
                command_buffer,
                offscreen_index,
                active_offscreens.last().copied(),
                drawable_texture,
                offscreen_items,
                mask_contexts,
                mask_lookup,
                &mut target_initialized,
            )?;
        }

        Ok(())
    }

    fn draw_item_to_target(
        &self,
        command_buffer: &CommandBufferRef,
        target: Option<usize>,
        drawable_texture: &metal::TextureRef,
        item: &DrawItem,
        vertex_buffer: &metal::BufferRef,
        mask_contexts: &[MaskContext],
        mask_lookup: &HashMap<Vec<i32>, usize>,
        target_initialized: &mut TargetInitialization,
    ) -> Result<(), String> {
        if !item.drawable.flags.visible || item.drawable.opacity <= 0.0 {
            return Ok(());
        }

        let Some(texture) = self
            .textures
            .get(item.drawable.texture_index.max(0) as usize)
        else {
            return Ok(());
        };
        let Some(buffers) = self.drawable_buffers.get(item.drawable.index) else {
            return Ok(());
        };
        if !buffers.is_ready() {
            return Ok(());
        }

        let Some(target_texture) = self.target_texture(target, drawable_texture) else {
            return Ok(());
        };
        let extended_blend = matches!(item.drawable.blend_mode, CubismBlendMode::Extended { .. });
        if extended_blend {
            if !target_initialized.is_initialized(target) {
                clear_render_target(command_buffer, target_texture)?;
                target_initialized.mark_initialized(target);
            }
            if let Some(snapshot) = self.blend_snapshot_texture.as_ref() {
                copy_texture(command_buffer, target_texture, snapshot);
            }
        }
        let render_pass_descriptor = RenderPassDescriptor::new();
        let color_attachment = render_pass_descriptor
            .color_attachments()
            .object_at(0)
            .ok_or_else(|| "Metal offscreen drawable pass has no color attachment".to_string())?;
        color_attachment.set_texture(Some(target_texture));
        color_attachment.set_load_action(target_initialized.load_action(target));
        color_attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
        color_attachment.set_store_action(MTLStoreAction::Store);

        let encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
        encoder.set_fragment_sampler_state(0, Some(&self.atlas_sampler));
        encoder.set_fragment_sampler_state(1, Some(&self.mask_sampler));
        encoder.set_render_pipeline_state(self.pipeline_state(item.drawable.blend_mode));
        encoder.set_cull_mode(drawable_cull_mode(&item.drawable));
        encoder.set_front_facing_winding(drawable_front_winding(
            &item.frame,
            FitTransform::identity(),
        ));
        encoder.set_vertex_buffer(0, Some(vertex_buffer), buffers.vertex_offset);
        encoder.set_fragment_texture(0, Some(texture));

        let mask_index = mask_lookup.get(&item.drawable.masks).copied();
        let mask_context = mask_index.and_then(|index| mask_contexts.get(index));
        let mask_texture = mask_index
            .and_then(|index| {
                mask_contexts
                    .get(index)
                    .and_then(|context| self.mask_atlas_textures.get(context.buffer_index))
            })
            .unwrap_or(&self.white_mask_texture);
        encoder.set_fragment_texture(1, Some(mask_texture));
        if extended_blend {
            if let Some(snapshot) = self.blend_snapshot_texture.as_ref() {
                encoder.set_fragment_texture(2, Some(snapshot));
            }
        }
        set_fragment_params_for_drawable(
            &encoder,
            &item.drawable,
            mask_context,
            self.is_highlighted_drawable(&item.drawable),
            self.debug_texture_mode,
        );
        encoder.draw_indexed_primitives(
            MTLPrimitiveType::Triangle,
            buffers.index_count as u64,
            MTLIndexType::UInt16,
            buffers.index_buffer.as_ref().expect("checked by is_ready"),
            0,
        );
        encoder.end_encoding();
        target_initialized.mark_initialized(target);
        Ok(())
    }

    fn flush_offscreen(
        &self,
        command_buffer: &CommandBufferRef,
        offscreen_index: usize,
        parent_target: Option<usize>,
        drawable_texture: &metal::TextureRef,
        offscreen_items: &[OffscreenItem],
        mask_contexts: &[MaskContext],
        mask_lookup: &HashMap<Vec<i32>, usize>,
        target_initialized: &mut TargetInitialization,
    ) -> Result<(), String> {
        let Some(offscreen_texture) = self.offscreen_textures.get(offscreen_index) else {
            return Ok(());
        };
        if !target_initialized.is_initialized(Some(offscreen_index)) {
            return Ok(());
        }
        let Some(target_texture) = self.target_texture(parent_target, drawable_texture) else {
            return Ok(());
        };
        let offscreen = &offscreen_items[offscreen_index].offscreen;
        if offscreen.opacity <= 0.0 {
            return Ok(());
        }
        let extended_blend = matches!(offscreen.blend_mode, CubismBlendMode::Extended { .. });
        if extended_blend {
            if !target_initialized.is_initialized(parent_target) {
                clear_render_target(command_buffer, target_texture)?;
                target_initialized.mark_initialized(parent_target);
            }
            if let Some(snapshot) = self.blend_snapshot_texture.as_ref() {
                copy_texture(command_buffer, target_texture, snapshot);
            }
        }

        let render_pass_descriptor = RenderPassDescriptor::new();
        let color_attachment = render_pass_descriptor
            .color_attachments()
            .object_at(0)
            .ok_or_else(|| "Metal offscreen composite pass has no color attachment".to_string())?;
        color_attachment.set_texture(Some(target_texture));
        color_attachment.set_load_action(target_initialized.load_action(parent_target));
        color_attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
        color_attachment.set_store_action(MTLStoreAction::Store);

        let encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
        encoder.set_fragment_sampler_state(0, Some(&self.atlas_sampler));
        encoder.set_fragment_sampler_state(1, Some(&self.mask_sampler));
        encoder.set_render_pipeline_state(self.pipeline_state(offscreen.blend_mode));
        encoder.set_cull_mode(MTLCullMode::None);
        encoder.set_front_facing_winding(MTLWinding::CounterClockwise);
        encoder.set_vertex_buffer(0, Some(&self.quad_vertex_buffer), 0);
        encoder.set_fragment_texture(0, Some(offscreen_texture));
        if extended_blend {
            if let Some(snapshot) = self.blend_snapshot_texture.as_ref() {
                encoder.set_fragment_texture(2, Some(snapshot));
            }
        }
        let mask_index = mask_lookup.get(&offscreen.masks).copied();
        let mask_context = mask_index.and_then(|index| mask_contexts.get(index));
        let mask_texture = mask_index
            .and_then(|index| {
                mask_contexts
                    .get(index)
                    .and_then(|context| self.mask_atlas_textures.get(context.buffer_index))
            })
            .unwrap_or(&self.white_mask_texture);
        encoder.set_fragment_texture(1, Some(mask_texture));
        set_fragment_params_for_offscreen(&encoder, offscreen, mask_context);
        encoder.draw_indexed_primitives(
            MTLPrimitiveType::Triangle,
            6,
            MTLIndexType::UInt16,
            &self.quad_index_buffer,
            0,
        );
        encoder.end_encoding();
        target_initialized.mark_initialized(parent_target);
        Ok(())
    }

    fn target_texture<'a>(
        &'a self,
        target: Option<usize>,
        drawable_texture: &'a metal::TextureRef,
    ) -> Option<&'a metal::TextureRef> {
        match target {
            Some(index) => self.offscreen_textures.get(index).map(|texture| &**texture),
            None => Some(drawable_texture),
        }
    }

    fn render_high_precision_drawables(
        &self,
        command_buffer: &CommandBufferRef,
        drawable_texture: &metal::TextureRef,
        draw_items: &[DrawItem],
        drawable_buffers: &[DrawableGpuBuffers],
        vertex_buffer: &metal::BufferRef,
        mask_contexts: &[MaskContext],
        mask_lookup: &HashMap<Vec<i32>, usize>,
    ) -> Result<(), String> {
        let mut rendered_anything = false;
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
            let Some(buffers) = drawable_buffers.get(item.drawable.index) else {
                continue;
            };
            if !buffers.is_ready() {
                continue;
            }

            let mask_index = mask_lookup.get(&item.drawable.masks).copied();
            if let Some(index) = mask_index {
                let Some(mask_texture) = self.high_precision_mask_textures.get(index) else {
                    continue;
                };
                let Some(mask_context) = mask_contexts.get(index) else {
                    continue;
                };
                render_high_precision_mask(
                    command_buffer,
                    &self.mask_pipeline_state,
                    mask_texture,
                    &self.atlas_sampler,
                    draw_items,
                    drawable_buffers,
                    &self.textures,
                    vertex_buffer,
                    mask_context,
                )?;
            }

            let mask_context = mask_index.and_then(|index| mask_contexts.get(index));
            let mask_texture = mask_index
                .and_then(|index| self.high_precision_mask_textures.get(index))
                .unwrap_or(&self.white_mask_texture);
            let fragment_params = drawable_fragment_params(
                &item.drawable,
                mask_context,
                self.is_highlighted_drawable(&item.drawable),
                self.debug_texture_mode,
            );

            let render_pass_descriptor = RenderPassDescriptor::new();
            let color_attachment = render_pass_descriptor
                .color_attachments()
                .object_at(0)
                .ok_or_else(|| {
                    "Metal high precision main pass has no color attachment".to_string()
                })?;
            color_attachment.set_texture(Some(drawable_texture));
            color_attachment.set_load_action(if rendered_anything {
                MTLLoadAction::Load
            } else {
                MTLLoadAction::Clear
            });
            color_attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
            color_attachment.set_store_action(MTLStoreAction::Store);

            let encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
            encoder.set_fragment_sampler_state(0, Some(&self.atlas_sampler));
            encoder.set_fragment_sampler_state(1, Some(&self.mask_sampler));
            encoder.set_render_pipeline_state(self.pipeline_state(item.drawable.blend_mode));
            encoder.set_cull_mode(drawable_cull_mode(&item.drawable));
            encoder.set_front_facing_winding(drawable_front_winding(
                &item.frame,
                FitTransform::identity(),
            ));
            encoder.set_vertex_buffer(0, Some(vertex_buffer), buffers.vertex_offset);
            encoder.set_fragment_texture(0, Some(texture));
            encoder.set_fragment_texture(1, Some(mask_texture));
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
            encoder.end_encoding();
            rendered_anything = true;
        }

        if !rendered_anything {
            clear_drawable(command_buffer, drawable_texture)?;
        }

        Ok(())
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
    pub extended_blend_count: usize,
    pub masked_count: usize,
    pub has_command_queue: bool,
}

struct DrawItem {
    drawable: CubismDrawableInfo,
    frame: CubismDrawableFrame,
}

struct OffscreenItem {
    offscreen: CubismOffscreenInfo,
}

#[derive(Clone, Copy)]
enum RenderObject {
    Drawable(usize),
    Offscreen(usize),
}

struct TargetInitialization {
    main: bool,
    offscreens: Vec<bool>,
}

impl TargetInitialization {
    fn new(offscreen_count: usize) -> Self {
        Self {
            main: false,
            offscreens: vec![false; offscreen_count],
        }
    }

    fn load_action(&self, target: Option<usize>) -> MTLLoadAction {
        if self.is_initialized(target) {
            MTLLoadAction::Load
        } else {
            MTLLoadAction::Clear
        }
    }

    fn mark_initialized(&mut self, target: Option<usize>) {
        match target {
            Some(index) => {
                if let Some(initialized) = self.offscreens.get_mut(index) {
                    *initialized = true;
                }
            }
            None => self.main = true,
        }
    }

    fn is_initialized(&self, target: Option<usize>) -> bool {
        match target {
            Some(index) => self.offscreens.get(index).copied().unwrap_or(false),
            None => self.main,
        }
    }
}

struct MaskContext {
    masks: Vec<i32>,
    bounds: Bounds,
    buffer_index: usize,
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

    fn rebuild_matrices(&mut self, mask_texture_size: u64, pixels_per_unit: Option<f32>) {
        let placement = MaskPlacement::new(
            self.bounds,
            self.layout_bounds,
            mask_texture_size,
            pixels_per_unit,
        );
        self.matrix_for_mask = Affine2::for_mask(placement);
        self.matrix_for_draw = Affine2::for_draw(placement);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

    fn for_mask(placement: MaskPlacement) -> Self {
        let draw = Self::for_draw(placement);
        Self {
            x: [draw.x[0] * 2.0, draw.x[1] * 2.0 - 1.0],
            y: [draw.y[0] * 2.0, draw.y[1] * 2.0 - 1.0],
        }
    }

    fn for_draw(placement: MaskPlacement) -> Self {
        Self {
            x: [
                placement.scale_x,
                placement.layout.x - placement.bounds.min_x * placement.scale_x,
            ],
            y: [
                placement.scale_y,
                placement.layout.y - placement.bounds.min_y * placement.scale_y,
            ],
        }
    }
}

#[derive(Clone, Copy)]
struct MaskPlacement {
    bounds: Bounds,
    layout: LayoutBounds,
    scale_x: f32,
    scale_y: f32,
}

impl MaskPlacement {
    const MARGIN: f32 = 0.05;

    fn new(
        bounds: Bounds,
        layout: LayoutBounds,
        mask_texture_size: u64,
        pixels_per_unit: Option<f32>,
    ) -> Self {
        let Some(ppu) = pixels_per_unit.filter(|value| value.is_finite() && *value > 0.0) else {
            return Self::full_layout(bounds, layout);
        };

        let mask_width = mask_texture_size.max(1) as f32;
        let mask_height = mask_width;
        let physical_mask_width = (layout.width * mask_width).max(f32::EPSILON);
        let physical_mask_height = (layout.height * mask_height).max(f32::EPSILON);
        let mut adjusted_bounds = bounds;
        let scale_x = if bounds.width() * ppu > physical_mask_width {
            adjusted_bounds = adjusted_bounds.expanded_x(bounds.width() * Self::MARGIN);
            layout.width / adjusted_bounds.width().max(f32::EPSILON)
        } else {
            ppu / physical_mask_width
        };
        let scale_y = if bounds.height() * ppu > physical_mask_height {
            adjusted_bounds = adjusted_bounds.expanded_y(bounds.height() * Self::MARGIN);
            layout.height / adjusted_bounds.height().max(f32::EPSILON)
        } else {
            ppu / physical_mask_height
        };

        Self {
            bounds: adjusted_bounds,
            layout,
            scale_x,
            scale_y,
        }
    }

    fn full_layout(bounds: Bounds, layout: LayoutBounds) -> Self {
        let bounds = bounds.expanded_by_fraction(Self::MARGIN);
        Self {
            bounds,
            layout,
            scale_x: layout.width / bounds.width().max(f32::EPSILON),
            scale_y: layout.height / bounds.height().max(f32::EPSILON),
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
    render_texture_count: usize,
    texture_size: u64,
}

impl MaskAtlasLayout {
    fn for_mask_count(mask_count: usize, tile_size: u64) -> Self {
        Self {
            mask_count,
            render_texture_count: mask_render_texture_count(mask_count),
            texture_size: tile_size.max(1),
        }
    }
}

fn mask_render_texture_count(mask_count: usize) -> usize {
    if mask_count == 0 {
        0
    } else if mask_count <= DEFAULT_MASK_CONTEXTS_PER_TEXTURE {
        1
    } else {
        mask_count.div_ceil(MULTI_MASK_CONTEXTS_PER_TEXTURE)
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

    fn expanded_x(&self, amount: f32) -> Self {
        Self {
            min_x: self.min_x - amount.max(0.0),
            max_x: self.max_x + amount.max(0.0),
            ..*self
        }
    }

    fn expanded_y(&self, amount: f32) -> Self {
        Self {
            min_y: self.min_y - amount.max(0.0),
            max_y: self.max_y + amount.max(0.0),
            ..*self
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
    sample_count: u64,
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
    descriptor.set_raster_sample_count(sample_count);
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

fn create_extended_pipeline_state(
    device: &Device,
    library: &Library,
    sample_count: u64,
) -> Result<RenderPipelineState, String> {
    let vertex = library
        .get_function("live2d_vertex", None)
        .map_err(|error| format!("Failed to load Metal vertex shader: {error}"))?;
    let fragment = library
        .get_function("live2d_extended_fragment", None)
        .map_err(|error| format!("Failed to load Metal extended fragment shader: {error}"))?;

    let descriptor = RenderPipelineDescriptor::new();
    descriptor.set_vertex_function(Some(&vertex));
    descriptor.set_fragment_function(Some(&fragment));
    descriptor.set_raster_sample_count(sample_count);
    let attachment = descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| "Metal extended pipeline has no color attachment".to_string())?;
    attachment.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    attachment.set_blending_enabled(false);

    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| format!("Failed to create Metal extended render pipeline: {error}"))
}

fn supported_sample_count(device: &Device) -> u64 {
    if device.supports_texture_sample_count(PREFERRED_MSAA_SAMPLE_COUNT) {
        PREFERRED_MSAA_SAMPLE_COUNT
    } else {
        1
    }
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

fn create_atlas_sampler(device: &Device, mipmapped: bool) -> SamplerState {
    let descriptor = SamplerDescriptor::new();
    descriptor.set_min_filter(MTLSamplerMinMagFilter::Linear);
    descriptor.set_mag_filter(MTLSamplerMinMagFilter::Linear);
    descriptor.set_mip_filter(if mipmapped {
        MTLSamplerMipFilter::Linear
    } else {
        MTLSamplerMipFilter::NotMipmapped
    });
    descriptor.set_address_mode_s(MTLSamplerAddressMode::ClampToEdge);
    descriptor.set_address_mode_t(MTLSamplerAddressMode::ClampToEdge);
    if mipmapped {
        descriptor.set_max_anisotropy(8);
    }
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
    mipmapped: bool,
) -> Result<Vec<Texture>, String> {
    let mut textures = Vec::with_capacity(model.textures.len());
    for texture_path in &model.textures {
        let image = image::open(texture_path)
            .map_err(|error| format!("Failed to load texture {}: {error}", texture_path.display()))?
            .to_rgba8();
        textures.push(upload_texture(device, command_queue, &image, mipmapped));
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

fn create_msaa_color_texture(
    device: &Device,
    width: u64,
    height: u64,
    sample_count: u64,
) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2Multisample);
    descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    descriptor.set_width(width);
    descriptor.set_height(height);
    descriptor.set_sample_count(sample_count);
    descriptor.set_usage(MTLTextureUsage::RenderTarget);
    descriptor.set_resource_options(MTLResourceOptions::StorageModePrivate);
    device.new_texture(&descriptor)
}

fn create_offscreen_texture(device: &Device, width: u64, height: u64) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    descriptor.set_width(width.max(1));
    descriptor.set_height(height.max(1));
    descriptor.set_usage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    descriptor.set_resource_options(MTLResourceOptions::StorageModePrivate);
    device.new_texture(&descriptor)
}

fn create_blend_snapshot_texture(device: &Device, width: u64, height: u64) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    descriptor.set_width(width.max(1));
    descriptor.set_height(height.max(1));
    descriptor.set_usage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
    descriptor.set_resource_options(MTLResourceOptions::StorageModePrivate);
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

fn create_quad_buffers(device: &Device) -> (Buffer, Buffer) {
    let vertices = [
        MetalVertex {
            position: [-1.0, -1.0],
            model_position: [-1.0, -1.0],
            uv: [0.0, 0.0],
        },
        MetalVertex {
            position: [1.0, -1.0],
            model_position: [1.0, -1.0],
            uv: [1.0, 0.0],
        },
        MetalVertex {
            position: [-1.0, 1.0],
            model_position: [-1.0, 1.0],
            uv: [0.0, 1.0],
        },
        MetalVertex {
            position: [1.0, 1.0],
            model_position: [1.0, 1.0],
            uv: [1.0, 1.0],
        },
    ];
    let indices = [0_u16, 1, 2, 2, 1, 3];
    let vertex_buffer = device.new_buffer_with_data(
        vertices.as_ptr().cast(),
        std::mem::size_of_val(&vertices) as u64,
        MTLResourceOptions::CPUCacheModeDefaultCache | MTLResourceOptions::StorageModeShared,
    );
    let index_buffer = device.new_buffer_with_data(
        indices.as_ptr().cast(),
        std::mem::size_of_val(&indices) as u64,
        MTLResourceOptions::CPUCacheModeDefaultCache | MTLResourceOptions::StorageModeShared,
    );
    (vertex_buffer, index_buffer)
}

fn upload_texture(
    device: &Device,
    command_queue: &CommandQueue,
    image: &RgbaImage,
    mipmapped: bool,
) -> Texture {
    let descriptor = TextureDescriptor::new();
    descriptor.set_texture_type(MTLTextureType::D2);
    descriptor.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
    descriptor.set_width(image.width() as u64);
    descriptor.set_height(image.height() as u64);
    descriptor.set_mipmap_level_count(if mipmapped {
        mipmap_level_count(image.width(), image.height())
    } else {
        1
    });
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
    if mipmapped {
        generate_mipmaps(command_queue, &texture);
    }
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

fn render_mask_atlases(
    command_buffer: &CommandBufferRef,
    mask_pipeline_state: &RenderPipelineState,
    mask_atlas_textures: &[Texture],
    layout: &MaskAtlasLayout,
    draw_items: &[DrawItem],
    drawable_buffers: &[DrawableGpuBuffers],
    textures: &[Texture],
    atlas_sampler: &SamplerState,
    vertex_buffer: &metal::BufferRef,
    mask_contexts: &[MaskContext],
) -> Result<(), String> {
    for (buffer_index, mask_atlas_texture) in mask_atlas_textures.iter().enumerate() {
        render_mask_atlas(
            command_buffer,
            mask_pipeline_state,
            mask_atlas_texture,
            layout,
            draw_items,
            drawable_buffers,
            textures,
            atlas_sampler,
            vertex_buffer,
            mask_contexts,
            buffer_index,
        )?;
    }

    Ok(())
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
    buffer_index: usize,
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

    for context in mask_contexts
        .iter()
        .filter(|context| context.buffer_index == buffer_index)
    {
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

fn render_high_precision_mask(
    command_buffer: &CommandBufferRef,
    mask_pipeline_state: &RenderPipelineState,
    mask_texture: &Texture,
    atlas_sampler: &SamplerState,
    draw_items: &[DrawItem],
    drawable_buffers: &[DrawableGpuBuffers],
    textures: &[Texture],
    vertex_buffer: &metal::BufferRef,
    mask_context: &MaskContext,
) -> Result<(), String> {
    let render_pass_descriptor = RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| {
            "Metal high precision mask render pass has no color attachment".to_string()
        })?;
    color_attachment.set_texture(Some(mask_texture));
    color_attachment.set_load_action(MTLLoadAction::Clear);
    color_attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
    color_attachment.set_store_action(MTLStoreAction::Store);

    let encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    encoder.set_render_pipeline_state(mask_pipeline_state);
    encoder.set_cull_mode(MTLCullMode::None);
    encoder.set_fragment_sampler_state(0, Some(atlas_sampler));
    render_mask_context(
        encoder,
        mask_texture.width(),
        mask_context,
        draw_items,
        drawable_buffers,
        textures,
        vertex_buffer,
    );
    encoder.end_encoding();

    Ok(())
}

fn clear_drawable(
    command_buffer: &CommandBufferRef,
    drawable_texture: &metal::TextureRef,
) -> Result<(), String> {
    let render_pass_descriptor = RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| "Metal clear pass has no color attachment".to_string())?;
    color_attachment.set_texture(Some(drawable_texture));
    color_attachment.set_load_action(MTLLoadAction::Clear);
    color_attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
    color_attachment.set_store_action(MTLStoreAction::Store);
    let encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    encoder.end_encoding();
    Ok(())
}

fn clear_render_target(
    command_buffer: &CommandBufferRef,
    target_texture: &metal::TextureRef,
) -> Result<(), String> {
    let render_pass_descriptor = RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .ok_or_else(|| "Metal clear target pass has no color attachment".to_string())?;
    color_attachment.set_texture(Some(target_texture));
    color_attachment.set_load_action(MTLLoadAction::Clear);
    color_attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
    color_attachment.set_store_action(MTLStoreAction::Store);
    let encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    encoder.end_encoding();
    Ok(())
}

fn copy_texture(
    command_buffer: &CommandBufferRef,
    source: &metal::TextureRef,
    destination: &Texture,
) {
    let width = source.width().min(destination.width()).max(1);
    let height = source.height().min(destination.height()).max(1);
    let encoder = command_buffer.new_blit_command_encoder();
    encoder.copy_from_texture(
        source,
        0,
        0,
        MTLOrigin { x: 0, y: 0, z: 0 },
        MTLSize {
            width,
            height,
            depth: 1,
        },
        destination,
        0,
        0,
        MTLOrigin { x: 0, y: 0, z: 0 },
    );
    encoder.end_encoding();
}

fn render_mask_context(
    encoder: &metal::RenderCommandEncoderRef,
    texture_size: u64,
    context: &MaskContext,
    draw_items: &[DrawItem],
    drawable_buffers: &[DrawableGpuBuffers],
    textures: &[Texture],
    vertex_buffer: &metal::BufferRef,
) {
    let mask_vertex_params = MaskVertexParams {
        mask_x: context.matrix_for_mask.x,
        mask_y: context.matrix_for_mask.y,
    };
    encoder.set_viewport(MTLViewport {
        originX: 0.0,
        originY: 0.0,
        width: texture_size as f64,
        height: texture_size as f64,
        znear: 0.0,
        zfar: 1.0,
    });
    encoder.set_scissor_rect(MTLScissorRect {
        x: 0,
        y: 0,
        width: texture_size,
        height: texture_size,
    });
    encoder.set_vertex_bytes(
        1,
        std::mem::size_of::<MaskVertexParams>() as u64,
        (&raw const mask_vertex_params).cast(),
    );

    let mut cull_mode = None;
    let mut front_winding = None;
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

fn collect_offscreen_items(runtime: &CubismModelRuntime) -> Vec<OffscreenItem> {
    let mut offscreens = runtime.offscreens();
    offscreens.sort_by_key(|offscreen| offscreen.index);
    offscreens
        .into_iter()
        .map(|offscreen| OffscreenItem { offscreen })
        .collect()
}

fn render_objects(draw_items: &[DrawItem], offscreen_items: &[OffscreenItem]) -> Vec<RenderObject> {
    let mut objects = draw_items
        .iter()
        .map(|item| {
            (
                item.drawable.render_order,
                RenderObject::Drawable(item.drawable.index),
            )
        })
        .chain(offscreen_items.iter().map(|item| {
            (
                item.offscreen.render_order,
                RenderObject::Offscreen(item.offscreen.index),
            )
        }))
        .collect::<Vec<_>>();
    objects.sort_by_key(|(render_order, _)| *render_order);
    objects.into_iter().map(|(_, object)| object).collect()
}

fn draw_item_lookup(draw_items: &[DrawItem]) -> HashMap<usize, usize> {
    draw_items
        .iter()
        .enumerate()
        .map(|(item_index, item)| (item.drawable.index, item_index))
        .collect()
}

fn part_is_descendant_of(part_index: i32, ancestor_index: i32, parts: &[CubismPartInfo]) -> bool {
    if ancestor_index < 0 {
        return false;
    }

    let mut current = part_index;
    while current >= 0 {
        if current == ancestor_index {
            return true;
        }
        let Some(part) = parts.get(current as usize) else {
            return false;
        };
        current = part.parent_part_index;
    }

    false
}

fn set_fragment_params_for_drawable(
    encoder: &metal::RenderCommandEncoderRef,
    drawable: &CubismDrawableInfo,
    mask_context: Option<&MaskContext>,
    highlighted: bool,
    debug_mode: u32,
) {
    let fragment_params = drawable_fragment_params(drawable, mask_context, highlighted, debug_mode);
    encoder.set_fragment_bytes(
        0,
        std::mem::size_of::<FragmentParams>() as u64,
        (&raw const fragment_params).cast(),
    );
}

fn drawable_fragment_params(
    drawable: &CubismDrawableInfo,
    mask_context: Option<&MaskContext>,
    highlighted: bool,
    debug_mode: u32,
) -> FragmentParams {
    let draw_matrix = mask_context
        .map(|context| context.matrix_for_draw)
        .unwrap_or_else(Affine2::identity);
    let layout_bounds = mask_context
        .map(MaskContext::shader_layout_bounds)
        .unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let mask_channel = mask_context
        .map(|context| context.channel.index())
        .unwrap_or_else(|| MaskChannel::Red.index());
    let (multiply_color, screen_color) = if highlighted {
        ([0.2, 1.0, 0.2, 1.0], [0.0, 0.45, 0.0, 1.0])
    } else {
        (drawable.multiply_color, drawable.screen_color)
    };

    FragmentParams {
        opacity: drawable.opacity.clamp(0.0, 1.0),
        has_mask: u32::from(mask_context.is_some()),
        inverted_mask: u32::from(drawable.flags.inverted_mask),
        _padding: 0,
        draw_x: draw_matrix.x,
        draw_y: draw_matrix.y,
        layout_bounds,
        mask_channel_index: mask_channel,
        _padding2: [0; 3],
        multiply_color,
        screen_color,
        color_blend: blend_color(drawable.blend_mode),
        alpha_blend: blend_alpha(drawable.blend_mode),
        _padding3: [0; 2],
        debug_mode,
        _padding4: [0; 3],
    }
}

fn set_fragment_params_for_offscreen(
    encoder: &metal::RenderCommandEncoderRef,
    offscreen: &CubismOffscreenInfo,
    mask_context: Option<&MaskContext>,
) {
    let draw_matrix = mask_context
        .map(|context| context.matrix_for_draw)
        .unwrap_or_else(Affine2::identity);
    let layout_bounds = mask_context
        .map(MaskContext::shader_layout_bounds)
        .unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let mask_channel = mask_context
        .map(|context| context.channel.index())
        .unwrap_or_else(|| MaskChannel::Red.index());
    let fragment_params = FragmentParams {
        opacity: offscreen.opacity.clamp(0.0, 1.0),
        has_mask: u32::from(mask_context.is_some()),
        inverted_mask: u32::from(offscreen.flags.inverted_mask),
        _padding: 0,
        draw_x: draw_matrix.x,
        draw_y: draw_matrix.y,
        layout_bounds,
        mask_channel_index: mask_channel,
        _padding2: [0; 3],
        multiply_color: offscreen.multiply_color,
        screen_color: offscreen.screen_color,
        color_blend: blend_color(offscreen.blend_mode),
        alpha_blend: blend_alpha(offscreen.blend_mode),
        _padding3: [0; 2],
        debug_mode: 0,
        _padding4: [0; 3],
    };
    encoder.set_fragment_bytes(
        0,
        std::mem::size_of::<FragmentParams>() as u64,
        (&raw const fragment_params).cast(),
    );
}

fn debug_texture_mode(value: Option<&str>) -> u32 {
    match value.unwrap_or("none").trim().to_ascii_lowercase().as_str() {
        "uv" => 1,
        "rgb" | "texture" | "color" => 2,
        "alpha" => 3,
        _ => 0,
    }
}

fn blend_color(blend_mode: CubismBlendMode) -> u32 {
    match blend_mode {
        CubismBlendMode::Extended { color, .. } => color.max(0) as u32,
        CubismBlendMode::Additive => 1,
        CubismBlendMode::Multiplicative => 2,
        CubismBlendMode::Normal | CubismBlendMode::Unknown(_) => 0,
    }
}

fn blend_alpha(blend_mode: CubismBlendMode) -> u32 {
    match blend_mode {
        CubismBlendMode::Extended { alpha, .. } => alpha.max(0) as u32,
        CubismBlendMode::Normal
        | CubismBlendMode::Additive
        | CubismBlendMode::Multiplicative
        | CubismBlendMode::Unknown(_) => 0,
    }
}

fn unique_mask_contexts(
    items: &[DrawItem],
    offscreen_items: &[OffscreenItem],
    disable_masks: bool,
    mask_texture_size: u64,
    pixels_per_unit: Option<f32>,
    high_precision_masks: bool,
) -> Vec<MaskContext> {
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
                .unwrap_or_else(|| bounds_for(items).unwrap_or_else(Bounds::unit));
            mask_contexts.push(MaskContext {
                masks: item.drawable.masks.clone(),
                bounds,
                buffer_index: 0,
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
    for item in offscreen_items {
        if item.offscreen.masks.is_empty() {
            continue;
        }

        if !mask_contexts
            .iter()
            .any(|context| context.masks.as_slice() == item.offscreen.masks.as_slice())
        {
            let bounds = clipped_bounds_for_mask(items, &item.offscreen.masks)
                .unwrap_or_else(|| bounds_for(items).unwrap_or_else(Bounds::unit));
            mask_contexts.push(MaskContext {
                masks: item.offscreen.masks.clone(),
                bounds,
                buffer_index: 0,
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
    if high_precision_masks {
        assign_high_precision_mask_layouts(&mut mask_contexts, mask_texture_size, pixels_per_unit);
    } else {
        assign_mask_layouts(&mut mask_contexts, mask_texture_size, pixels_per_unit);
    }
    mask_contexts
}

fn assign_high_precision_mask_layouts(
    mask_contexts: &mut [MaskContext],
    mask_texture_size: u64,
    pixels_per_unit: Option<f32>,
) {
    for (buffer_index, context) in mask_contexts.iter_mut().enumerate() {
        context.buffer_index = buffer_index;
        context.channel = MaskChannel::Red;
        context.layout_bounds = LayoutBounds {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        context.rebuild_matrices(mask_texture_size, pixels_per_unit);
    }
}

fn assign_mask_layouts(
    mask_contexts: &mut [MaskContext],
    mask_texture_size: u64,
    pixels_per_unit: Option<f32>,
) {
    let count = mask_contexts.len();
    if count == 0 {
        return;
    }

    let contexts_per_texture = if count <= DEFAULT_MASK_CONTEXTS_PER_TEXTURE {
        DEFAULT_MASK_CONTEXTS_PER_TEXTURE
    } else {
        MULTI_MASK_CONTEXTS_PER_TEXTURE
    };

    for (buffer_index, chunk) in mask_contexts.chunks_mut(contexts_per_texture).enumerate() {
        assign_mask_layouts_for_texture(chunk, buffer_index, mask_texture_size, pixels_per_unit);
    }
}

fn assign_mask_layouts_for_texture(
    mask_contexts: &mut [MaskContext],
    buffer_index: usize,
    mask_texture_size: u64,
    pixels_per_unit: Option<f32>,
) {
    let div_count = mask_contexts.len() / 4;
    let mod_count = mask_contexts.len() % 4;
    let mut cursor = 0;

    for channel_index in 0..4 {
        let layout_count = div_count + usize::from(channel_index < mod_count);
        for slot in 0..layout_count {
            let Some(context) = mask_contexts.get_mut(cursor) else {
                return;
            };
            context.buffer_index = buffer_index;
            context.channel = MaskChannel::from_index(channel_index);
            context.layout_bounds = layout_bounds_for_slot(slot, layout_count);
            context.rebuild_matrices(mask_texture_size, pixels_per_unit);
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

fn stable_mask_texture_size(physical_drawable_size: u64) -> u64 {
    physical_drawable_size
        .max(MIN_MASK_TEXTURE_SIZE)
        .next_power_of_two()
        .min(MAX_MASK_TEXTURE_SIZE)
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
    use super::{
        Affine2, Bounds, LayoutBounds, MAX_MASK_TEXTURE_SIZE, MaskChannel, MaskContext,
        MaskPlacement, MetalRenderer, assign_high_precision_mask_layouts, assign_mask_layouts,
        mask_render_texture_count, part_is_descendant_of, stable_mask_texture_size,
    };
    use crate::cubism::CubismPartInfo;
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
        assert!(probe.extended_blend_count <= probe.drawable_count + runtime.offscreens().len());
        assert!(probe.masked_count <= probe.drawable_count);
    }

    #[test]
    fn mask_texture_size_is_bucketed_for_resize_stability() {
        assert_eq!(stable_mask_texture_size(1), 512);
        assert_eq!(stable_mask_texture_size(511), 512);
        assert_eq!(stable_mask_texture_size(512), 512);
        assert_eq!(stable_mask_texture_size(576), 1024);
        assert_eq!(stable_mask_texture_size(768), 1024);
        assert_eq!(stable_mask_texture_size(1025), 2048);
        assert_eq!(stable_mask_texture_size(4096), MAX_MASK_TEXTURE_SIZE);
    }

    #[test]
    fn mask_placement_uses_physical_mask_precision_when_tile_has_room() {
        let placement = MaskPlacement::new(
            Bounds {
                min_x: 10.0,
                min_y: 20.0,
                max_x: 20.0,
                max_y: 30.0,
            },
            LayoutBounds {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 0.5,
            },
            1024,
            Some(10.0),
        );

        assert_eq!(placement.bounds.min_x, 10.0);
        assert_eq!(placement.bounds.max_x, 20.0);
        assert!((placement.scale_x - 10.0 / 512.0).abs() < 0.0001);
        assert!((placement.scale_y - 10.0 / 512.0).abs() < 0.0001);
    }

    #[test]
    fn mask_placement_expands_axis_when_model_exceeds_tile_pixels() {
        let placement = MaskPlacement::new(
            Bounds {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 10.0,
                max_y: 10.0,
            },
            LayoutBounds {
                x: 0.0,
                y: 0.0,
                width: 0.25,
                height: 0.25,
            },
            512,
            Some(1000.0),
        );

        assert_eq!(placement.bounds.min_x, -0.5);
        assert_eq!(placement.bounds.max_x, 10.5);
        assert_eq!(placement.bounds.min_y, -0.5);
        assert_eq!(placement.bounds.max_y, 10.5);
        assert!((placement.scale_x - 0.25 / 11.0).abs() < 0.0001);
        assert!((placement.scale_y - 0.25 / 11.0).abs() < 0.0001);
    }

    #[test]
    fn high_precision_mask_layouts_use_full_red_textures() {
        let mut contexts = vec![
            MaskContext {
                masks: vec![1],
                bounds: Bounds {
                    min_x: 0.0,
                    min_y: 0.0,
                    max_x: 10.0,
                    max_y: 10.0,
                },
                buffer_index: 0,
                channel: MaskChannel::Green,
                layout_bounds: LayoutBounds {
                    x: 0.5,
                    y: 0.5,
                    width: 0.5,
                    height: 0.5,
                },
                matrix_for_mask: Affine2::identity(),
                matrix_for_draw: Affine2::identity(),
            },
            MaskContext {
                masks: vec![2],
                bounds: Bounds {
                    min_x: -5.0,
                    min_y: -5.0,
                    max_x: 5.0,
                    max_y: 5.0,
                },
                buffer_index: 0,
                channel: MaskChannel::Alpha,
                layout_bounds: LayoutBounds {
                    x: 0.75,
                    y: 0.75,
                    width: 0.25,
                    height: 0.25,
                },
                matrix_for_mask: Affine2::identity(),
                matrix_for_draw: Affine2::identity(),
            },
        ];

        assign_high_precision_mask_layouts(&mut contexts, 1024, Some(10.0));

        for context in contexts {
            assert!(matches!(context.channel, MaskChannel::Red));
            assert_eq!(context.layout_bounds.x, 0.0);
            assert_eq!(context.layout_bounds.y, 0.0);
            assert_eq!(context.layout_bounds.width, 1.0);
            assert_eq!(context.layout_bounds.height, 1.0);
        }
    }

    #[test]
    fn shared_masks_span_multiple_render_textures_after_default_capacity() {
        assert_eq!(mask_render_texture_count(0), 0);
        assert_eq!(mask_render_texture_count(36), 1);
        assert_eq!(mask_render_texture_count(37), 2);
        assert_eq!(mask_render_texture_count(64), 2);
        assert_eq!(mask_render_texture_count(65), 3);
    }

    #[test]
    fn shared_mask_layout_assigns_buffer_indices_across_textures() {
        let mut contexts: Vec<MaskContext> = (0..37)
            .map(|index| MaskContext {
                masks: vec![index as i32],
                bounds: Bounds {
                    min_x: 0.0,
                    min_y: 0.0,
                    max_x: 10.0,
                    max_y: 10.0,
                },
                buffer_index: 0,
                channel: MaskChannel::Red,
                layout_bounds: LayoutBounds {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                },
                matrix_for_mask: Affine2::identity(),
                matrix_for_draw: Affine2::identity(),
            })
            .collect();

        assign_mask_layouts(&mut contexts, 1024, Some(10.0));

        assert_eq!(
            contexts
                .iter()
                .filter(|context| context.buffer_index == 0)
                .count(),
            32
        );
        assert_eq!(
            contexts
                .iter()
                .filter(|context| context.buffer_index == 1)
                .count(),
            5
        );
    }

    #[test]
    fn part_descendant_walks_parent_chain() {
        let parts = vec![
            CubismPartInfo {
                index: 0,
                id: "Root".to_string(),
                parent_part_index: -1,
                offscreen_index: -1,
                opacity: 1.0,
            },
            CubismPartInfo {
                index: 1,
                id: "Head".to_string(),
                parent_part_index: 0,
                offscreen_index: 0,
                opacity: 1.0,
            },
            CubismPartInfo {
                index: 2,
                id: "Eye".to_string(),
                parent_part_index: 1,
                offscreen_index: -1,
                opacity: 1.0,
            },
        ];

        assert!(part_is_descendant_of(2, 1, &parts));
        assert!(part_is_descendant_of(2, 0, &parts));
        assert!(!part_is_descendant_of(1, 2, &parts));
        assert!(!part_is_descendant_of(-1, 0, &parts));
    }
}
