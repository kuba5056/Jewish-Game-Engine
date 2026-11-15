# Engine guide

How to build a game on this engine. Written for someone, human or AI, who
has the repository and nothing else. Read the first three sections, skim the
rest, and keep "Pitfalls" open while you work.

Working code beats prose: every feature below has a runnable example in
`crates/engine/examples/`, and `template/` is a complete starter project.

## Contents

1. [Starting a game](#1-starting-a-game)
2. [The shape of a game](#2-the-shape-of-a-game)
3. [What happens in a frame](#3-what-happens-in-a-frame)
4. [Entities and components](#4-entities-and-components)
5. [Showing things](#5-showing-things)
6. [Assets](#6-assets)
7. [Input](#7-input)
8. [Physics](#8-physics)
9. [Audio](#9-audio)
10. [User interface](#10-user-interface)
11. [Animation](#11-animation)
12. [Particles](#12-particles)
13. [Scene files](#13-scene-files)
14. [Scripts](#14-scripts)
15. [Tools: editor and debug overlay](#15-tools-editor-and-debug-overlay)
16. [Pitfalls](#16-pitfalls)
17. [Checking your work](#17-checking-your-work)
18. [Library versions](#18-library-versions)

## 1. Starting a game

Copy `template/` out of the repository, rename the package in its
`Cargo.toml`, and run it:

```
cp -r template ../my_game
cd ../my_game
cargo run
```

That directory is a normal Cargo project with one dependency:

```toml
[dependencies]
engine = { git = "https://github.com/kuba5056/Jewish-Game-Engine" }
```

Everything a game needs is re-exported from the `engine` crate; do not
depend on `engine_render`, `engine_physics` and the other internal crates.

A game keeps its files in an `assets/` directory next to `Cargo.toml`
(scenes, scripts, models, textures, sounds). All asset paths in code, scenes
and scripts are relative to it. The engine looks for `assets/` in the
working directory first, then next to the executable, so `cargo run` from
the project root and a shipped build with `assets/` beside the binary both
work. Change the directory with `Config::asset_dir`.

Requirements: Rust (the template pins the tested version in
`rust-toolchain.toml`; rustup installs it by itself). Linux also needs the
ALSA headers to build (`libasound2-dev` on Debian/Ubuntu) and a Vulkan
driver to run. On NixOS run `nix-shell` first; the template has a
`shell.nix`. Windows needs nothing extra.

## 2. The shape of a game

A game is a type that implements `Game`, handed to `engine::run`:

```rust
use engine::math::Vec3;
use engine::{Config, Context, Game, KeyCode};

struct MyGame;

impl Game for MyGame {
    fn init(&mut self, ctx: &mut Context) {
        ctx.load_scene("scenes/main.ron").expect("scene");
    }

    fn fixed_update(&mut self, ctx: &mut Context) {
        // 60 times a second, after each physics step: movement, game rules.
    }

    fn update(&mut self, ctx: &mut Context) {
        // Once per frame: one-off key presses, camera, interface.
        if ctx.input.key_pressed(KeyCode::Escape) {
            ctx.exit();
        }
    }
}

fn main() -> Result<(), engine::Error> {
    engine::run(Config::default(), MyGame)
}
```

All `Game` methods are optional: `init`, `fixed_update`, `update`,
`resized(ctx, width, height)` and `shutdown`.

`Context` is the whole engine, passed to every callback:

| Field | What it is |
|---|---|
| `ctx.world` | all entities and components (a `hecs::World`) |
| `ctx.time` | `delta_secs()`, `fixed_delta_secs()`, `elapsed()` |
| `ctx.input` | keyboard and mouse |
| `ctx.physics` | the physics simulation |
| `ctx.audio` | sound playback |
| `ctx.renderer` | draw calls and render settings |
| `ctx.camera` | the camera the frame is drawn from |
| `ctx.scripts_paused` | set `true` to stop entity scripts |

Methods: `load_scene`, `save_scene`, `load_model`, `load_texture`,
`load_sound`, `ui()`, `add_system`, `add_fixed_system`, `window_size()`,
`set_cursor_captured(bool)`, `exit()`.

`Config` fields: `window` (`title`, `width`, `height`, `resizable`),
`fixed_hz` (default 60), `asset_dir` (default `"assets"`), `hot_reload`
(default: on in debug builds), `debug_overlay_key` (default F3),
`show_debug_overlay`.

Math types come from `engine::math` (the `glam` crate): `Vec3`, `Vec4`,
`Quat`, `Mat4`. The world is right-handed with Y up; "forward" for cameras,
lights and `Transform::looking_to` is the local -Z axis. Units are metres
and seconds by convention (gravity is 9.81).

## 3. What happens in a frame

In this order:

1. Changed asset and script files are reloaded (debug builds).
2. Zero or more fixed steps, to keep up with real time. Each one:
   physics step → `Game::fixed_update` → fixed systems → scripts'
   `on_collision` and `fixed_update`.
3. `Game::update` → systems → scripts' `init` and `update`.
4. Collision events are cleared.
5. `Visual`s become meshes, animations advance, world transforms are
   computed, every visible entity and light is queued, particles update.
6. The frame is drawn: shadows, the lit scene, tone mapping, interface.

Consequences worth remembering:

- A frame may contain no fixed step or several. Read one-off presses
  (`key_pressed`) in `update` and remember them for `fixed_update`; held
  keys (`key_down`) are fine anywhere.
- `ctx.physics.collisions()` holds every contact that began or ended this
  frame and is readable in both `fixed_update` and `update`.
- An entity spawned in `update` is drawn in the same frame. Its physics
  body is created on the next physics step.
- `GlobalTransform` (world-space matrix) is computed in step 5, so in
  `update` it shows last frame's positions. Call
  `engine::propagate_transforms(&mut ctx.world)` if you need it sooner.

## 4. Entities and components

The engine uses the `hecs` ECS, re-exported as `engine::ecs`. An entity is
an id; components are plain Rust values attached to it. Any `'static +
Send + Sync` type can be a component.

```rust
let cube = ctx.world.spawn((Transform::from_xyz(0.0, 1.0, 0.0), Health(100)));

// One entity's component:
if let Ok(mut health) = ctx.world.get::<&mut Health>(cube) {
    health.0 -= 10;
}

// All entities with given components:
for (transform, health) in ctx.world.query_mut::<(&mut Transform, &Health)>() { /* ... */ }

// With the entity id, and with filters:
for (entity, transform) in ctx.world.query::<(Entity, &Transform)>().with::<&Enemy>().iter() { /* ... */ }

ctx.world.insert_one(cube, Burning);          // add or replace a component
ctx.world.remove_one::<Burning>(cube);        // remove one
engine::despawn_recursive(&mut ctx.world, cube); // remove it and its children
```

You cannot spawn or despawn while iterating a query: collect the entities
into a `Vec` first, then change the world.

Components the engine understands:

| Component | Effect |
|---|---|
| `Transform` | position, rotation, scale; required for almost everything |
| `Parent(entity)` | makes the `Transform` relative to another entity |
| `Name(String)` | a label; `engine::find_by_name(&world, "Player")` |
| `Visual` | what the entity looks like, as data (cube, sphere, plane, model) |
| `MeshRenderer` | a mesh and material to draw (created from `Visual`, or by hand) |
| `DirectionalLight` | the sun; shines along the entity's forward direction |
| `PointLight` | a lamp at the entity's position |
| `Collider` | a solid shape; without `RigidBody` it is a static obstacle |
| `RigidBody` | makes a collider move under physics |
| `ParticleEmitter` | emits particles from the entity |
| `AnimationPlayer` | plays a model's animations (added by `Model::spawn`) |
| `Script` | runs a `.rhai` script for the entity |

Read-only components the engine maintains: `GlobalTransform`, `Skin`,
`ModelPart`.

`Transform` helpers: `Transform::from_xyz(x, y, z)`, `from_translation(v)`,
`.with_rotation(quat)`, `.with_scale(v)`, `.looking_to(direction)`,
`.looking_at(point)`. `Transform::IDENTITY` is the origin.

Reusable logic can be registered as a system, a plain function run every
frame or every fixed step: `ctx.add_system(my_system)` where
`fn my_system(ctx: &mut Context)`.

## 5. Showing things

### Visuals

The simplest way to show something is a `Visual`, which is plain data (so
it also works in scene files):

```rust
ctx.world.spawn((
    Transform::from_xyz(0.0, 0.5, 0.0),
    Visual::Cube { size: 1.0, material: MaterialDesc::colored(0.8, 0.2, 0.2) },
));
```

Variants: `Cube { size, material }`, `Sphere { radius, material }`,
`Plane { size, uv_repeat, material }` (a flat square facing up) and
`Model { path }` (a glTF file). The engine turns a `Visual` into meshes on
the next frame, redoes that when the `Visual` changes, and removes them
when it is removed.

`MaterialDesc` fields: `texture` (`Option<String>`, a path), `base_color`
(`[r, g, b, a]`, linear), `metallic` (0 non-metal to 1 metal), `roughness`
(0 mirror to 1 matte), `emissive` (`[r, g, b]`; values above 1 glow).

### Meshes by hand

For your own geometry, build a `MeshData` (vertices and `u32` indices,
counter-clockwise triangles) or use `MeshData::cube(size)`, `sphere(radius,
segments)`, `plane(size, uv_repeat)`, then:

```rust
let mesh = ctx.renderer.create_mesh(&MeshData::cube(1.0));
ctx.world.spawn((Transform::IDENTITY, MeshRenderer { mesh, material: Material::colored(0.2, 0.6, 1.0) }));
```

`Material` has the same fields as `MaterialDesc` but holds a loaded
`Texture` (`Option<Texture>`) and `glam` vectors. Meshes and textures are
cheap to clone; clones share GPU data. For immediate drawing without an
entity, call `ctx.renderer.draw(&mesh, &material, matrix)` every frame.

### Lights

A scene with no light shows only dim ambient light. Give it a sun:

```rust
ctx.world.spawn((
    Transform::IDENTITY.looking_to(Vec3::new(-0.5, -1.0, -0.4)),
    DirectionalLight::default(), // color, intensity (3 = bright daylight)
));
```

Only one `DirectionalLight` is used; it casts shadows within
`ctx.renderer.shadow_distance` (default 25) of the camera. `PointLight {
color, intensity, range }` shines from the entity's position, casts no
shadow, and at most 16 are used per frame.

Render settings on `ctx.renderer`: `clear_color` (background),
`ambient_sky`, `ambient_ground`, `exposure`, `shadows`, `shadow_distance`.

### Camera

`ctx.camera` has `position`, `rotation`, `fov_y_degrees`, `near`, `far`.
Aim it with `look_at(point)` or `set_yaw_pitch(yaw, pitch)` (radians).
`forward()`, `right()`, `up()` give its axes.
`screen_ray((x, y), width, height)` returns the ray through a pixel, and
`engine::pick(&ctx.world, origin, direction)` the visible entity it hits.

## 6. Assets

| Call | Loads |
|---|---|
| `ctx.load_scene("scenes/x.ron")` | a scene file; spawns and returns its entities |
| `ctx.load_model("models/x.gltf")` | a glTF 2.0 model (`.gltf` or `.glb`) |
| `ctx.load_texture("textures/x.png")` | a PNG or JPEG |
| `ctx.load_sound("sounds/x.wav")` | WAV, OGG Vorbis, MP3 or FLAC |

All return `Result<_, AssetError>`; print the error, it names the file.
Loading the same path twice returns the already-loaded asset. In debug
builds, changed texture, model and script files are reloaded while the
game runs.

A loaded `Model` is placed in the world with
`model.spawn(&mut ctx.world, transform)`, which returns a root entity with
the model's nodes as child entities. From glTF the engine reads the node
tree, triangle meshes, base colour and its texture, metallic, roughness and
emissive factors, skins and animations. It ignores normal maps and other
material textures, morph targets, cameras and lights.

## 7. Input

```rust
ctx.input.key_down(KeyCode::KeyW)        // held
ctx.input.key_pressed(KeyCode::Space)    // went down this frame
ctx.input.key_released(KeyCode::Space)
ctx.input.button_down(MouseButton::Left) // also _pressed, _released
ctx.input.cursor()                       // Option<(x, y)> in window pixels
ctx.input.mouse_delta()                  // raw motion this frame, for mouse-look
ctx.input.scroll()                       // wheel, in lines
```

Key codes are physical positions (`KeyCode::KeyW` is the W position on any
layout): `KeyA`..`KeyZ`, `Digit0`..`Digit9`, `ArrowLeft`, `Space`, `Enter`,
`Escape`, `ShiftLeft`, `ControlLeft`, `F1`..`F12`. For mouse-look call
`ctx.set_cursor_captured(true)` and read `mouse_delta()`.

Presses the interface used (a click on a button, typing in a text field)
are not reported by `ctx.input`.

## 8. Physics

Give an entity a `Transform` and a `Collider` and it is a static obstacle;
add a `RigidBody` and it moves.

```rust
ctx.world.spawn((
    Transform::from_xyz(0.0, 5.0, 0.0),
    Visual::Sphere { radius: 0.5, material: MaterialDesc::default() },
    Collider::sphere(0.5).with_restitution(0.5),
    RigidBody::dynamic(),
));
```

- Shapes: `Collider::cuboid(size)`, `sphere(radius)`, `capsule(half_height,
  radius)`; `Collider::default()` is a 1×1×1 box. Builders:
  `with_friction`, `with_restitution`, `with_density`, `as_sensor()`.
- Bodies: `RigidBody::dynamic()` (moved by forces), `RigidBody::kinematic()`
  (moved only by writing its `Transform`; pushes dynamic bodies). Fields:
  `linear_velocity` (initial), `linear_damping`, `angular_damping`,
  `gravity_scale`, `lock_rotation`, `continuous_collision`.
- The `Transform`'s scale is applied to the collider shape, so a unit cube
  visual with a unit box collider can be stretched into a floor by scale
  alone.
- The simulation writes each body's position and rotation into its
  `Transform` after every step. Writing the `Transform` yourself teleports
  the body (and is how kinematic bodies are moved).

Queries and control, all by entity:

```rust
ctx.physics.linear_velocity(e)             // Option<Vec3>; None until the body exists
ctx.physics.set_linear_velocity(e, v)
ctx.physics.apply_impulse(e, impulse)
ctx.physics.raycast(origin, direction, max_distance, Some(ignore_entity)) // Option<RayHit>
ctx.physics.collisions()                   // &[Collision { a, b, started }]
ctx.physics.set_gravity(v);  ctx.physics.paused = true;
```

`RayHit` has `entity`, `distance`, `point`, `normal`. A sensor collider
reports contacts without blocking: use it for pickups and trigger zones,
and match contacts with `collision.other(my_entity)`.

## 9. Audio

```rust
let sound = ctx.load_sound("sounds/jump.wav")?;
ctx.audio.play(&sound);
let mut music = ctx.audio.play_with(&sound, Playback { volume: 0.5, looping: true, ..Playback::default() });
music.stop();
```

`Playback` fields: `volume` (1 = unchanged), `pitch` (2 = an octave up),
`pan` (-1 left to 1 right), `looping`. The handle has `stop`, `pause`,
`resume`, `set_volume`, `is_playing`. With no audio device the game runs
silently. Sound is not positional.

## 10. User interface

The engine embeds `egui` (re-exported as `engine::egui`). Build the
interface every frame during `update`, not in `init`:

```rust
engine::egui::Window::new("Menu").show(&ctx.ui(), |ui| {
    ui.label("Score: 3");
    if ui.button("Quit").clicked() {
        ctx.exit();
    }
});
```

`egui::Area` with `.anchor(...)` suits HUD text; `egui::Window` with
`.title_bar(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0])` suits menus.
See `examples/sparkball.rs` for a menu, HUD, pause and end screens.

## 11. Animation

`Model::spawn` adds an `AnimationPlayer` to the root entity of any model
that has animations:

```rust
let robot = model.spawn(&mut ctx.world, Transform::IDENTITY);
if let Ok(mut player) = ctx.world.get::<&mut AnimationPlayer>(robot) {
    player.play("Walk");      // false if there is no such clip
    player.looping = true;    // also: speed, paused
}
```

Also `clip_names()`, `current()`, `time()`, `stop()`, `is_finished()`.
Skinned (skeletal) meshes and plain node animation both work. Switching
clips is instant; there is no blending. Each spawned instance animates
independently.

## 12. Particles

```rust
ctx.world.spawn((Transform::from_xyz(0.0, 0.2, 0.0), ParticleEmitter::fire()));
```

Presets: `fire()`, `smoke()`, `burst(count)` (a one-off spray that despawns
its entity when finished). Or set fields on `ParticleEmitter::default()`:
`rate` (per second), `burst`, `emitting`, `lifetime` and `speed` (ranges),
`direction` and `spread` (cone half-angle in degrees), `radius`, `gravity`,
`drag`, `size` (start, end), `color_start`, `color_end` (RGBA; fade alpha
to 0), `additive` (glow), `max_particles`, `despawn_when_done`.

Particles live in world space, so a moving emitter leaves a trail. They are
unlit, do not collide and cast no shadow.

## 13. Scene files

A scene is a RON file listing entities. Load it with `ctx.load_scene`;
write the current world with `ctx.save_scene` or the editor. Every field
except the ones you need can be left out:

```ron
(
    entities: [
        (
            name: "Sun",
            transform: (rotation: (-0.432607, 0.380028, 0.207921, 0.790695)),
            directional_light: (color: (1.0, 0.96, 0.9), intensity: 3.0),
        ),
        (
            name: "Floor",
            transform: (translation: (0.0, -0.5, 0.0), scale: (20.0, 1.0, 20.0)),
            visual: Cube(size: 1.0, material: (base_color: (0.4, 0.45, 0.4, 1.0), roughness: 0.9)),
            collider: (friction: 0.9),
        ),
        (
            name: "Ball",
            transform: (translation: (0.0, 3.0, 0.0)),
            visual: Sphere(radius: 0.5, material: (texture: "textures/ball.png", metallic: 0.0)),
            collider: (shape: Sphere(radius: 0.5), restitution: 0.6),
            rigid_body: (kind: Dynamic, angular_damping: 0.5),
            script: "scripts/coin.rhai",
        ),
        (
            name: "Torch",
            parent: 2,   // index into this list: follows the ball
            transform: (translation: (0.0, 1.0, 0.0)),
            point_light: (color: (1.0, 0.6, 0.2), intensity: 3.0, range: 6.0),
            particles: (rate: 40.0, additive: true, color_start: (4.0, 1.6, 0.3, 0.9), color_end: (1.5, 0.1, 0.0, 0.0)),
        ),
        (name: "Robot", visual: Model(path: "models/robot.gltf")),
    ],
)
```

Field reference:

- `transform`: `translation (x, y, z)`, `rotation (x, y, z, w)` (a
  quaternion), `scale (x, y, z)`. A turn of θ about the vertical axis is
  `(0, sin(θ/2), 0, cos(θ/2))`.
- `visual`: `Cube(size, material)`, `Sphere(radius, material)`,
  `Plane(size, uv_repeat, material)`, `Model(path)`.
- `material`: `texture`, `base_color (r, g, b, a)`, `metallic`,
  `roughness`, `emissive (r, g, b)`.
- `collider`: `shape` (`Box(half_extents: (x, y, z))`, `Sphere(radius)`,
  `Capsule(half_height, radius)`), `friction`, `restitution`, `density`,
  `sensor`. `collider: ()` is a unit box.
- `rigid_body`: `kind` (`Dynamic`, `Kinematic`, `Fixed`),
  `linear_velocity`, `linear_damping`, `angular_damping`, `gravity_scale`,
  `lock_rotation`, `continuous_collision`. `rigid_body: ()` is dynamic.
- `directional_light`, `point_light`, `particles`: the component's fields.
- `script`: a script path. `parent`: an index into `entities`.

Game code usually finds scene entities by name and attaches the rest, as
`examples/sparkball.rs` does with its `"Cell"` and `"Start"` markers.

## 14. Scripts

An entity with a `Script` (in code: `Script::new("scripts/x.rhai")`; in a
scene: `script: "scripts/x.rhai"`) runs a [Rhai](https://rhai.rs) file.
The file defines any of these functions:

```rust
fn init() { this.speed = 3.0; }     // once, before the first update
fn update(dt) { }                   // every frame
fn fixed_update(dt) { }             // every physics step
fn on_collision(other) { }          // when it starts touching `other`
```

`this` is a map that belongs to that one entity and persists between
calls; `this.entity` is the entity. Code outside functions does not run.

Available to scripts:

| | |
|---|---|
| Vectors | `vec3(x, y, z)`; `.x .y .z`; `+ - * /`; `.length()`, `.normalized()`, `.dot(v)`, `.cross(v)`, `.distance(v)`, `.lerp(v, t)` |
| Entity properties | `.position`, `.rotation` (pitch, yaw, roll in degrees), `.scale`, `.velocity` (read and write); `.name`, `.alive` (read) |
| Entity methods | `.impulse(v)`, `.despawn()` |
| World | `find(name)` (entity or `()`), `find_all(name)`, `raycast(origin, direction, max)` (map with `entity`, `distance`, `point`, `normal`, or `()`), `spawn_scene(path)`, `spawn_scene(path, position)` |
| Input, time | `key_down(name)`, `key_pressed(name)`, `key_released(name)`, `mouse_down(button)`, `elapsed()` |
| Other | `play_sound(path)`, `play_sound(path, volume)`, `set_camera(position, target)`, `camera_position()`, `exit()`, `print(x)` |

Key names: `"W"`, `"Space"`, `"Left"`, `"1"`, `"Shift"`, `"F5"`, `"Escape"`.

To change one component of a vector property, copy it, change it, write it
back: `let p = this.entity.position; p.y += 1.0; this.entity.position = p;`.

A script that raises an error is logged and stops until its file changes.
Scripts cannot create entities except by `spawn_scene`, cannot read another
entity's `this`, and have no interface API. `raycast` from a script does
not skip the script's own entity: start the ray outside its collider.
See `assets/scripts/` and `examples/scripted.rs`.

## 15. Tools: editor and debug overlay

The editor opens a scene for editing: click to select, drag the arrows to
move, edit components in the inspector, Play to test with physics and
scripts, Ctrl+S to save. It lives in the engine repository and works on the
`assets/` of the directory it is started from, so to edit your own game's
scenes run it from the game's directory:

```
cargo run --manifest-path ../Jewish-Game-Engine/Cargo.toml -p editor -- scenes/main.ron
```

(From inside the engine repository, `cargo run -p editor -- scenes/demo.ron`
edits the engine's example scenes.)

F3 in any game opens the debug overlay: frame rate, entity and draw counts,
an entity list and an inspector that edits transforms, materials and
lights live.

## 16. Pitfalls

- **Nothing visible / everything dark.** No `DirectionalLight`, or the
  camera looks the wrong way. Set `ctx.camera.position` and `look_at`.
- **Entity does not appear.** It needs a `Transform` as well as a
  `Visual` or `MeshRenderer`.
- **Asset not found.** Paths are relative to `assets/`, and the game must
  be run from the directory that contains `assets/`.
- **Body ignores `set_linear_velocity` or returns `None`.** The body is
  created on the first physics step after spawning; check for `None`.
- **Jump or shot sometimes ignored.** `key_pressed` was read in
  `fixed_update`. Read it in `update` and store it.
- **Physics object with a `Parent`.** Not supported: bodies live in world
  space. Parent a visual child to the body instead.
- **Changing `Collider` or `RigidBody` after spawn has no effect.** They
  are read once, when the body is created. Despawn and respawn.
- **Borrow errors with `ctx.world`.** A query borrows the world; collect
  entity ids into a `Vec`, end the query, then spawn, despawn or insert.
- **Mutating through `world.get`.** Use `get::<&mut T>` and keep the guard
  in a short scope; two live guards on the same component panic.
- **Interface built in `init`.** It must be built each frame in `update`.
- **Scene rotation looks wrong.** Rotations are quaternions `(x, y, z,
  w)`, not Euler angles.
- **Saved scene misses objects.** Only entities with a `Transform` are
  saved, and what they look like only if they have a `Visual` (a
  hand-made `MeshRenderer` is not saved).
- **Colours washed out or too bright.** Colours are linear and the image
  is tone-mapped; lower light `intensity` or `ctx.renderer.exposure`.
- **Shadows missing far away.** They cover `shadow_distance` around the
  camera's focus; raise it (shadows get softer).

## 17. Checking your work

```
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run
```

Game rules that do not need a window can be unit-tested against a bare
`World` and `Physics` (`Physics::new()`, then `physics.step(&mut world,
dt)`); the engine's own tests in `crates/engine_physics` show how.

To run a game without a display (CI, containers, an AI agent's sandbox),
use a virtual X server and a software Vulkan driver. On Debian/Ubuntu:

```
sudo apt-get install -y xvfb mesa-vulkan-drivers libxkbcommon-x11-0 imagemagick xdotool
xvfb-run -a -s "-screen 0 1280x720x24" bash -c '
  ./target/debug/my_game & sleep 5
  xdotool search --name "My Game" windowfocus   # needed before sending keys
  xdotool keydown d; sleep 1; xdotool keyup d
  import -window root shot.png                   # screenshot
  wait'
```

Give the game a way to exit by itself in such runs (the examples read an
environment variable for a time limit). Software rendering is slow, around
25 frames a second, which is not representative of real hardware.

## 18. Library versions

The engine is built on recent versions whose APIs differ from most
tutorials and from what a language model is likely to remember. If you
write code against these libraries directly, read their source or docs for
the exact version in `Cargo.lock` rather than relying on memory:

- **glam 0.34**: `Mat4::perspective_rh` and `look_at_rh` are gone; the
  functions live in `glam::camera::rh`. Types such as `Vec4` cannot have
  two components mutably borrowed at once; go through `to_array()`.
- **egui 0.37**: `SidePanel` and `TopBottomPanel` became `Panel`, which
  needs a parent `Ui`. Top-level `Window` and `Area` work with
  `ctx.ui()`. `Id::new` is deprecated in favour of `Id::unique`.
- **hecs 0.11**: queries yield components only; ask for `Entity` in the
  query tuple to get the id.
- **wgpu 30**, **winit 0.30**, **rapier3d 0.36**, **kira 0.12**, **rhai
  1.27**: wrapped by the engine; games should not need them directly.
