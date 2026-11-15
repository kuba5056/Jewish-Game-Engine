// Shadow pass: depth only, seen from the sun.

@group(0) @binding(0) var<uniform> light_view_proj: mat4x4<f32>;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
}

@vertex
fn vs_main(in: VertexIn) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(in.model_0, in.model_1, in.model_2, in.model_3);
    return light_view_proj * model * vec4<f32>(in.position, 1.0);
}
