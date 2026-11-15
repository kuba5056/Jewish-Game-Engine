// Final pass: HDR scene colour to the displayable range.

struct Params {
    exposure: vec4<f32>, // x: exposure multiplier
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var hdr_texture: texture_2d<f32>;

// One triangle that covers the whole screen.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// ACES filmic curve (Narkowicz fit): keeps mid-tones, rolls off highlights
// smoothly instead of clipping them.
fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14),
                 vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let hdr = textureLoad(hdr_texture, vec2<i32>(position.xy), 0).rgb;
    return vec4<f32>(aces(hdr * params.exposure.x), 1.0);
}
