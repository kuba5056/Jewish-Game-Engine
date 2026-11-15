//! The functions and types scripts can use.
//!
//! **Vectors** — `vec3(x, y, z)`; fields `.x .y .z`; `+ - * /` with vectors
//! and numbers; `.length()`, `.normalized()`, `.dot(v)`, `.cross(v)`,
//! `.distance(v)`, `.lerp(v, t)`.
//!
//! **Entities** (`this.entity`, or from `find`) — properties `.position`,
//! `.rotation` (pitch, yaw, roll in degrees), `.scale`, `.velocity`
//! (physics bodies), all readable and writable; read-only `.name` and
//! `.alive`; methods `.impulse(v)` and `.despawn()`.
//!
//! **World** — `find(name)` (an entity or `()`), `find_all(name)`,
//! `raycast(origin, direction, max_distance)` (a map with `entity`,
//! `distance`, `point`, `normal`, or `()`), `spawn_scene(path)` and
//! `spawn_scene(path, position)`.
//!
//! **Input and time** — `key_down(name)`, `key_pressed(name)`,
//! `key_released(name)`, `mouse_down(button)` (0 left, 1 right, 2 middle),
//! `elapsed()`.
//!
//! **Other** — `play_sound(path)`, `play_sound(path, volume)`,
//! `camera_position()`, `set_camera(position, target)`, `exit()`, and
//! `print(...)`, which writes to the log.

use std::cell::RefCell;
use std::rc::Rc;

use engine_platform::MouseButton;
use engine_scene::{despawn_recursive, find_by_name, Entity, Name, Transform};
use glam::{EulerRot, Quat, Vec3};
use rhai::{Array, Dynamic, Engine, EvalAltResult, Map, FLOAT, INT};

use crate::keys::key_from_name;
use crate::{Command, Host};

type ScriptResult<T> = Result<T, Box<EvalAltResult>>;

/// An entity as scripts see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityRef(pub Entity);

/// Accepts whole numbers where a decimal is expected, so `vec3(1, 0, 0)`
/// works as well as `vec3(1.0, 0.0, 0.0)`.
fn number(value: &Dynamic) -> ScriptResult<f32> {
    value
        .as_float()
        .or_else(|_| value.as_int().map(|i| i as f32))
        .map_err(|kind| format!("expected a number, got {kind}").into())
}

pub(crate) fn register(engine: &mut Engine, host: &Rc<RefCell<Host>>) {
    register_vec3(engine);
    register_entity(engine, host);
    register_globals(engine, host);
}

fn register_vec3(engine: &mut Engine) {
    engine
        .register_type_with_name::<Vec3>("Vec3")
        .register_fn("vec3", |x: Dynamic, y: Dynamic, z: Dynamic| {
            Ok::<_, Box<EvalAltResult>>(Vec3::new(number(&x)?, number(&y)?, number(&z)?))
        })
        .register_get_set("x", |v: &mut Vec3| v.x, |v: &mut Vec3, x: FLOAT| v.x = x)
        .register_get_set("y", |v: &mut Vec3| v.y, |v: &mut Vec3, y: FLOAT| v.y = y)
        .register_get_set("z", |v: &mut Vec3| v.z, |v: &mut Vec3, z: FLOAT| v.z = z)
        .register_fn("+", |a: Vec3, b: Vec3| a + b)
        .register_fn("-", |a: Vec3, b: Vec3| a - b)
        .register_fn("-", |a: Vec3| -a)
        .register_fn("*", |a: Vec3, b: Vec3| a * b)
        .register_fn("*", |a: Vec3, b: FLOAT| a * b)
        .register_fn("*", |a: FLOAT, b: Vec3| a * b)
        .register_fn("*", |a: Vec3, b: INT| a * b as f32)
        .register_fn("*", |a: INT, b: Vec3| a as f32 * b)
        .register_fn("/", |a: Vec3, b: FLOAT| a / b)
        .register_fn("/", |a: Vec3, b: INT| a / b as f32)
        .register_fn("==", |a: Vec3, b: Vec3| a == b)
        .register_fn("!=", |a: Vec3, b: Vec3| a != b)
        .register_fn("length", |v: &mut Vec3| v.length())
        .register_fn("normalized", |v: &mut Vec3| v.normalize_or_zero())
        .register_fn("dot", |a: &mut Vec3, b: Vec3| a.dot(b))
        .register_fn("cross", |a: &mut Vec3, b: Vec3| a.cross(b))
        .register_fn("distance", |a: &mut Vec3, b: Vec3| a.distance(b))
        .register_fn("lerp", |a: &mut Vec3, b: Vec3, t: FLOAT| a.lerp(b, t))
        .register_fn("to_string", |v: &mut Vec3| {
            format!("({}, {}, {})", v.x, v.y, v.z)
        })
        .register_fn("to_debug", |v: &mut Vec3| {
            format!("vec3({}, {}, {})", v.x, v.y, v.z)
        });
}

