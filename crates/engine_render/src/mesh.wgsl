// Main pass: physically based shading into an HDR target.

const PI: f32 = 3.14159265;
const MAX_POINT_LIGHTS: u32 = 16u;

struct PointLight {
    position_range: vec4<f32>, // xyz: world position, w: range
    color: vec4<f32>,          // rgb: colour times intensity
}

struct Globals {
    view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_position: vec4<f32>,
    ambient_sky: vec4<f32>,
    ambient_ground: vec4<f32>,
    sun_direction: vec4<f32>, // xyz: direction the light travels; w: 1 if present
    sun_color: vec4<f32>,     // rgb: colour times intensity
    // x: shadow map texel size in UV, y: normal offset in world units,
    // z: 1 if shadows are on.
    shadow: vec4<f32>,
    point_count: vec4<u32>,
    points: array<PointLight, MAX_POINT_LIGHTS>,
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
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
    @location(10) base_color: vec4<f32>,
    @location(11) surface: vec4<f32>,  // x: metallic, y: roughness
    @location(12) emissive: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) base_color: vec4<f32>,
    @location(4) surface: vec4<f32>,
    @location(5) emissive: vec4<f32>,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    let model = mat4x4<f32>(in.model_0, in.model_1, in.model_2, in.model_3);
    let normal_matrix = mat3x3<f32>(in.normal_0.xyz, in.normal_1.xyz, in.normal_2.xyz);
    let world = model * vec4<f32>(in.position, 1.0);

    var out: VertexOut;
    out.clip_position = globals.view_proj * world;
    out.world_position = world.xyz;
    out.normal = normal_matrix * in.normal;
    out.uv = in.uv;
    out.base_color = in.base_color;
    out.surface = in.surface;
    out.emissive = in.emissive;
    return out;
}

// GGX microfacet distribution.
fn distribution(n_dot_h: f32, alpha: f32) -> f32 {
    let a2 = alpha * alpha;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

// Smith-Schlick geometry term for one direction.
fn geometry(n_dot_x: f32, k: f32) -> f32 {
    return n_dot_x / (n_dot_x * (1.0 - k) + k);
}

fn fresnel(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

// Light reflected towards the viewer from one light. `l` points to the light.
fn shade(
    n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, radiance: vec3<f32>,
    diffuse_color: vec3<f32>, f0: vec3<f32>, roughness: f32,
) -> vec3<f32> {
    let n_dot_l = max(dot(n, l), 0.0);
    if n_dot_l <= 0.0 {
        return vec3<f32>(0.0);
    }
    let h = normalize(v + l);
    let n_dot_v = max(dot(n, v), 1e-4);
    let n_dot_h = max(dot(n, h), 0.0);

    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    let f = fresnel(max(dot(h, v), 0.0), f0);
    let specular = distribution(n_dot_h, roughness * roughness)
        * geometry(n_dot_v, k) * geometry(n_dot_l, k) * f
        / (4.0 * n_dot_v * n_dot_l + 1e-4);
    // Energy that was reflected specularly is not available for diffuse.
    let diffuse = (vec3<f32>(1.0) - f) * diffuse_color / PI;
    return (diffuse + specular) * radiance * n_dot_l;
}

// 1 where the sun reaches `world_position`, 0 in shadow, in between at edges.
fn sun_visibility(world_position: vec3<f32>, n: vec3<f32>) -> f32 {
    if globals.shadow.z < 0.5 {
        return 1.0;
    }
    // Sampling from a point pushed off the surface avoids self-shadowing.
    let offset_position = world_position + n * globals.shadow.y;
    let clip = globals.light_view_proj * vec4<f32>(offset_position, 1.0);
    let uv = clip.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    if uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || clip.z < 0.0 || clip.z > 1.0 {
        return 1.0; // Outside the shadowed area around the camera.
    }
    // 3x3 percentage-closer filter for soft edges.
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let offset = vec2<f32>(f32(x), f32(y)) * globals.shadow.x;
            lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + offset, clip.z);
        }
    }
    return lit / 9.0;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let base = textureSample(base_texture, base_sampler, in.uv) * in.base_color;
    let metallic = clamp(in.surface.x, 0.0, 1.0);
    // Perfectly smooth surfaces make point-like highlights that alias badly.
    let roughness = clamp(in.surface.y, 0.045, 1.0);

    let n = normalize(in.normal);
    let v = normalize(globals.camera_position.xyz - in.world_position);
    let n_dot_v = max(dot(n, v), 0.0);

    // Metals have no diffuse reflection and tint their specular reflection.
    let diffuse_color = base.rgb * (1.0 - metallic);
    let f0 = mix(vec3<f32>(0.04), base.rgb, metallic);

    var color = in.emissive.rgb;

    // Ambient: a sky/ground gradient stands in for the environment, sampled
    // along the normal for diffuse and along the reflection for specular.
    let sky = globals.ambient_sky.rgb;
    let ground = globals.ambient_ground.rgb;
    let r = reflect(-v, n);
    let f_ambient = f0 + (max(vec3<f32>(1.0 - roughness), f0) - f0) * pow(1.0 - n_dot_v, 5.0);
    color += mix(ground, sky, n.y * 0.5 + 0.5) * diffuse_color;
    color += mix(ground, sky, r.y * 0.5 + 0.5) * f_ambient;

    if globals.sun_direction.w > 0.5 {
        let visibility = sun_visibility(in.world_position, n);
        let l = -globals.sun_direction.xyz;
        color += shade(n, v, l, globals.sun_color.rgb, diffuse_color, f0, roughness) * visibility;
    }

    for (var i = 0u; i < globals.point_count.x; i++) {
        let light = globals.points[i];
        let to_light = light.position_range.xyz - in.world_position;
        let distance = length(to_light);
        let range = light.position_range.w;
        if distance >= range {
            continue;
        }
        // Inverse-square falloff, faded to exactly zero at the range.
        let window = 1.0 - pow(distance / range, 4.0);
        let attenuation = window * window / max(distance * distance, 1e-4);
        color += shade(n, v, to_light / distance, light.color.rgb * attenuation,
                       diffuse_color, f0, roughness);
    }

    return vec4<f32>(color, 1.0);
}
