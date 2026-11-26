// Nearest-neighbor fullscreen retro upscale pass with sRGB surface compensation (`T03`).

struct UpscaleUniforms {
    output_is_srgb: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var internal_tex: texture_2d<f32>;
@group(0) @binding(1) var nearest_sampler: sampler;
@group(0) @binding(2) var<uniform> params: UpscaleUniforms;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VsOut;
    out.clip_position = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

fn srgb_channel_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(internal_tex, nearest_sampler, in.uv);
    if params.output_is_srgb != 0u {
        return vec4<f32>(
            srgb_channel_to_linear(c.r),
            srgb_channel_to_linear(c.g),
            srgb_channel_to_linear(c.b),
            c.a,
        );
    }
    return c;
}
