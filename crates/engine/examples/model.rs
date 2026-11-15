//! Loads a glTF model and places it in the world several times.
//!
//! While this runs (in a debug build), edit or replace
//! `assets/models/robot_panel.png` and the robots change on screen.
//!
//! Run from the repository root with: `cargo run --example model`

use engine::math::{Quat, Vec3};
use engine::{
    Config, Context, DirectionalLight, Entity, Game, KeyCode, Material, MeshData, MeshRenderer,
    Transform, WindowConfig,
};

#[derive(Default)]
struct ModelDemo {
    robots: Vec<Entity>,
}

impl Game for ModelDemo {
    fn init(&mut self, ctx: &mut Context) {
        ctx.renderer.clear_color = Vec3::new(0.25, 0.45, 0.8);
        ctx.camera.position = Vec3::new(0.0, 2.2, 6.5);
        ctx.camera.look_at(Vec3::new(0.0, 1.0, 0.0));

        let floor = MeshRenderer {
            mesh: ctx.renderer.create_mesh(&MeshData::plane(40.0, 1.0)),
            material: Material::colored(0.35, 0.4, 0.35),
        };
        ctx.world.spawn((Transform::IDENTITY, floor));
        ctx.world.spawn((
            Transform::IDENTITY.looking_to(Vec3::new(-0.5, -1.0, -0.6)),
            DirectionalLight::default(),
        ));

        let robot = match ctx.load_model("models/robot.gltf") {
            Ok(model) => model,
            Err(e) => {
                engine::log::error!("{e}");
                ctx.exit();
                return;
            }
        };
        // One model, three instances sharing the same GPU data.
        for (x, scale) in [(-2.5, 0.7), (0.0, 1.0), (2.5, 1.3)] {
            let transform = Transform::from_xyz(x, 0.0, 0.0).with_scale(Vec3::splat(scale));
            self.robots.push(robot.spawn(&mut ctx.world, transform));
        }
    }

    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.key_pressed(KeyCode::Escape) {
            ctx.exit();
        }
        // Lets automated runs stop on their own: MODEL_SECONDS=3.
        if let Some(limit) = std::env::var("MODEL_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
        {
            if ctx.time.elapsed().as_secs_f32() >= limit {
                ctx.exit();
            }
        }

        // Turning only the root entity turns the whole robot.
        let t = ctx.time.elapsed().as_secs_f32();
        for (i, &robot) in self.robots.iter().enumerate() {
            if let Ok(mut transform) = ctx.world.get::<&mut Transform>(robot) {
                transform.rotation = Quat::from_rotation_y(0.6 + t * 0.5 * (i as f32 - 1.0));
            }
        }
    }
}

fn main() -> Result<(), engine::Error> {
    let config = Config {
        window: WindowConfig {
            title: "Model".to_owned(),
            ..WindowConfig::default()
        },
        ..Config::default()
    };
    engine::run(config, ModelDemo::default())
}