fn register_entity(engine: &mut Engine, host: &Rc<RefCell<Host>>) {
    engine
        .register_type_with_name::<EntityRef>("Entity")
        .register_fn("==", |a: EntityRef, b: EntityRef| a == b)
        .register_fn("!=", |a: EntityRef, b: EntityRef| a != b)
        .register_fn("to_string", |e: &mut EntityRef| {
            format!("entity {}", e.0.id())
        });

    // Reads one value out of the entity's Transform, or fails with a message
    // the script author can act on.
    fn read<T>(host: &Host, entity: EntityRef, get: impl Fn(&Transform) -> T) -> ScriptResult<T> {
        match host.world.get::<&Transform>(entity.0) {
            Ok(transform) => Ok(get(&transform)),
            Err(_) => Err("the entity no longer exists or has no Transform".into()),
        }
    }
    fn write(host: &Host, entity: EntityRef, set: impl Fn(&mut Transform)) -> ScriptResult<()> {
        match host.world.get::<&mut Transform>(entity.0) {
            Ok(mut transform) => {
                set(&mut transform);
                Ok(())
            }
            Err(_) => Err("the entity no longer exists or has no Transform".into()),
        }
    }

    let (get, set) = (host.clone(), host.clone());
    engine.register_get_set(
        "position",
        move |e: &mut EntityRef| read(&get.borrow(), *e, |t| t.translation),
        move |e: &mut EntityRef, v: Vec3| write(&set.borrow(), *e, |t| t.translation = v),
    );
    let (get, set) = (host.clone(), host.clone());
    engine.register_get_set(
        "scale",
        move |e: &mut EntityRef| read(&get.borrow(), *e, |t| t.scale),
        move |e: &mut EntityRef, v: Vec3| write(&set.borrow(), *e, |t| t.scale = v),
    );
    let (get, set) = (host.clone(), host.clone());
    engine.register_get_set(
        "rotation",
        move |e: &mut EntityRef| {
            read(&get.borrow(), *e, |t| {
                let (yaw, pitch, roll) = t.rotation.to_euler(EulerRot::YXZ);
                Vec3::new(pitch.to_degrees(), yaw.to_degrees(), roll.to_degrees())
            })
        },
        move |e: &mut EntityRef, degrees: Vec3| {
            write(&set.borrow(), *e, |t| {
                t.rotation = Quat::from_euler(
                    EulerRot::YXZ,
                    degrees.y.to_radians(),
                    degrees.x.to_radians(),
                    degrees.z.to_radians(),
                );
            })
        },
    );

    // Velocity only means something for physics bodies. Reading it elsewhere
    // gives zero and writing does nothing, so a script need not care whether
    // the body has been created yet (that happens on the first physics step).
    let (get, set) = (host.clone(), host.clone());
    engine.register_get_set(
        "velocity",
        move |e: &mut EntityRef| {
            get.borrow()
                .physics
                .linear_velocity(e.0)
                .unwrap_or(Vec3::ZERO)
        },
        move |e: &mut EntityRef, v: Vec3| {
            set.borrow_mut().physics.set_linear_velocity(e.0, v);
        },
    );
    let h = host.clone();
    engine.register_fn("impulse", move |e: &mut EntityRef, v: Vec3| {
        h.borrow_mut().physics.apply_impulse(e.0, v);
    });

    let h = host.clone();
    engine.register_get("name", move |e: &mut EntityRef| {
        match h.borrow().world.get::<&Name>(e.0) {
            Ok(name) => name.0.clone(),
            Err(_) => String::new(),
        }
    });
    let h = host.clone();
    engine.register_get("alive", move |e: &mut EntityRef| {
        h.borrow().world.contains(e.0)
    });
    let h = host.clone();
    engine.register_fn("despawn", move |e: &mut EntityRef| {
        despawn_recursive(&mut h.borrow_mut().world, e.0);
    });
}

