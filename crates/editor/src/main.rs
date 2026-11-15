//! A scene editor for the engine.
//!
//! Opens a scene file from `assets/`, lets you select, move, add, remove
//! and tweak entities, and saves the result. "Play" runs the scene with
//! physics and scripts; "Stop" puts it back as it was.
//!
//! Run from the repository root with:
//! `cargo run -p editor -- scenes/demo.ron`
//!
//! Mouse: left click selects, drag an arrow to move, hold right button to
//! look around (with WASD to fly, Shift to go faster), wheel to dolly.
//! Keys: F frames the selection, Delete removes it, Ctrl+D duplicates,
//! Ctrl+S saves.

mod gizmo;
mod panels;

use std::collections::HashMap;

use engine::math::Vec3;
use engine::{
    despawn_recursive, Collider, Config, Context, DirectionalLight, Entity, Game, GlobalTransform,
    KeyCode, MaterialDesc, Mesh, MeshData, ModelPart, MouseButton, Name, Parent, ParticleEmitter,
    PointLight, RigidBody, SceneFile, Script, Transform, Visual, WindowConfig,
};

/// A handle arrow being dragged.
struct Drag {
    axis: usize,
    /// Where on the axis the mouse grabbed it.
    grabbed_at: f32,
    /// The entity's world position and local translation at that moment.
    world_start: Vec3,
    local_start: Vec3,
}

/// The scene as it was when "Play" was pressed, to return to on "Stop".
struct PlaySession {
    scene: SceneFile,
    camera: engine::Camera,
}

struct Editor {
    /// Scene file being edited, relative to the asset directory.
    path: String,
    status: String,
    selected: Option<Entity>,
    playing: Option<PlaySession>,

    unit_cube: Option<Mesh>,
    drag: Option<Drag>,
    hovered_axis: Option<usize>,
    yaw: f32,
    pitch: f32,
    /// Text being typed into path fields, committed when editing ends.
    text: HashMap<&'static str, String>,
}

impl Editor {
    fn new(path: String) -> Self {
        Self {
            path,
            status: String::new(),
            selected: None,
            playing: None,
            unit_cube: None,
            drag: None,
            hovered_axis: None,
            yaw: 0.0,
            pitch: 0.0,
            text: HashMap::new(),
        }
    }

    fn select(&mut self, entity: Option<Entity>) {
        if self.selected != entity {
            self.selected = entity;
            self.text.clear();
            self.drag = None;
        }
    }

    // --- Scene files --------------------------------------------------------

    fn new_scene(&mut self, ctx: &mut Context) {
        ctx.world.clear();
        self.select(None);
        ctx.world.spawn((
            Name("Sun".to_owned()),
            Transform::IDENTITY.looking_to(Vec3::new(-0.5, -1.0, -0.4)),
            DirectionalLight::default(),
        ));
        ctx.world.spawn((
            Name("Floor".to_owned()),
            Transform::from_xyz(0.0, -0.5, 0.0).with_scale(Vec3::new(20.0, 1.0, 20.0)),
            Visual::Cube {
                size: 1.0,
                material: MaterialDesc::colored(0.4, 0.45, 0.4),
            },
            Collider::default(),
        ));
        self.status = "New scene".to_owned();
    }

    fn open(&mut self, ctx: &mut Context) {
        // Load into a fresh world only once the file is known to be good.
        let previous = std::mem::take(&mut ctx.world);
        match ctx.load_scene(&self.path) {
            Ok(entities) => {
                self.select(None);
                self.status = format!("Opened {} ({} entities)", self.path, entities.len());
            }
            Err(e) => {
                ctx.world = previous;
                self.status = e.to_string();
            }
        }
    }

    fn save(&mut self, ctx: &mut Context) {
        if self.playing.is_some() {
            self.status = "Stop the game before saving".to_owned();
            return;
        }
        self.status = match ctx.save_scene(&self.path) {
            Ok(()) => format!("Saved {}", self.path),
            Err(e) => e.to_string(),
        };
    }

    // --- Play mode ----------------------------------------------------------

    fn set_simulating(ctx: &mut Context, simulating: bool) {
        ctx.physics.paused = !simulating;
        ctx.scripts_paused = !simulating;
    }

