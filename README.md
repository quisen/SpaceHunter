# SpaceHunter

A fast, modern re-creation of the classic **SpaceMonger** disk-space visualiser, written in Rust.
See where your space went — in **2D** (nested treemap) or **3D** (extruded city).

* **Windows `.exe`** (native, GPU accelerated) — also builds for Linux/macOS
* **PWA** (installable, offline, WebAssembly) — pick a folder, nothing is uploaded

![toolbar of the original](research/assets/toolbar.png)

## Features
- Parallel scanner (~1 M entries/s on SSD), size-on-disk accounting, hard-link/symlink/junction safe
- **Classic** layout (exact SpaceMonger algorithm, density + bias) and **Squarified** layout
- 2D treemap with folder frames, labels, hover tooltips, selection, zoom in/out/full (animated)
- 3D view: orbit / pan / zoom, height ∝ file size, picking, depth-buffered instanced rendering
- Colours: by file type, original Rainbow palette, depth, size heat-map, monochrome
- Breadcrumb, folder contents, largest-files list, free-space block, Run/Open, optional Delete

## Controls
| | |
|---|---|
| Double-click folder / `Enter` | zoom in |
| `Backspace` / wheel down | zoom out |
| `Home` | zoom full |
| `F5` | rescan |
| `Ctrl+O` | open folder |
| `2` / `3` | 2D / 3D |
| 3D: drag / right-drag / wheel | orbit / pan / dolly |

## Build
```sh
# native
cargo run --release -p spacehunter -- [PATH] [--3d] [--demo]

# Windows .exe from Linux (needs zig: pip install ziglang; cargo install cargo-zigbuild)
rustup target add x86_64-pc-windows-gnu
cargo zigbuild --release --target x86_64-pc-windows-gnu -p spacehunter --bin spacehunter

# PWA (needs: rustup target add wasm32-unknown-unknown; cargo install wasm-bindgen-cli)
./scripts/build-web.sh && python3 -m http.server -d dist 8080   # http://localhost:8080
```
The PWA needs HTTPS (or `localhost`) to be installable; folder access uses the File System Access API (Chromium) with an `<input webkitdirectory>` fallback elsewhere.

## Layout
- `crates/core` – tree model, scanner, layouts (`classic`, squarified), formatting. No UI dependencies.
- `crates/app` – egui/eframe UI, 2D mesh renderer, 3D GL renderer, native + web platform glue.
- `research/` – reverse-engineering notes of SpaceMonger 1.4 ([FINDINGS.md](research/FINDINGS.md)) and tools.

MIT licensed. SpaceMonger is © Sean Werkema; this project contains no code from it.
