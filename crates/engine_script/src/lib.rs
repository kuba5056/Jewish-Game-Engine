//! Game logic in script files, written in [Rhai](https://rhai.rs).
//!
//! Give an entity a [`Script`] naming a `.rhai` file in the asset directory.
//! The file may define any of these functions; inside them `this` is a map
//! that belongs to that one entity and keeps its values between calls, and
//! `this.entity` is the entity itself:
//!
//! ```text
//! fn init() { this.speed = 3.0; }           // once, before anything else
//! fn update(dt) { ... }                      // every frame
//! fn fixed_update(dt) { ... }                // every physics step
//! fn on_collision(other) { ... }             // when it starts touching `other`
//! ```
//!
//! See [`api`] for what scripts can call. Script files are reloaded when
//! they change on disk, keeping each entity's `this`.

mod api;
mod keys;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

use engine_physics::{Collision, Physics};
use engine_platform::Input;
use engine_render::Camera;
use engine_scene::{Entity, World};
use glam::Vec3;
use rhai::{CallFnOptions, Dynamic, Map, Scope, AST};
use serde::{Deserialize, Serialize};

pub use api::EntityRef;

/// How often script files are checked for changes, in seconds.
const RELOAD_INTERVAL: f32 = 0.5;
/// A script call that runs longer than this many operations is stopped, so
/// an endless loop in a script cannot freeze the game.
const MAX_OPERATIONS: u64 = 5_000_000;

/// Runs a script file for this entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Script {
    /// Path of the `.rhai` file in the asset directory.
    pub path: String,
}

impl Script {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

/// Something a script asked for that only the engine can do. Returned from
/// each run and carried out afterwards.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    PlaySound {
        path: String,
        volume: f32,
    },
    /// Spawn a scene file's entities, moved by `offset`.
    SpawnScene {
        path: String,
        offset: Vec3,
    },
    Exit,
}

/// The parts of the engine scripts work on, lent for the length of one run.
pub struct Frame<'a> {
    pub world: &'a mut World,
    pub physics: &'a mut Physics,
    pub input: &'a Input,
    pub camera: &'a mut Camera,
    /// Seconds since the game started.
    pub elapsed: f32,
}

/// What the script API functions reach. While scripts run it holds the
/// engine's own world and physics (swapped in, not copied); in between it
/// holds empty stand-ins.
pub(crate) struct Host {
    pub(crate) world: World,
    pub(crate) physics: Physics,
    pub(crate) input: Input,
    pub(crate) camera: Camera,
    pub(crate) elapsed: f32,
    pub(crate) commands: Vec<Command>,
}

/// A compiled script file.
struct Program {
    /// `None` if the file could not be read or compiled.
    ast: Option<AST>,
    has_init: bool,
    has_update: bool,
    has_fixed_update: bool,
    has_on_collision: bool,
    modified: Option<SystemTime>,
    /// Goes up on every reload, so entities can tell their script changed.
    version: u64,
}

/// One entity's script state.
struct Instance {
    /// The map scripts see as `this`.
    this: Dynamic,
    initialized: bool,
    /// The program version this instance failed on; it is left alone until
    /// the file changes, rather than logging the same error every frame.
    failed_version: Option<u64>,
}

#[derive(Clone, Copy)]
enum Stage<'a> {
    Update,
    Fixed(&'a [Collision]),
}

/// Compiles and runs the scripts of every entity that has a [`Script`].
pub struct ScriptEngine {
    engine: rhai::Engine,
    host: Rc<RefCell<Host>>,
    root: PathBuf,
    hot_reload: bool,
    since_check: f32,
    programs: HashMap<String, Program>,
    instances: HashMap<Entity, Instance>,
    next_version: u64,
}

