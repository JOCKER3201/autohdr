#[repr(align(4))]
struct AlignedShader([u8; include_bytes!("../shader.spv").len()]);

const ALIGNED_SHADER: AlignedShader = AlignedShader(*include_bytes!("../shader.spv"));

pub const SHADER_CODE: &[u32] = unsafe {
    std::slice::from_raw_parts(
        ALIGNED_SHADER.0.as_ptr() as *const u32,
        ALIGNED_SHADER.0.len() / 4,
    )
};
