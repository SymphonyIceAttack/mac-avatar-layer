use crate::live2d_model::Live2dModel;

#[derive(Debug, Clone)]
pub struct CubismRuntimeInfo {
    pub status: CubismRuntimeStatus,
    pub core_version: Option<String>,
    pub latest_moc_version: Option<u32>,
    pub moc_version: Option<u32>,
    pub parameter_count: Option<i32>,
    pub part_count: Option<i32>,
    pub drawable_count: Option<i32>,
    pub canvas_size: Option<[f32; 2]>,
    pub pixels_per_unit: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CubismRuntimeStatus {
    Disabled,
    Loaded,
}

#[derive(Debug, Clone)]
pub struct CubismParameterInfo {
    pub id: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub value: f32,
}

#[derive(Debug, Clone)]
pub struct CubismDrawableInfo {
    pub index: usize,
    pub id: String,
    pub parent_part_index: i32,
    pub parent_part_id: Option<String>,
    pub blend_mode: CubismBlendMode,
    pub texture_index: i32,
    pub vertex_count: i32,
    pub index_count: i32,
    pub opacity: f32,
    pub draw_order: i32,
    pub render_order: i32,
    pub masks: Vec<i32>,
    pub flags: DrawableFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum CubismBlendMode {
    Normal,
    Additive,
    Multiplicative,
    Unknown(i32),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DrawableFlags {
    pub visible: bool,
    pub double_sided: bool,
    pub blend_additive: bool,
    pub blend_multiplicative: bool,
    pub inverted_mask: bool,
}

#[derive(Debug, Clone)]
pub struct CubismDrawableFrame {
    pub positions: Vec<[f32; 2]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u16>,
}

impl CubismRuntimeInfo {
    #[cfg(not(feature = "cubism-core"))]
    pub fn disabled() -> Self {
        Self {
            status: CubismRuntimeStatus::Disabled,
            core_version: None,
            latest_moc_version: None,
            moc_version: None,
            parameter_count: None,
            part_count: None,
            drawable_count: None,
            canvas_size: None,
            pixels_per_unit: None,
        }
    }

    pub fn summary(&self) -> String {
        match self.status {
            CubismRuntimeStatus::Disabled => {
                "Cubism Core: disabled; texture atlas placeholder mode".to_string()
            }
            CubismRuntimeStatus::Loaded => {
                let canvas = self
                    .canvas_size
                    .map(|size| format!("{:.0}x{:.0}", size[0], size[1]))
                    .unwrap_or_else(|| "unknown".to_string());
                let ppu = self
                    .pixels_per_unit
                    .map(|value| format!("{value:.1}"))
                    .unwrap_or_else(|| "unknown".to_string());
                let latest_moc = self
                    .latest_moc_version
                    .map(|version| version.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                format!(
                    "Cubism Core {} | moc {}/latest {} | params {} | parts {} | drawables {} | canvas {} @ {} ppu",
                    self.core_version.as_deref().unwrap_or("unknown"),
                    self.moc_version
                        .map(|version| version.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    latest_moc,
                    self.parameter_count
                        .map(|count| count.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    self.part_count
                        .map(|count| count.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    self.drawable_count
                        .map(|count| count.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    canvas,
                    ppu,
                )
            }
        }
    }
}

#[cfg(feature = "cubism-core")]
mod core {
    use super::{
        CubismBlendMode, CubismDrawableFrame, CubismDrawableInfo, CubismParameterInfo,
        CubismRuntimeInfo, CubismRuntimeStatus, DrawableFlags,
    };
    use crate::live2d_model::Live2dModel;
    use live2d_cubism_core_sys as sys;
    use std::ffi::{CStr, c_uint, c_void};
    use std::fs;
    use std::slice;

    pub struct CubismModelRuntime {
        _moc_memory: AlignedMemory,
        _model_memory: AlignedMemory,
        model: *mut sys::csmModel,
        info: CubismRuntimeInfo,
    }

    impl CubismModelRuntime {
        pub fn load(model: &Live2dModel) -> Result<Self, String> {
            let moc_bytes = fs::read(&model.moc)
                .map_err(|error| format!("Failed to read moc {}: {error}", model.moc.display()))?;
            let mut moc_memory = AlignedMemory::from_bytes(moc_bytes, sys::ALIGN_OF_MOC)?;
            let moc_size = usize_to_c_uint(moc_memory.len(), "moc size")?;

            let moc_version = unsafe { sys::csmGetMocVersion(moc_memory.as_ptr(), moc_size) };
            let is_consistent =
                unsafe { sys::csmHasMocConsistency(moc_memory.as_mut_ptr(), moc_size) };
            if is_consistent == 0 {
                return Err(format!(
                    "Cubism Core rejected inconsistent moc file: {}",
                    model.moc.display()
                ));
            }

            let moc = unsafe { sys::csmReviveMocInPlace(moc_memory.as_mut_ptr(), moc_size) };
            if moc.is_null() {
                return Err(format!(
                    "Cubism Core failed to revive moc file: {}",
                    model.moc.display()
                ));
            }

            let model_size = unsafe { sys::csmGetSizeofModel(moc) };
            if model_size == 0 {
                return Err("Cubism Core returned zero model memory size".to_string());
            }

            let mut model_memory = AlignedMemory::zeroed(model_size as usize, sys::ALIGN_OF_MODEL)?;
            let model_ptr = unsafe {
                sys::csmInitializeModelInPlace(moc, model_memory.as_mut_ptr(), model_size)
            };
            if model_ptr.is_null() {
                return Err("Cubism Core failed to initialize model in place".to_string());
            }

            unsafe {
                sys::csmUpdateModel(model_ptr);
            }

            let mut canvas_size = sys::csmVector2::default();
            let mut canvas_origin = sys::csmVector2::default();
            let mut pixels_per_unit = 0.0_f32;
            unsafe {
                sys::csmReadCanvasInfo(
                    model_ptr,
                    &mut canvas_size,
                    &mut canvas_origin,
                    &mut pixels_per_unit,
                );
            }

            let info = CubismRuntimeInfo {
                status: CubismRuntimeStatus::Loaded,
                core_version: Some(format_core_version(unsafe { sys::csmGetVersion() })),
                latest_moc_version: Some(unsafe { sys::csmGetLatestMocVersion() }),
                moc_version: Some(moc_version),
                parameter_count: Some(unsafe { sys::csmGetParameterCount(model_ptr) }),
                part_count: Some(unsafe { sys::csmGetPartCount(model_ptr) }),
                drawable_count: Some(unsafe { sys::csmGetDrawableCount(model_ptr) }),
                canvas_size: Some([canvas_size.x, canvas_size.y]),
                pixels_per_unit: Some(pixels_per_unit),
            };

            Ok(Self {
                _moc_memory: moc_memory,
                _model_memory: model_memory,
                model: model_ptr,
                info,
            })
        }

        pub fn info(&self) -> &CubismRuntimeInfo {
            &self.info
        }

        pub fn update(&mut self) {
            unsafe {
                sys::csmUpdateModel(self.model);
            }
        }

        pub fn set_parameter_value(&mut self, id: &str, value: f32) -> bool {
            let count = unsafe { sys::csmGetParameterCount(self.model) };
            if count <= 0 {
                return false;
            }

            let len = count as usize;
            let ids = unsafe { ptr_array(sys::csmGetParameterIds(self.model), len) };
            let mins = unsafe { value_array(sys::csmGetParameterMinimumValues(self.model), len) };
            let maxes = unsafe { value_array(sys::csmGetParameterMaximumValues(self.model), len) };
            let values =
                unsafe { slice::from_raw_parts_mut(sys::csmGetParameterValues(self.model), len) };

            for index in 0..len {
                if unsafe { cstr_to_string(ids[index]) } == id {
                    values[index] = value.clamp(mins[index], maxes[index]);
                    return true;
                }
            }

            false
        }

        pub fn parameter(&self, id: &str) -> Option<CubismParameterInfo> {
            let count = unsafe { sys::csmGetParameterCount(self.model) };
            if count <= 0 {
                return None;
            }

            let len = count as usize;
            let ids = unsafe { ptr_array(sys::csmGetParameterIds(self.model), len) };
            let mins = unsafe { value_array(sys::csmGetParameterMinimumValues(self.model), len) };
            let maxes = unsafe { value_array(sys::csmGetParameterMaximumValues(self.model), len) };
            let defaults =
                unsafe { value_array(sys::csmGetParameterDefaultValues(self.model), len) };
            let values = unsafe { value_array(sys::csmGetParameterValues(self.model), len) };

            for index in 0..len {
                let parameter_id = unsafe { cstr_to_string(ids[index]) };
                if parameter_id == id {
                    return Some(CubismParameterInfo {
                        id: parameter_id,
                        min: mins[index],
                        max: maxes[index],
                        default: defaults[index],
                        value: values[index],
                    });
                }
            }

            None
        }

        pub fn parameters(&self) -> Vec<CubismParameterInfo> {
            let count = unsafe { sys::csmGetParameterCount(self.model) };
            if count <= 0 {
                return Vec::new();
            }

            let len = count as usize;
            let ids = unsafe { ptr_array(sys::csmGetParameterIds(self.model), len) };
            let mins = unsafe { value_array(sys::csmGetParameterMinimumValues(self.model), len) };
            let maxes = unsafe { value_array(sys::csmGetParameterMaximumValues(self.model), len) };
            let defaults =
                unsafe { value_array(sys::csmGetParameterDefaultValues(self.model), len) };
            let values = unsafe { value_array(sys::csmGetParameterValues(self.model), len) };

            (0..len)
                .map(|index| CubismParameterInfo {
                    id: unsafe { cstr_to_string(ids[index]) },
                    min: mins[index],
                    max: maxes[index],
                    default: defaults[index],
                    value: values[index],
                })
                .collect()
        }

        pub fn drawables(&self) -> Vec<CubismDrawableInfo> {
            let count = unsafe { sys::csmGetDrawableCount(self.model) };
            if count <= 0 {
                return Vec::new();
            }

            let len = count as usize;
            let ids = unsafe { ptr_array(sys::csmGetDrawableIds(self.model), len) };
            let part_count = unsafe { sys::csmGetPartCount(self.model) }.max(0) as usize;
            let part_ids = unsafe { ptr_array(sys::csmGetPartIds(self.model), part_count) };
            let parent_part_indices =
                unsafe { value_array(sys::csmGetDrawableParentPartIndices(self.model), len) };
            let constant_flags =
                unsafe { value_array(sys::csmGetDrawableConstantFlags(self.model), len) };
            let dynamic_flags =
                unsafe { value_array(sys::csmGetDrawableDynamicFlags(self.model), len) };
            let blend_modes =
                unsafe { value_array(sys::csmGetDrawableBlendModes(self.model), len) };
            let texture_indices =
                unsafe { value_array(sys::csmGetDrawableTextureIndices(self.model), len) };
            let vertex_counts =
                unsafe { value_array(sys::csmGetDrawableVertexCounts(self.model), len) };
            let index_counts =
                unsafe { value_array(sys::csmGetDrawableIndexCounts(self.model), len) };
            let opacities = unsafe { value_array(sys::csmGetDrawableOpacities(self.model), len) };
            let draw_orders =
                unsafe { value_array(sys::csmGetDrawableDrawOrders(self.model), len) };
            let render_orders = unsafe { value_array(sys::csmGetRenderOrders(self.model), len) };
            let mask_counts =
                unsafe { value_array(sys::csmGetDrawableMaskCounts(self.model), len) };
            let masks = unsafe { ptr_array(sys::csmGetDrawableMasks(self.model), len) };

            (0..len)
                .map(|index| CubismDrawableInfo {
                    index,
                    id: unsafe { cstr_to_string(ids[index]) },
                    parent_part_index: *parent_part_indices.get(index).unwrap_or(&-1),
                    parent_part_id: parent_part_indices.get(index).and_then(|part_index| {
                        (*part_index >= 0)
                            .then_some(*part_index as usize)
                            .filter(|part_index| *part_index < part_count)
                            .map(|part_index| unsafe { cstr_to_string(part_ids[part_index]) })
                    }),
                    blend_mode: cubism_blend_mode(blend_modes[index]),
                    texture_index: texture_indices[index],
                    vertex_count: vertex_counts[index],
                    index_count: index_counts[index],
                    opacity: opacities[index],
                    draw_order: draw_orders[index],
                    render_order: *render_orders.get(index).unwrap_or(&(index as i32)),
                    masks: unsafe {
                        value_array(masks[index], mask_counts[index].max(0) as usize).to_vec()
                    },
                    flags: drawable_flags(constant_flags[index], dynamic_flags[index]),
                })
                .collect()
        }

        pub fn drawable_frame_by_index(
            &self,
            drawable_index: usize,
        ) -> Option<CubismDrawableFrame> {
            let count = unsafe { sys::csmGetDrawableCount(self.model) };
            if drawable_index >= count.max(0) as usize {
                return None;
            }

            let len = count as usize;
            let vertex_counts =
                unsafe { value_array(sys::csmGetDrawableVertexCounts(self.model), len) };
            let index_counts =
                unsafe { value_array(sys::csmGetDrawableIndexCounts(self.model), len) };
            let positions_ptrs =
                unsafe { ptr_array(sys::csmGetDrawableVertexPositions(self.model), len) };
            let uv_ptrs = unsafe { ptr_array(sys::csmGetDrawableVertexUvs(self.model), len) };
            let index_ptrs = unsafe { ptr_array(sys::csmGetDrawableIndices(self.model), len) };

            let vertex_count = vertex_counts[drawable_index].max(0) as usize;
            let index_count = index_counts[drawable_index].max(0) as usize;
            let positions = unsafe { vector2_array(positions_ptrs[drawable_index], vertex_count) };
            let uvs = unsafe { vector2_array(uv_ptrs[drawable_index], vertex_count) };
            let indices = unsafe { value_array(index_ptrs[drawable_index], index_count) }.to_vec();

            Some(CubismDrawableFrame {
                positions,
                uvs,
                indices,
            })
        }
    }

    fn drawable_flags(constant: sys::csmFlags, dynamic: sys::csmFlags) -> DrawableFlags {
        DrawableFlags {
            visible: dynamic & sys::IS_VISIBLE != 0,
            double_sided: constant & sys::IS_DOUBLE_SIDED != 0,
            blend_additive: constant & sys::BLEND_ADDITIVE != 0,
            blend_multiplicative: constant & sys::BLEND_MULTIPLICATIVE != 0,
            inverted_mask: constant & sys::IS_INVERTED_MASK != 0,
        }
    }

    fn cubism_blend_mode(value: i32) -> CubismBlendMode {
        match value {
            0 => CubismBlendMode::Normal,
            1 => CubismBlendMode::Additive,
            2 => CubismBlendMode::Multiplicative,
            other => CubismBlendMode::Unknown(other),
        }
    }

    fn format_core_version(version: sys::csmVersion) -> String {
        let major = (version >> 24) & 0xff;
        let minor = (version >> 16) & 0xff;
        let patch = version & 0xffff;
        format!("{major}.{minor}.{patch}")
    }

    struct AlignedMemory {
        storage: Vec<u8>,
        offset: usize,
        len: usize,
    }

    impl AlignedMemory {
        fn from_bytes(bytes: Vec<u8>, alignment: usize) -> Result<Self, String> {
            let mut memory = Self::zeroed(bytes.len(), alignment)?;
            memory.as_slice_mut().copy_from_slice(&bytes);
            Ok(memory)
        }

        fn zeroed(len: usize, alignment: usize) -> Result<Self, String> {
            if !alignment.is_power_of_two() {
                return Err(format!("Alignment must be a power of two: {alignment}"));
            }

            let storage = vec![0_u8; len + alignment - 1];
            let base = storage.as_ptr() as usize;
            let aligned = (base + alignment - 1) & !(alignment - 1);
            let offset = aligned - base;

            Ok(Self {
                storage,
                offset,
                len,
            })
        }

        fn len(&self) -> usize {
            self.len
        }

        fn as_ptr(&self) -> *const c_void {
            self.storage[self.offset..].as_ptr().cast::<c_void>()
        }

        fn as_mut_ptr(&mut self) -> *mut c_void {
            self.storage[self.offset..].as_mut_ptr().cast::<c_void>()
        }

        fn as_slice_mut(&mut self) -> &mut [u8] {
            &mut self.storage[self.offset..self.offset + self.len]
        }
    }

    fn usize_to_c_uint(value: usize, label: &str) -> Result<c_uint, String> {
        c_uint::try_from(value)
            .map_err(|_| format!("{label} is too large for Cubism Core: {value}"))
    }

    unsafe fn ptr_array<T>(ptr: *const *const T, len: usize) -> &'static [*const T] {
        if ptr.is_null() || len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(ptr, len) }
        }
    }

    unsafe fn value_array<T>(ptr: *const T, len: usize) -> &'static [T] {
        if ptr.is_null() || len == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(ptr, len) }
        }
    }

    unsafe fn vector2_array(ptr: *const sys::csmVector2, len: usize) -> Vec<[f32; 2]> {
        if ptr.is_null() || len == 0 {
            return Vec::new();
        }

        unsafe { slice::from_raw_parts(ptr, len) }
            .iter()
            .map(|point| [point.x, point.y])
            .collect()
    }

    unsafe fn cstr_to_string(ptr: *const std::os::raw::c_char) -> String {
        if ptr.is_null() {
            return String::new();
        }

        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }

    pub use CubismModelRuntime as Runtime;
}

#[cfg(not(feature = "cubism-core"))]
mod core {
    use super::CubismRuntimeInfo;
    use crate::live2d_model::Live2dModel;