impl ScriptEngine {
    /// `root` is the directory script paths are relative to.
    pub fn new(root: impl Into<PathBuf>, hot_reload: bool) -> Self {
        let host = Rc::new(RefCell::new(Host {
            world: World::new(),
            physics: Physics::new(),
            input: Input::default(),
            camera: Camera::default(),
            elapsed: 0.0,
            commands: Vec::new(),
        }));
        let mut engine = rhai::Engine::new();
        engine.set_max_operations(MAX_OPERATIONS);
        engine.on_print(|text| log::info!("[script] {text}"));
        engine.on_debug(|text, _, position| log::debug!("[script] {position} {text}"));
        api::register(&mut engine, &host);
        Self {
            engine,
            host,
            root: root.into(),
            hot_reload,
            since_check: 0.0,
            programs: HashMap::new(),
            instances: HashMap::new(),
            next_version: 0,
        }
    }

    /// Runs every script's `init` (if new) and `update`. Call once per frame.
    pub fn update(&mut self, frame: Frame, dt: f32) -> Vec<Command> {
        self.reload_changed(dt);
        self.run(frame, dt, Stage::Update)
    }

    /// Runs every script's `on_collision` for the contacts in `collisions`
    /// that began, then its `fixed_update`. Call once per physics step.
    pub fn fixed_update(
        &mut self,
        frame: Frame,
        dt: f32,
        collisions: &[Collision],
    ) -> Vec<Command> {
        self.run(frame, dt, Stage::Fixed(collisions))
    }

    fn compile(&mut self, path: &str) {
        let file = self.root.join(path);
        let modified = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
        let source = std::fs::read_to_string(&file).map_err(|e| e.to_string());
        let ast = source.and_then(|text| self.engine.compile(text).map_err(|e| e.to_string()));
        let ast = match ast {
            Ok(ast) => Some(ast),
            Err(e) => {
                log::error!("{}: {e}", file.display());
                None
            }
        };
        let defines = |name: &str, params: usize| {
            ast.as_ref().is_some_and(|ast| {
                ast.iter_functions()
                    .any(|f| f.name == name && f.params.len() == params)
            })
        };
        self.next_version += 1;
        let program = Program {
            has_init: defines("init", 0),
            has_update: defines("update", 1),
            has_fixed_update: defines("fixed_update", 1),
            has_on_collision: defines("on_collision", 1),
            ast,
            modified,
            version: self.next_version,
        };
        self.programs.insert(path.to_owned(), program);
    }

    fn reload_changed(&mut self, dt: f32) {
        self.since_check += dt;
        if !self.hot_reload || self.since_check < RELOAD_INTERVAL {
            return;
        }
        self.since_check = 0.0;
        let changed: Vec<String> = self
            .programs
            .iter()
            .filter(|(path, program)| {
                let now = std::fs::metadata(self.root.join(path))
                    .and_then(|m| m.modified())
                    .ok();
                now != program.modified
            })
            .map(|(path, _)| path.clone())
            .collect();
        for path in changed {
            log::info!("reloading {path}");
            self.compile(&path);
        }
    }

