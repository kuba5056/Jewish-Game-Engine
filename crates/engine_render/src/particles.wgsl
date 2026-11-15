// Particles: camera-facing soft discs, drawn after the opaque scene.

// The leading fields of the Globals uniform in mesh.wgsl.
struct Globals {
    view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
    camera_right: vec4<f32>,
    camera_up: vec4<f32>,
}

@group(0) @binding(0) var<uniform> globals: Globals;

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) offset: vec2<f32>, // -1..1 across the quad
    @location(1) color: vec4<f32>,
}

@vertex
fn vs_main(
    @builtin(vertex_index) index: u32,
    @location(0) position_size: vec4<f32>, // xyz: world centre, w: diameter
    @location(1) color: vec4<f32>,
) -> VertexOut {
    // Two triangles covering the quad; no vertex buffer needed.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let corner = corners[index];
    let radius = position_size.w * 0.5;
    let world = position_size.xyz
        + (globals.camera_right.xyz * corner.x + globals.camera_up.xyz * corner.y) * radius;

    var out: VertexOut;
    out.clip_position = globals.view_proj * vec4<f32>(world, 1.0);
    out.offset = corner;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Solid in the middle, fading out towards the rim.
    let fade = 1.0 - smoothstep(0.35, 1.0, length(in.offset));
    return vec4<f32>(in.color.rgb, in.color.a * fade);
}
