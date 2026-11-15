# Development shell for NixOS / Nix: `nix-shell`, then `cargo run --example sparkball`.
{ pkgs ? import <nixpkgs> { } }:
let
  libs = with pkgs; [
    alsa-lib
    libxkbcommon
    wayland
    vulkan-loader
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    xorg.libXrandr
    xorg.libxcb
  ];
in
pkgs.mkShell {
  # rustup installs the compiler version pinned in rust-toolchain.toml.
  nativeBuildInputs = with pkgs; [ rustup pkg-config ];
  buildInputs = libs;
  # The window system, Vulkan and audio libraries are loaded at run time,
  # so they must be on the library path, not only available at build time.
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath libs;
}