    fn play(&mut self, ctx: &mut Context) {
        self.playing = Some(PlaySession {
            scene: SceneFile::from_world(&ctx.world),
            camera: ctx.camera.clone(),
        });
        self.select(None);
        Self::set_simulating(ctx, true);
        self.status = "Playing".to_owned();
    }

    fn stop(&mut self, ctx: &mut Context) {
        let Some(session) = self.playing.take() else {
            return;
        };
        Self::set_simulating(ctx, false);
        ctx.world.clear();
        session.scene.spawn(&mut ctx.world);
        ctx.camera = session.camera;
        self.select(None);
        self.status = "Stopped; the scene is as it was before Play".to_owned();
    }

    // --- Entities -----------------------------------------------------------

    /// A spot a few units in front of the camera, on or above the ground.
    fn spawn_point(ctx: &Context) -> Vec3 {
        let point = ctx.camera.position + ctx.camera.forward() * 6.0;
        point.with_y(point.y.max(0.5))
    }

    fn add(&mut self, ctx: &mut Context, kind: panels::NewEntity) {
        use panels::NewEntity::*;
        let at = Transform::from_translation(Self::spawn_point(ctx));
        let named = |name: &str| Name(name.to_owned());
        let material = MaterialDesc::default();
        let entity = match kind {
            Empty => ctx.world.spawn((named("Empty"), at)),
            Cube => ctx.world.spawn((
                named("Cube"),
                at,
                Visual::Cube {
                    size: 1.0,
                    material,
                },
            )),
            Sphere => ctx.world.spawn((
                named("Sphere"),
                at,
                Visual::Sphere {
                    radius: 0.5,
                    material,
                },
            )),
            Plane => ctx.world.spawn((
                named("Plane"),
                at,
                Visual::Plane {
                    size: 4.0,
                    uv_repeat: 1.0,
                    material,
                },
            )),
            Model => ctx.world.spawn((
                named("Model"),
                at.with_scale(Vec3::ONE),
                Visual::Model {
                    path: "models/robot.gltf".to_owned(),
                },
            )),
            Sun => ctx.world.spawn((
                named("Sun"),
                at.looking_to(Vec3::new(-0.5, -1.0, -0.4)),
                DirectionalLight::default(),
            )),
            Lamp => ctx.world.spawn((named("Lamp"), at, PointLight::default())),
            Particles => ctx
                .world
                .spawn((named("Particles"), at, ParticleEmitter::fire())),
        };
        self.select(Some(entity));
    }

