# Nova Files

A fast, rounded file manager for Linux, written in Rust on [GPUI](https://gpui.rs), the UI framework behind Zed. It is a standalone app and does not depend on any one distro or desktop.

This is an early scaffold: a window with a sidebar of places and a list of what is in the current folder. Click a place or a folder to open it.

## Run it

### With Nix

```sh
nix run github:ImperaLuna/Nova-Files
```

From a checkout:

```sh
nix run          # build and start the app
nix build        # build it into ./result
nix develop      # a shell with cargo, rust-analyzer and every native library
cargo run        # inside `nix develop`
```

To install it on NixOS or with Home Manager, add the flake as an input and use its overlay or package:

```nix
# flake.nix
inputs.nova-files.url = "github:ImperaLuna/Nova-Files";

# configuration.nix or home.nix
nixpkgs.overlays = [ inputs.nova-files.overlays.default ];
environment.systemPackages = [ pkgs.nova-files ];   # or home.packages
```

### Without Nix

You need a recent stable Rust toolchain and the development packages for Vulkan, Wayland, X11 (libxcb, libX11), xkbcommon, fontconfig and freetype, plus `pkg-config`, `cmake` and `clang`. Then:

```sh
cargo run --release
```

On Debian or Ubuntu that is roughly:

```sh
sudo apt install pkg-config cmake clang libvulkan1 libwayland-dev \
  libxkbcommon-x11-dev libxcb1-dev libx11-xcb-dev libfontconfig-dev libfreetype-dev
```

## Layout

- `src/main.rs`: the whole app for now.
- `nix/package.nix`: the Nix package, also used by the overlay.
- `flake.nix`: package, overlay and dev shell for `x86_64-linux` and `aarch64-linux`.

GPUI is pinned to the `gpui` 0.2.2 release on crates.io through `Cargo.lock`.
