# Jewish Game Engine

A general-purpose 3D game engine in Rust for Linux and Windows.

## Status

Early: window, main loop with a fixed simulation step, keyboard and mouse input.
Rendering comes next.

## Try it

```
cargo run --example hello_window
```

On Linux the usual desktop libraries are needed at run time (X11 or Wayland,
`libxkbcommon`); every desktop distribution ships them.

## Layout

- `crates/engine` – the facade games depend on (`Game` trait, `engine::run`)
- `crates/engine_core` – timing and the fixed-step clock
- `crates/engine_platform` – window and input (winit)

## License

MIT