fn register_globals(engine: &mut Engine, host: &Rc<RefCell<Host>>) {
    fn key(name: &str) -> ScriptResult<engine_platform::KeyCode> {
        key_from_name(name).ok_or_else(|| {
            format!("unknown key \"{name}\" (try \"W\", \"Space\", \"Left\")").into()
        })
    }
    let h = host.clone();
    engine.register_fn("key_down", move |name: &str| {
        Ok::<_, Box<EvalAltResult>>(h.borrow().input.key_down(key(name)?))
    });
    let h = host.clone();
    engine.register_fn("key_pressed", move |name: &str| {
        Ok::<_, Box<EvalAltResult>>(h.borrow().input.key_pressed(key(name)?))
    });
    let h = host.clone();
    engine.register_fn("key_released", move |name: &str| {
        Ok::<_, Box<EvalAltResult>>(h.borrow().input.key_released(key(name)?))
    });
    let h = host.clone();
    engine.register_fn("mouse_down", move |button: INT| {
        let button = match button {
            0 => MouseButton::Left,
            1 => MouseButton::Right,
            2 => MouseButton::Middle,
            _ => return false,
        };
        h.borrow().input.button_down(button)
    });
    let h = host.clone();
    engine.register_fn("elapsed", move || h.borrow().elapsed);

    let h = host.clone();
    engine.register_fn("find", move |name: &str| {
        match find_by_name(&h.borrow().world, name).first() {
            Some(&entity) => Dynamic::from(EntityRef(entity)),
            None => Dynamic::UNIT,
        }
    });
    let h = host.clone();
    engine.register_fn("find_all", move |name: &str| -> Array {
        find_by_name(&h.borrow().world, name)
            .into_iter()
            .map(|entity| Dynamic::from(EntityRef(entity)))
            .collect()
    });
    let h = host.clone();
    engine.register_fn(
        "raycast",
        move |origin: Vec3, direction: Vec3, max_distance: Dynamic| {
            let max_distance = number(&max_distance)?;
            let hit = h
                .borrow()
                .physics
                .raycast(origin, direction, max_distance, None);
            Ok::<_, Box<EvalAltResult>>(match hit {
                Some(hit) => {
                    let mut map = Map::new();
                    map.insert("entity".into(), Dynamic::from(EntityRef(hit.entity)));
                    map.insert("distance".into(), Dynamic::from_float(hit.distance));
                    map.insert("point".into(), Dynamic::from(hit.point));
                    map.insert("normal".into(), Dynamic::from(hit.normal));
                    Dynamic::from_map(map)
                }
                None => Dynamic::UNIT,
            })
        },
    );

    let command = |host: &Rc<RefCell<Host>>, command: Command| {
        host.borrow_mut().commands.push(command);
    };
    let h = host.clone();
    engine.register_fn("play_sound", move |path: &str| {
        command(
            &h,
            Command::PlaySound {
                path: path.to_owned(),
                volume: 1.0,
            },
        );
    });
    let h = host.clone();
    engine.register_fn("play_sound", move |path: &str, volume: Dynamic| {
        let volume = number(&volume)?;
        command(
            &h,
            Command::PlaySound {
                path: path.to_owned(),
                volume,
            },
        );
        Ok::<_, Box<EvalAltResult>>(())
    });
    let h = host.clone();
    engine.register_fn("spawn_scene", move |path: &str| {
        command(
            &h,
            Command::SpawnScene {
                path: path.to_owned(),
                offset: Vec3::ZERO,
            },
        );
    });
    let h = host.clone();
    engine.register_fn("spawn_scene", move |path: &str, offset: Vec3| {
        command(
            &h,
            Command::SpawnScene {
                path: path.to_owned(),
                offset,
            },
        );
    });
    let h = host.clone();
    engine.register_fn("exit", move || command(&h, Command::Exit));

    let h = host.clone();
    engine.register_fn("camera_position", move || h.borrow().camera.position);
    let h = host.clone();
    engine.register_fn("set_camera", move |position: Vec3, target: Vec3| {
        let camera = &mut h.borrow_mut().camera;
        camera.position = position;
        camera.look_at(target);
    });
}
