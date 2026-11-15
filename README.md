# Jewish Game Engine

A general-purpose 3D game engine in Rust for Linux and Windows.

## Status

Early. Working so far:

- window, main loop with a fixed simulation step, keyboard and mouse input
- renderer: meshes, perspective camera, textures (PNG/JPEG or raw pixels,
  with mipmaps), depth buffer, one directional light, vsync

Next: entities and scenes (ECS), then glTF asset loading.

## Try it

```
cargo run --example cubes
```

Hold the right mouse button to look around, WASD to move, Escape to quit.

On Linux the usual desktop libraries are needed at run time (X11 or Wayland,
`libxkbcommon`) plus a Vulkan driver; every desktop distribution ships them.
Windows uses DirectX 12 or Vulkan, with nothing extra to install.

## Layout

- `crates/engine` – the facade games depend on (`Game` trait, `engine::run`)
- `crates/engine_core` – timing and the fixed-step clock
- `crates/engine_platform` – window and input (winit)
- `crates/engine_render` – renderer (wgpu)

## License

MIT
