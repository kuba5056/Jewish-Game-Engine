struct Globals {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,   // xyz: direction the light travels, normalised
    light_color: vec4<f32>,
    ambient: vec4<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var base_texture: texture_2d<f32>;
@group(1) @binding(1) var base_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // Per-instance.
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(7) normal_0: vec4<f32>,
    @location(8) normal_1: vec4<f32>,
    @location(9) normal_2: vec4<f32>,
    @location(10) color: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    let model = mat4x4<f32>(in.model_0, in.model_1, in.model_2, in.model_3);
    let normal_matrix = mat3x3<f32>(in.normal_0.xyz, in.normal_1.xyz, in.normal_2.xyz);

    var out: VertexOut;
    out.clip_position = globals.view_proj * model * vec4<f32>(in.position, 1.0);
    out.normal = normal_matrix * in.normal;
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let base = textureSample(base_texture, base_sampler, in.uv) * in.color;
    let diffuse = max(dot(normalize(in.normal), -globals.light_dir.xyz), 0.0);
    let light = globals.ambient.rgb + globals.light_color.rgb * diffuse;
    return vec4<f32>(base.rgb * light, base.a);
}