    pub struct Runtime {
        info: CubismRuntimeInfo,
    }

    impl Runtime {
        pub fn load(_model: &Live2dModel) -> Result<Self, String> {
            Ok(Self {
                info: CubismRuntimeInfo::disabled(),
            })
        }

        pub fn info(&self) -> &CubismRuntimeInfo {
            &self.info
        }

        pub fn update(&mut self) {}

        pub fn set_parameter_value(&mut self, _id: &str, _value: f32) -> bool {
            false
        }

        pub fn parameter(&self, _id: &str) -> Option<super::CubismParameterInfo> {
            None
        }

        pub fn parameters(&self) -> Vec<super::CubismParameterInfo> {
            Vec::new()
        }

        pub fn drawables(&self) -> Vec<super::CubismDrawableInfo> {
            Vec::new()
        }

        pub fn drawable_frame_by_index(
            &self,
            _drawable_index: usize,
        ) -> Option<super::CubismDrawableFrame> {
            None
        }

        #[cfg(test)]
        pub fn is_disabled(&self) -> bool {
            self.info.status == super::CubismRuntimeStatus::Disabled
        }
    }
}

pub use core::Runtime as CubismModelRuntime;

pub fn load_runtime(model: &Live2dModel) -> Result<CubismModelRuntime, String> {
    CubismModelRuntime::load(model)
}

#[cfg(test)]
mod tests {
    #[cfg(not(feature = "cubism-core"))]
    use super::{CubismRuntimeStatus, load_runtime};
    #[cfg(not(feature = "cubism-core"))]
    use crate::live2d_model::Live2dModel;

    #[test]
    #[cfg(not(feature = "cubism-core"))]
    fn default_runtime_is_disabled_without_sdk() {
        let model = Live2dModel::load("public/model/0.model3.json")
            .expect("public model should load for disabled runtime test");
        let runtime = load_runtime(&model).expect("disabled runtime should load");

        assert!(runtime.is_disabled());
        assert_eq!(runtime.info().status, CubismRuntimeStatus::Disabled);
    }
}
