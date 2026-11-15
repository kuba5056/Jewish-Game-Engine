# Jewish Game Engine

A general-purpose 3D game engine in Rust for Linux and Windows.

## Status

Early. Working so far:

- window, main loop with a fixed simulation step, keyboard and mouse input
- renderer: meshes, perspective camera, textures (PNG/JPEG or raw pixels,
  with mipmaps), depth buffer, one directional light, vsync
- entities and components (hecs ECS), parent/child transform hierarchy,
  systems; entities with a `MeshRenderer` are drawn automatically
- assets: glTF models and PNG/JPEG textures loaded from an `assets/`
  directory, shared when loaded twice, and reloaded on screen when the
  files change (debug builds)

Next: PBR lighting and shadows, then physics and audio.

## Try it

```
cargo run --example cubes   # entities, hierarchy, fly camera
cargo run --example model   # glTF model loading and hot reload
```

In `cubes`, hold the right mouse button to look around, WASD to move.
Escape quits. Run the examples from the repository root so they find
`assets/`.

On Linux the usual desktop libraries are needed at run time (X11 or Wayland,
`libxkbcommon`) plus a Vulkan driver; every desktop distribution ships them.
Windows uses DirectX 12 or Vulkan, with nothing extra to install.

## Layout

- `crates/engine` – the facade games depend on (`Game` trait, `engine::run`)
- `crates/engine_assets` – reading images and glTF models from disk
- `crates/engine_core` – timing and the fixed-step clock
- `crates/engine_platform` – window and input (winit)
- `crates/engine_render` – renderer (wgpu)
- `crates/engine_scene` – ECS world, transforms and hierarchy (hecs)

## License

MIT