    fn run(&mut self, frame: Frame, dt: f32, stage: Stage) -> Vec<Command> {
        // Lend the engine's state to the API functions by swapping it into
        // the host; it is swapped back below, so nothing is copied.
        {
            let mut host = self.host.borrow_mut();
            std::mem::swap(&mut host.world, frame.world);
            std::mem::swap(&mut host.physics, frame.physics);
            host.input = frame.input.clone();
            host.camera = frame.camera.clone();
            host.elapsed = frame.elapsed;
        }

        let scripted: Vec<(Entity, String)> = self
            .host
            .borrow()
            .world
            .query::<(Entity, &Script)>()
            .iter()
            .map(|(entity, script)| (entity, script.path.clone()))
            .collect();
        self.instances
            .retain(|entity, _| scripted.iter().any(|(e, _)| e == entity));

        for (entity, path) in &scripted {
            // An earlier script this frame may have despawned it.
            if !self.host.borrow().world.contains(*entity) {
                continue;
            }
            if !self.programs.contains_key(path) {
                self.compile(path);
            }
            let Self {
                engine,
                programs,
                instances,
                ..
            } = self;
            let program = &programs[path];
            let Some(ast) = &program.ast else { continue };
            let instance = instances.entry(*entity).or_insert_with(|| {
                let mut this = Map::new();
                this.insert("entity".into(), Dynamic::from(EntityRef(*entity)));
                Instance {
                    this: Dynamic::from_map(this),
                    initialized: false,
                    failed_version: None,
                }
            });
            if instance.failed_version == Some(program.version) {
                continue;
            }

            let mut call = |name: &str, args: Vec<Dynamic>| {
                let options = CallFnOptions::new()
                    .eval_ast(false)
                    .bind_this_ptr(&mut instance.this);
                engine.call_fn_with_options::<Dynamic>(options, &mut Scope::new(), ast, name, args)
            };
            let mut result = Ok(Dynamic::UNIT);
            if !instance.initialized && program.has_init {
                result = call("init", vec![]);
            }
            match stage {
                Stage::Update if result.is_ok() && program.has_update => {
                    result = call("update", vec![Dynamic::from_float(dt)]);
                }
                Stage::Fixed(collisions) if result.is_ok() => {
                    if program.has_on_collision {
                        let touched = collisions
                            .iter()
                            .filter(|collision| collision.started)
                            .filter_map(|collision| collision.other(*entity));
                        for other in touched {
                            result = call("on_collision", vec![Dynamic::from(EntityRef(other))]);
                            if result.is_err() {
                                break;
                            }
                        }
                    }
                    if result.is_ok() && program.has_fixed_update {
                        result = call("fixed_update", vec![Dynamic::from_float(dt)]);
                    }
                }
                _ => {}
            }
            instance.initialized = true;
            if let Err(e) = result {
                log::error!("{path}: {e} (script stopped until the file changes)");
                instance.failed_version = Some(program.version);
            }
        }

        let mut host = self.host.borrow_mut();
        std::mem::swap(&mut host.world, frame.world);
        std::mem::swap(&mut host.physics, frame.physics);
        *frame.camera = host.camera.clone();
        std::mem::take(&mut host.commands)
    }
}

#[cfg(test)]
mod tests {
    use engine_physics::{Collider, RigidBody};
    use engine_platform::KeyCode;
    use engine_scene::{Name, Transform};

    use super::*;

    const DT: f32 = 1.0 / 60.0;

    /// A scratch asset directory holding one script per test.
    struct Fixture {
        dir: PathBuf,
        scripts: ScriptEngine,
        world: World,
        physics: Physics,
        input: Input,
        camera: Camera,
    }