    /// Copies an entity (not its children), one unit to the side.
    fn duplicate(&mut self, ctx: &mut Context, entity: Entity) {
        let world = &mut ctx.world;
        let Ok(transform) = world.get::<&Transform>(entity).map(|t| *t) else {
            return;
        };
        let mut moved = transform;
        moved.translation.x += 1.0;
        let copy = world.spawn((moved,));

        // Each component the editor knows about is copied if present.
        fn copy_one<T: Clone + Send + Sync + 'static>(
            world: &mut engine::World,
            from: Entity,
            to: Entity,
        ) {
            let component = world.get::<&T>(from).ok().map(|c| (*c).clone());
            if let Some(component) = component {
                let _ = world.insert_one(to, component);
            }
        }
        copy_one::<Name>(world, entity, copy);
        copy_one::<Parent>(world, entity, copy);
        copy_one::<Visual>(world, entity, copy);
        copy_one::<DirectionalLight>(world, entity, copy);
        copy_one::<PointLight>(world, entity, copy);
        copy_one::<Collider>(world, entity, copy);
        copy_one::<RigidBody>(world, entity, copy);
        copy_one::<ParticleEmitter>(world, entity, copy);
        copy_one::<Script>(world, entity, copy);
        self.select(Some(copy));
    }

    fn delete(&mut self, ctx: &mut Context, entity: Entity) {
        despawn_recursive(&mut ctx.world, entity);
        self.select(None);
    }

    /// World position of an entity, as of the last frame.
    fn world_position(ctx: &Context, entity: Entity) -> Option<Vec3> {
        let global = ctx.world.get::<&GlobalTransform>(entity).ok()?;
        Some(global.translation())
    }

    // --- Viewport -----------------------------------------------------------

    fn fly_camera(&mut self, ctx: &mut Context) {
        if ctx.input.button_pressed(MouseButton::Right) {
            ctx.set_cursor_captured(true);
            // Carry on from wherever the camera points now.
            let forward = ctx.camera.forward();
            self.yaw = (-forward.x).atan2(-forward.z);
            self.pitch = forward.y.clamp(-1.0, 1.0).asin();
        }
        if ctx.input.button_released(MouseButton::Right) {
            ctx.set_cursor_captured(false);
        }
        let dt = ctx.time.delta_secs();
        if ctx.input.button_down(MouseButton::Right) {
            let (dx, dy) = ctx.input.mouse_delta();
            self.yaw -= dx * 0.003;
            self.pitch = (self.pitch - dy * 0.003).clamp(-1.5, 1.5);
            ctx.camera.set_yaw_pitch(self.yaw, self.pitch);

            let axis = |positive: KeyCode, negative: KeyCode| {
                ctx.input.key_down(positive) as i32 as f32
                    - ctx.input.key_down(negative) as i32 as f32
            };
            let direction = ctx.camera.forward() * axis(KeyCode::KeyW, KeyCode::KeyS)
                + ctx.camera.right() * axis(KeyCode::KeyD, KeyCode::KeyA)
                + Vec3::Y * axis(KeyCode::KeyE, KeyCode::KeyQ);
            let speed = if ctx.input.key_down(KeyCode::ShiftLeft) {
                20.0
            } else {
                6.0
            };
            ctx.camera.position += direction.normalize_or_zero() * speed * dt;
        }
        ctx.camera.position += ctx.camera.forward() * ctx.input.scroll() * 1.5;

        if ctx.input.key_pressed(KeyCode::KeyF) {
            if let Some(target) = self.selected.and_then(|e| Self::world_position(ctx, e)) {
                ctx.camera.position = target - ctx.camera.forward() * 6.0;
            }
        }
    }

    /// The ray from the camera through the mouse cursor.
    fn mouse_ray(ctx: &Context) -> Option<(Vec3, Vec3)> {
        let cursor = ctx.input.cursor()?;
        let (width, height) = ctx.window_size();
        Some(ctx.camera.screen_ray(cursor, width as f32, height as f32))
    }

    /// Selection by clicking and moving by dragging the handle.
    fn interact(&mut self, ctx: &mut Context) {
        let Some((origin, direction)) = Self::mouse_ray(ctx) else {
            return;
        };
        let handle = self.selected.and_then(|entity| {
            let position = Self::world_position(ctx, entity)?;
            Some((
                entity,
                position,
                gizmo::length(position, ctx.camera.position),
            ))
        });

        if !ctx.input.button_down(MouseButton::Left) {
            self.drag = None;
        }
        if let (Some(drag), Some(entity)) = (&self.drag, self.selected) {
            let axis = gizmo::AXES[drag.axis];
            if let Some(hit) = gizmo::closest_on_axis(drag.world_start, axis, origin, direction) {
                let mut offset = axis * (hit.along - drag.grabbed_at);
                // The Transform is relative to the parent, so the world-space
                // offset has to be expressed in the parent's space.
                let parent = ctx.world.get::<&Parent>(entity).ok().map(|p| p.0);
                if let Some(parent) = parent.and_then(|p| ctx.world.get::<&GlobalTransform>(p).ok())
                {
                    offset = parent.0.inverse().transform_vector3(offset);
                }
                if let Ok(mut transform) = ctx.world.get::<&mut Transform>(entity) {
                    transform.translation = drag.local_start + offset;
                }
            }
            return;
        }

        self.hovered_axis = handle.and_then(|(_, position, length)| {
            gizmo::hovered_axis(position, length, origin, direction)
        });
        if !ctx.input.button_pressed(MouseButton::Left) {
            return;
        }
        // A click on an arrow starts a drag; anywhere else it selects.
        if let (Some(axis), Some((entity, position, _))) = (self.hovered_axis, handle) {
            let grabbed = gizmo::closest_on_axis(position, gizmo::AXES[axis], origin, direction);
            let local = ctx.world.get::<&Transform>(entity).map(|t| t.translation);
            if let (Some(hit), Ok(local_start)) = (grabbed, local) {
                self.drag = Some(Drag {
                    axis,
                    grabbed_at: hit.along,
                    world_start: position,
                    local_start,
                });
            }
            return;
        }
        let picked = engine::pick(&ctx.world, origin, direction).map(|(entity, _)| {
            // Clicking part of a model selects the model.
            let mut entity = entity;
            while ctx.world.satisfies::<&ModelPart>(entity) {
                match ctx.world.get::<&Parent>(entity) {
                    Ok(parent) => entity = parent.0,
                    Err(_) => break,
                }
            }
            entity
        });
        self.select(picked);
    }

    fn shortcuts(&mut self, ctx: &mut Context) {
        let ctrl =
            ctx.input.key_down(KeyCode::ControlLeft) || ctx.input.key_down(KeyCode::ControlRight);
        if ctrl && ctx.input.key_pressed(KeyCode::KeyS) {
            self.save(ctx);
        }
        let Some(entity) = self.selected else { return };
        if ctx.input.key_pressed(KeyCode::Delete) {
            self.delete(ctx, entity);
        } else if ctrl && ctx.input.key_pressed(KeyCode::KeyD) {
            self.duplicate(ctx, entity);
        }
    }
}

