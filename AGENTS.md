# Notes for AI agents (and new contributors)

This repository is a 3D game engine in Rust for Linux and Windows, with a
scene editor, examples and a starter project.

## If you were asked to make a game with this engine

1. Read `docs/GUIDE.md`. It covers the whole public API, the scene and
   script formats, the frame order and a list of pitfalls.
2. Start from `template/`: copy it out of the repository into a new
   directory. It is a complete, running project that depends on the engine
   through git. Do not build a game inside this repository.
3. Use only the `engine` crate's exports. The other `engine_*` crates are
   internal.
4. Read the example closest to what you are building before writing code:

   | Example (`crates/engine/examples/`) | Shows |
   |---|---|
   | `sparkball.rs` | a complete game: menu, HUD, level from a scene, physics, pickups, sound |
   | `scripted.rs` + `assets/scripts/` | a game made only of a scene file and scripts |
   | `physics.rs` | rigid bodies, collision events, sound on impact |
   | `animation.rs` | glTF models, skeletal animation |
   | `particles.rs` | particle effects |
   | `scene.rs` | loading a scene, the debug overlay |
   | `cubes.rs` | entities, hierarchy, materials, lights, a fly camera |

5. Verify by running, not only by compiling. `docs/GUIDE.md` section 17
   explains how to run a game without a display and take screenshots.

Prefer data over code where it fits: levels belong in scene files
(`.ron`), simple behaviour in scripts (`.rhai`), and both reload while the
game runs.

## If you are changing the engine itself

- Layout: `crates/engine` is the facade games use; `crates/engine_*` are
  its parts (core, platform, render, scene, assets, physics, audio, ui,
  script); `crates/editor` is the scene editor; `assets/` holds files for
  the examples; `template/` is the starter project (not a workspace
  member).
- Before committing, all of these must pass:

  ```
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```

  CI runs the same on Linux and Windows and also builds `template/`
  against the working tree.
- The Rust version is pinned in `rust-toolchain.toml` and in
  `.github/workflows/ci.yml`; change both together.
- Building on Linux needs the ALSA headers (`libasound2-dev`). On NixOS use
  `nix-shell`.
- Keep `docs/GUIDE.md`, `template/` and the examples in step with API
  changes: they are what a newcomer reads first.
- The dependencies (wgpu 30, glam 0.34, egui 0.37, hecs 0.11, rapier3d
  0.36, kira 0.12, rhai 1.27) have APIs that differ from older versions.
  Check their source in `~/.cargo/registry/src` instead of trusting memory.