    impl Fixture {
        fn new(test: &str, source: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("engine_script_{test}_{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("test.rhai"), source).unwrap();
            Self {
                scripts: ScriptEngine::new(&dir, true),
                dir,
                world: World::new(),
                physics: Physics::new(),
                input: Input::default(),
                camera: Camera::default(),
            }
        }

        fn spawn_scripted(&mut self, at: Vec3) -> Entity {
            self.world
                .spawn((Transform::from_translation(at), Script::new("test.rhai")))
        }

        fn fixed_update(&mut self, collisions: &[Collision]) -> Vec<Command> {
            let frame = Frame {
                world: &mut self.world,
                physics: &mut self.physics,
                input: &self.input,
                camera: &mut self.camera,
                elapsed: 0.0,
            };
            self.scripts.fixed_update(frame, DT, collisions)
        }

        fn update(&mut self) -> Vec<Command> {
            let frame = Frame {
                world: &mut self.world,
                physics: &mut self.physics,
                input: &self.input,
                camera: &mut self.camera,
                elapsed: 0.0,
            };
            self.scripts.update(frame, DT)
        }

        fn position(&self, entity: Entity) -> Vec3 {
            self.world.get::<&Transform>(entity).unwrap().translation
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn init_runs_once_and_state_persists_per_entity() {
        let mut fx = Fixture::new(
            "state",
            r#"
            fn init() { this.count = 0; }
            fn update(dt) {
                this.count += 1;
                this.entity.position = vec3(this.count, 0, 0);
            }
            "#,
        );
        let a = fx.spawn_scripted(Vec3::ZERO);
        fx.update();
        fx.update();
        // A second entity with the same script starts its own count.
        let b = fx.spawn_scripted(Vec3::ZERO);
        fx.update();
        assert_eq!(fx.position(a).x, 3.0);
        assert_eq!(fx.position(b).x, 1.0);
    }

    #[test]
    fn scripts_read_input_and_move_their_entity() {
        let mut fx = Fixture::new(
            "input",
            r#"
            fn update(dt) {
                if key_down("D") { this.entity.position += vec3(2.0, 0.0, 0.0) * dt; }
                if key_pressed("Space") { this.entity.scale = vec3(2, 2, 2); }
            }
            "#,
        );
        let entity = fx.spawn_scripted(Vec3::ZERO);
        fx.update();
        assert_eq!(fx.position(entity), Vec3::ZERO);

        fx.input.press_key(KeyCode::KeyD);
        fx.input.press_key(KeyCode::Space);
        for _ in 0..60 {
            fx.update();
        }
        assert!((fx.position(entity).x - 2.0).abs() < 1e-3);
        assert_eq!(
            fx.world.get::<&Transform>(entity).unwrap().scale,
            Vec3::splat(2.0)
        );
    }

    #[test]
    fn scripts_find_other_entities_and_send_commands() {
        let mut fx = Fixture::new(
            "world",
            r#"
            fn update(dt) {
                let target = find("Target");
                if target != () {
                    this.entity.position = this.entity.position.lerp(target.position, 0.5);
                    if target.position.distance(this.entity.position) < 0.1 {
                        target.despawn();
                        play_sound("sounds/pickup.wav", 0.5);
                        spawn_scene("scenes/effect.ron", this.entity.position);
                    }
                }
                if find_all("Nothing").len() == 0 && key_pressed("Escape") { exit(); }
            }
            "#,
        );
        let chaser = fx.spawn_scripted(Vec3::ZERO);
        let target = fx.world.spawn((
            Transform::from_xyz(4.0, 0.0, 0.0),
            Name("Target".to_owned()),
        ));

        let mut commands = Vec::new();
        for _ in 0..10 {
            commands.extend(fx.update());
        }
        assert!(!fx.world.contains(target));
        assert!(fx.position(chaser).distance(Vec3::new(4.0, 0.0, 0.0)) < 0.2);
        assert_eq!(commands.len(), 2);
        assert_eq!(
            commands[0],
            Command::PlaySound {
                path: "sounds/pickup.wav".to_owned(),
                volume: 0.5
            }
        );
        assert!(
            matches!(&commands[1], Command::SpawnScene { path, .. } if path == "scenes/effect.ron")
        );

        fx.input.press_key(KeyCode::Escape);
        assert_eq!(fx.update(), [Command::Exit]);
    }

    #[test]
    fn collisions_and_physics_reach_scripts() {
        let mut fx = Fixture::new(
            "physics",
            r#"
            fn init() { this.hits = 0; }
            fn on_collision(other) {
                this.hits += 1;
                if other.name == "Floor" { this.entity.velocity = vec3(0, 5, 0); }
            }
            fn fixed_update(dt) {
                let hit = raycast(this.entity.position, vec3(0, -1, 0), 100);
                this.entity.scale = if hit == () { vec3(1, 1, 1) } else { vec3(hit.distance, 1, 1) };
            }
            "#,
        );
        fx.world.spawn((
            Transform::from_xyz(0.0, -0.5, 0.0),
            Collider::cuboid(Vec3::new(20.0, 1.0, 20.0)),
            Name("Floor".to_owned()),
        ));
        let ball = fx.world.spawn((
            Transform::from_xyz(0.0, 1.0, 0.0),
            Collider::sphere(0.25),
            RigidBody::dynamic(),
            Script::new("test.rhai"),
        ));

        let mut bounced = false;
        for _ in 0..90 {
            fx.physics.step(&mut fx.world, DT);
            let collisions = fx.physics.collisions().to_vec();
            fx.physics.clear_collisions();
            fx.fixed_update(&collisions);
            bounced |= fx.physics.linear_velocity(ball).is_some_and(|v| v.y > 4.0);
        }
        assert!(
            bounced,
            "on_collision should have launched the ball upwards"
        );
        // The raycast from inside the ball hits the ball itself first, at
        // distance 0, so the script saw a hit.
        assert!(fx.world.get::<&Transform>(ball).unwrap().scale.x < 1.0);
    }

    #[test]
    fn a_failing_script_stops_without_taking_the_game_down() {
        let mut fx = Fixture::new(
            "errors",
            r#"
            fn update(dt) {
                this.entity.position = vec3(1, 0, 0);
                let missing = find("Nobody");
                missing.position = vec3(9, 9, 9);   // error: () has no position
                this.entity.position = vec3(2, 0, 0);
            }
            "#,
        );
        let entity = fx.spawn_scripted(Vec3::ZERO);
        fx.update();
        assert_eq!(fx.position(entity).x, 1.0);
        // It is not retried every frame.
        fx.world
            .get::<&mut Transform>(entity)
            .unwrap()
            .translation
            .x = 7.0;
        fx.update();
        assert_eq!(fx.position(entity).x, 7.0);
    }

    #[test]
    fn endless_loops_are_cut_short() {
        let mut fx = Fixture::new("loop", "fn update(dt) { loop { } }");
        let entity = fx.spawn_scripted(Vec3::ZERO);
        fx.update(); // Returns instead of hanging.
        assert!(fx.world.contains(entity));
    }

    #[test]
    fn broken_and_missing_files_are_reported_not_fatal() {
        let mut fx = Fixture::new("syntax", "fn update(dt) { this is not rhai");
        fx.spawn_scripted(Vec3::ZERO);
        fx.world
            .spawn((Transform::IDENTITY, Script::new("no_such_file.rhai")));
        assert!(fx.update().is_empty());
    }

    #[test]
    fn changed_files_are_reloaded_keeping_state() {
        let mut fx = Fixture::new(
            "reload",
            r#"
            fn init() { this.step = 1.0; }
            fn update(dt) { this.entity.position += vec3(this.step, 0, 0); }
            "#,
        );
        let entity = fx.spawn_scripted(Vec3::ZERO);
        fx.update();
        assert_eq!(fx.position(entity).x, 1.0);

        // The new version moves along Y instead, using the kept `this.step`.
        std::fs::write(
            fx.dir.join("test.rhai"),
            "fn update(dt) { this.entity.position += vec3(0, this.step, 0); }",
        )
        .unwrap();
        // Make sure the modification time differs even on coarse clocks.
        let later = SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(fx.dir.join("test.rhai"))
            .unwrap()
            .set_modified(later)
            .unwrap();
        for _ in 0..40 {
            fx.update(); // Past the reload interval.
        }
        let position = fx.position(entity);
        assert!(position.y > 0.0, "{position}");
        assert!(
            position.x < 40.0,
            "the old version must stop running: {position}"
        );
    }

    #[test]
    fn camera_changes_made_by_scripts_stick() {
        let mut fx = Fixture::new(
            "camera",
            "fn update(dt) { set_camera(vec3(0, 10, 0) + camera_position() * 0, vec3(0, 0, 0)); }",
        );
        fx.spawn_scripted(Vec3::ZERO);
        fx.update();
        assert_eq!(fx.camera.position, Vec3::new(0.0, 10.0, 0.0));
        assert!((fx.camera.forward() - Vec3::NEG_Y).length() < 1e-4);
    }
}