impl Game for Editor {
    fn init(&mut self, ctx: &mut Context) {
        ctx.renderer.clear_color = Vec3::new(0.22, 0.4, 0.75);
        ctx.camera.position = Vec3::new(4.0, 5.0, 10.0);
        ctx.camera.look_at(Vec3::new(0.0, 1.0, 0.0));
        self.unit_cube = Some(ctx.renderer.create_mesh(&MeshData::cube(1.0)));
        Self::set_simulating(ctx, false);

        self.open(ctx);
        if ctx.world.is_empty() {
            let reason = std::mem::take(&mut self.status);
            self.new_scene(ctx);
            self.status = format!("{reason}; started a new scene");
        }
    }

    fn update(&mut self, ctx: &mut Context) {
        // Lets automated runs stop on their own: EDITOR_SECONDS=3.
        if let Some(limit) = std::env::var("EDITOR_SECONDS")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
        {
            if ctx.time.elapsed().as_secs_f32() >= limit {
                ctx.exit();
            }
        }
        if self
            .selected
            .is_some_and(|entity| !ctx.world.contains(entity))
        {
            self.select(None);
        }

        // While playing, the scene's own scripts may drive the camera.
        if self.playing.is_none() {
            self.fly_camera(ctx);
        }
        self.shortcuts(ctx);
        self.interact(ctx);

        for action in panels::draw(self, ctx) {
            match action {
                panels::Action::New => self.new_scene(ctx),
                panels::Action::Open => self.open(ctx),
                panels::Action::Save => self.save(ctx),
                panels::Action::Play => self.play(ctx),
                panels::Action::Stop => self.stop(ctx),
                panels::Action::Add(kind) => self.add(ctx, kind),
                panels::Action::Select(entity) => self.select(entity),
                panels::Action::Duplicate(entity) => self.duplicate(ctx, entity),
                panels::Action::Delete(entity) => self.delete(ctx, entity),
            }
        }

        // The handle is drawn from last frame's position, like everything
        // the camera sees this frame.
        if let (Some(cube), Some(entity)) = (&self.unit_cube, self.selected) {
            if let Some(position) = Self::world_position(ctx, entity) {
                let length = gizmo::length(position, ctx.camera.position);
                let active = self
                    .drag
                    .as_ref()
                    .map(|drag| drag.axis)
                    .or(self.hovered_axis);
                gizmo::draw(&mut ctx.renderer, cube, position, length, active);
            }
        }
    }
}

fn main() -> Result<(), engine::Error> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "scenes/demo.ron".to_owned());
    let config = Config {
        window: WindowConfig {
            title: "Editor".to_owned(),
            width: 1440,
            height: 810,
            ..WindowConfig::default()
        },
        // The editor has its own panels; the built-in overlay would overlap.
        debug_overlay_key: None,
        ..Config::default()
    };
    engine::run(config, Editor::new(path))
}
