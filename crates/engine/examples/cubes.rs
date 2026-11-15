//! Spinning textured cubes over a floor, built from entities, with a fly camera.
//!
//! Hold the right mouse button to look around, WASD to move, Space/Shift to
//! go up/down, Escape to quit.
//!
//! Run with: `cargo run --example cubes`

use engine::math::{Quat, Vec3, Vec4};
use engine::{
    Config, Context, DirectionalLight, Game, KeyCode, Material, MeshData, MeshRenderer,
    MouseButton, Parent, PointLight, Transform, WindowConfig,
};

/// Rotates its entity at a constant rate: axis times radians per second.
struct Spin(Vec3);

/// A reusable system: turns every entity that has a `Spin`.
fn spin_system(ctx: &mut Context) {
    let dt = ctx.time.delta_secs();
    for (transform, spin) in ctx.world.query_mut::<(&mut Transform, &Spin)>() {
        transform.rotation = (Quat::from_scaled_axis(spin.0 * dt) * transform.rotation).normalize();
    }
}

#[derive(Default)]
struct Cubes {
    yaw: f32,
    pitch: f32,
}

/// A two-colour checkerboard, `cells` squares on a side.
fn checkerboard(size: u32, cells: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let light = (x * cells / size + y * cells / size).is_multiple_of(2);
            let v = if light { 230 } else { 60 };
            pixels.extend([v, v, v, 255]);
        }
    }
    pixels
}

impl Game for Cubes {
    fn init(&mut self, ctx: &mut Context) {
        ctx.renderer.clear_color = Vec3::new(0.25, 0.45, 0.8);
        let cube = ctx.renderer.create_mesh(&MeshData::cube(1.0));
        let floor = ctx.renderer.create_mesh(&MeshData::plane(40.0, 20.0));
        let checker = ctx.renderer.create_texture(64, 64, &checkerboard(64, 4));

        ctx.world.spawn((
            Transform::IDENTITY,
            MeshRenderer {
                mesh: floor,
                material: Material {
                    roughness: 0.9,
                    ..Material::textured(checker.clone())
                },
            },
        ));

        // The sun: only its direction matters.
        ctx.world.spawn((
            Transform::IDENTITY.looking_to(Vec3::new(-0.5, -1.0, -0.4)),
            DirectionalLight::default(),
        ));

        // Left to right: matte plastic, glossy plastic, polished metal.
        let cubes = [
            (Vec4::new(1.0, 0.35, 0.3, 1.0), 0.0, 0.9),
            (Vec4::new(0.35, 1.0, 0.4, 1.0), 0.0, 0.25),
            (Vec4::new(0.4, 0.5, 1.0, 1.0), 1.0, 0.3),
        ];
        for (i, (base_color, metallic, roughness)) in cubes.into_iter().enumerate() {
            let parent = ctx.world.spawn((
                Transform::from_xyz((i as f32 - 1.0) * 2.5, 1.2, 0.0),
                Spin(Vec3::new(0.4, 0.5 + i as f32 * 0.3, 0.0)),
                MeshRenderer {
                    mesh: cube.clone(),
                    material: Material {
                        texture: Some(checker.clone()),
                        base_color,
                        metallic,
                        roughness,
                        ..Material::default()
                    },
                },
            ));
            // A small glowing cube attached to the big one: it has no Spin of
            // its own, yet orbits because its Transform is relative to the
            // parent. The PointLight on it lights whatever it passes.
            let glow = base_color.truncate();
            ctx.world.spawn((
                Transform::from_xyz(0.0, 0.9, 0.0).with_scale(Vec3::splat(0.2)),
                Parent(parent),
                MeshRenderer {
                    mesh: cube.clone(),
                    material: Material {
                        base_color: Vec4::new(0.0, 0.0, 0.0, 1.0),
                        emissive: glow * 4.0,
                        ..Material::default()
                    },
                },
                PointLight {
                    color: glow,
                    intensity: 1.5,
                    range: 4.0,
                },
            ));
        }

        // A non-uniformly scaled golden pillar behind the cubes.
        ctx.world.spawn((
            Transform::from_xyz(0.0, 2.0, -4.0).with_scale(Vec3::new(1.0, 4.0, 1.0)),
            MeshRenderer {
                mesh: cube,
                material: Material {
                    base_color: Vec4::new(1.0, 0.77, 0.34, 1.0),
                    metallic: 1.0,
                    roughness: 0.35,
                    ..Material::default()
                },
            },
        ));

        ctx.add_system(spin_system);

        ctx.camera.position = Vec3::new(0.0, 2.5, 7.0);
        ctx.camera.look_at(Vec3::new(0.0, 1.0, 0.0));
        // Continue mouse-look from where look_at left the camera.
        let forward = ctx.camera.forward();
        self.yaw = (-forward.x).atan2(-forward.z);
        self.pitch = forward.y.asin();
    }

    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.key_pressed(KeyCode::Escape) {
            ctx.exit();
        }
        // Lets automated runs stop on their own: CUBES_SECONDS=3.
        if let Some(limit) = std::env::var("CUBES_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
        {
            if ctx.time.elapsed().as_secs_f32() >= limit {
                ctx.exit();
            }
        }

        self.fly_camera(ctx);
    }
}

impl Cubes {
    fn fly_camera(&mut self, ctx: &mut Context) {
        if ctx.input.button_pressed(MouseButton::Right) {
            ctx.set_cursor_captured(true);
        }
        if ctx.input.button_released(MouseButton::Right) {
            ctx.set_cursor_captured(false);
        }
        if ctx.input.button_down(MouseButton::Right) {
            let (dx, dy) = ctx.input.mouse_delta();
            let sensitivity = 0.003;
            self.yaw -= dx * sensitivity;
            self.pitch = (self.pitch - dy * sensitivity).clamp(-1.5, 1.5);
            ctx.camera.set_yaw_pitch(self.yaw, self.pitch);
        }

        let axis = |positive: KeyCode, negative: KeyCode| {
            ctx.input.key_down(positive) as i32 as f32 - ctx.input.key_down(negative) as i32 as f32
        };
        let movement = ctx.camera.forward() * axis(KeyCode::KeyW, KeyCode::KeyS)
            + ctx.camera.right() * axis(KeyCode::KeyD, KeyCode::KeyA)
            + Vec3::Y * axis(KeyCode::Space, KeyCode::ShiftLeft);
        let speed = 5.0;
        ctx.camera.position += movement.normalize_or_zero() * speed * ctx.time.delta_secs();
    }
}

fn main() -> Result<(), engine::Error> {
    let config = Config {
        window: WindowConfig {
            title: "Cubes".to_owned(),
            ..WindowConfig::default()
        },
        ..Config::default()
    };
    engine::run(config, Cubes::default())
}
