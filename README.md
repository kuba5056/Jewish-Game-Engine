# Jewish Game Engine

A general-purpose 3D game engine in Rust for Linux and Windows.

## Status

Early. Working so far:

- window, main loop with a fixed simulation step, keyboard and mouse input
- renderer: meshes, perspective camera, textures (PNG/JPEG or raw pixels,
  with mipmaps), vsync, 4x antialiasing
- lighting: physically based materials (base colour, metallic, roughness,
  emissive), a sun with soft shadows, up to 16 point lights, HDR rendering
  with filmic tone mapping
- entities and components (hecs ECS), parent/child transform hierarchy,
  systems; entities with a `MeshRenderer` are drawn automatically
- assets: glTF models and PNG/JPEG textures loaded from an `assets/`
  directory, shared when loaded twice, and reloaded on screen when the
  files change (debug builds)
- animation: skeletal (skinned meshes) and node animation from glTF, with
  an `AnimationPlayer` per model instance
- particles: emitters as components (fire, smoke and burst presets or
  custom settings), alpha-blended or additive, saved in scene files
- physics (rapier): boxes, spheres and capsules as dynamic, kinematic or
  static bodies, collision events, sensors, raycasts
- audio (kira): WAV/OGG/MP3/FLAC playback with volume, pitch, pan and
  looping; runs silently when there is no audio device
- user interface (egui) for game menus and tools, plus a built-in debug
  overlay (F3): frame statistics, entity list, inspector
- scene files (RON): describe entities in a file, load them, and save the
  world back

Possible next steps: animation blending, collision with
arbitrary meshes, spatial audio, networking.

## Try it

```
cargo run --example cubes   # entities, hierarchy, materials, lights, fly camera
cargo run --example model   # glTF model loading and hot reload
cargo run --example physics # rigid bodies, collisions, sound
cargo run --example scene   # scene file, debug overlay, inspector
cargo run --example animation # skinned and node animation
cargo run --example particles # fire, smoke, fountain, bursts
```

In `cubes`, hold the right mouse button to look around, WASD to move.
Escape quits. Run the examples from the repository root so they find
`assets/`.

On Linux, building needs the ALSA headers (`sudo apt install libasound2-dev`
on Debian/Ubuntu, `alsa-lib-devel` on Fedora). Running needs the usual
desktop libraries (X11 or Wayland, `libxkbcommon`) and a Vulkan driver, which
every desktop distribution ships. Windows uses DirectX 12 or Vulkan and needs
nothing extra.

On NixOS, run `nix-shell` in the repository first: `shell.nix` provides all
of the above. Graphics must be enabled in the system configuration
(`hardware.graphics.enable = true;`).

## Sparkball

The `sparkball` example is a small complete game built only on the engine's
public API: roll a ball around an arena, collect every spark before the clock
runs out, avoid the sweeper and the edges. It has a menu, a HUD, pausing, win
and lose screens and sound, and its level is the scene file
`assets/scenes/arena.ron`. Use it as a starting point for a game of your own.

```
cargo run --example sparkball            # WASD/arrows roll, Space jumps, Esc pauses
cargo run --example sparkball -- --demo  # the game plays itself
```

## Layout

- `crates/engine` – the facade games depend on (`Game` trait, `engine::run`)
- `crates/engine_assets` – reading images and glTF models from disk
- `crates/engine_audio` – sound playback (kira)
- `crates/engine_core` – timing and the fixed-step clock
- `crates/engine_physics` – rigid-body physics (rapier)
- `crates/engine_platform` – window and input (winit)
- `crates/engine_render` – renderer (wgpu)
- `crates/engine_scene` – ECS world, transforms and hierarchy (hecs)
- `crates/engine_ui` – user interface (egui)
- `assets/` – models, textures, sounds and scenes used by the examples

## License

MIT
