//! Spinning textured cubes over a floor, built from entities, with a fly camera.
//!
//! Hold the right mouse button to look around, WASD to move, Space/Shift to
//! go up/down, Escape to quit.
//!
//! Run with: `cargo run --example cubes`

use engine::math::{Quat, Vec3, Vec4};
use engine::{
    Config, Context, Game, KeyCode, MeshData, MeshRenderer, MouseButton, Parent, Transform,
    WindowConfig,
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
        let white = ctx.renderer.white_texture();

        ctx.world.spawn((
            Transform::IDENTITY,
            MeshRenderer {
                mesh: floor,
                texture: checker.clone(),
                color: Vec4::ONE,
            },
        ));

        let colors = [
            Vec4::new(1.0, 0.35, 0.3, 1.0),
            Vec4::new(0.35, 1.0, 0.4, 1.0),
            Vec4::new(0.4, 0.5, 1.0, 1.0),
        ];
        for (i, color) in colors.into_iter().enumerate() {
            let parent = ctx.world.spawn((
                Transform::from_xyz((i as f32 - 1.0) * 2.5, 1.2, 0.0),
                Spin(Vec3::new(0.4, 0.5 + i as f32 * 0.3, 0.0)),
                MeshRenderer {
                    mesh: cube.clone(),
                    texture: checker.clone(),
                    color,
                },
            ));
            // A small cube attached to the big one: it has no Spin of its
            // own, yet orbits because its Transform is relative to the parent.
            ctx.world.spawn((
                Transform::from_xyz(0.0, 0.9, 0.0).with_scale(Vec3::splat(0.3)),
                Parent(parent),
                MeshRenderer {
                    mesh: cube.clone(),
                    texture: white.clone(),
                    color,
                },
            ));
        }

        // A flat-coloured, non-uniformly scaled box behind the cubes.
        ctx.world.spawn((
            Transform::from_xyz(0.0, 2.0, -4.0).with_scale(Vec3::new(1.0, 4.0, 1.0)),
            MeshRenderer {
                mesh: cube,
                texture: white,
                color: Vec4::new(1.0, 0.8, 0.2, 1.0),
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
