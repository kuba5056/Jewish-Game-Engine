# My Game

A starter project for the
[Jewish Game Engine](https://github.com/kuba5056/Jewish-Game-Engine).

```
cargo run
```

WASD or the arrow keys roll the ball, Space jumps, F3 opens the debug
overlay, Escape quits. On NixOS run `nix-shell` first. On Debian/Ubuntu
install `libasound2-dev` once.

## What is here

- `src/main.rs` – the game: player movement, camera and a small HUD.
- `assets/scenes/main.ron` – the level: floor, light, player, a crate and
  a scripted beacon. Edit it by hand or with the engine's editor.
- `assets/scripts/spin.rhai` – a script attached to the beacon. Change it
  while the game runs and it reloads.
- `Cargo.toml` – one dependency, the engine, fetched from its repository.

## Where to go next

The engine's `docs/GUIDE.md` explains everything used here and the rest of
the API. Its `crates/engine/examples/sparkball.rs` is a complete small
game to borrow from.

Rename the package in `Cargo.toml` and the window title in `src/main.rs`,
then make it yours.
