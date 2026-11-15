// Shadow pass: depth only, seen from the sun.

@group(0) @binding(0) var<uniform> light_view_proj: mat4x4<f32>;
// See mesh.wgsl.
@group(0) @binding(1) var<storage, read> joint_matrices: array<mat4x4<f32>>;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(11) surface: vec4<f32>, // z: skinning, as in mesh.wgsl
    @location(13) joints: vec4<u32>,
    @location(14) weights: vec4<f32>,
}

@vertex
fn vs_main(in: VertexIn) -> @builtin(position) vec4<f32> {
    var world: vec4<f32>;
    if in.surface.z > 0.5 {
        let first = u32(in.surface.z) - 1u;
        let skin = joint_matrices[first + in.joints.x] * in.weights.x
            + joint_matrices[first + in.joints.y] * in.weights.y
            + joint_matrices[first + in.joints.z] * in.weights.z
            + joint_matrices[first + in.joints.w] * in.weights.w;
        world = skin * vec4<f32>(in.position, 1.0);
    } else {
        let model = mat4x4<f32>(in.model_0, in.model_1, in.model_2, in.model_3);
        world = model * vec4<f32>(in.position, 1.0);
    }
    return light_view_proj * world;
}
