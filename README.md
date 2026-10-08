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

## 2D overview

The default 2D map shows one folder level at a time. Tile area represents size;
small entries are grouped as Other items. Select a tile for details, double-click
a folder to explore it, and use Backspace or breadcrumbs to return. Double-click
Other items to inspect the detailed map. All files keeps the recursive map available,
with a limited number of readable labels. Overview rendering caches its mesh and
caps labels at 16 tiles instead of drawing text throughout the entire subtree.

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
### Publish Windows and web together

Follow [AGENTS.md](AGENTS.md). Build and commit `dist/`, then push `main`.
GitHub Actions publishes the Windows binary with an immutable commit tag, build
metadata and SHA-256. Once the Windows workflow succeeds:

```sh
python3 scripts/verify-release.py --commit "$(git rev-parse HEAD)"
./scripts/deploy.sh
```

The verification downloads the actual public executable and checks its commit and
checksum. `deploy.sh` refuses to publish with uncommitted changes or a stale Windows
release. The download URL remains `/releases/latest/download/SpaceHunter.exe`.

The worker must serve `index.html`, `spacehunter.js`, `spacehunter_bg.wasm`, `sw.js`, `manifest.webmanifest` and `icons/` from the same folder.

The PWA needs HTTPS (or `localhost`) to be installable; folder access uses the File System Access API (Chromium) with an `<input webkitdirectory>` fallback elsewhere.

## Layout
- `crates/core` – tree model, scanner, layouts (`classic`, squarified), formatting. No UI dependencies.
- `crates/app` – egui/eframe UI, 2D mesh renderer, 3D GL renderer, native + web platform glue.
- `research/` – reverse-engineering notes of SpaceMonger 1.4 ([FINDINGS.md](research/FINDINGS.md)) and tools.

MIT licensed. SpaceMonger is © Sean Werkema; this project contains no code from it.

## Language and visual identity

The app and landing support `pt-BR` and `en-US`. Choose the language in the app
 toolbar or the landing header. The browser shares and remembers that preference;
 the native app saves it in its configuration directory. On native systems,
 `SPACEHUNTER_LANG=pt-BR` sets the initial language when there is no saved preference.

Starting without arguments shows the welcome screen. Folder selection requires a
 click on **Choose a folder**, or `Ctrl+O`. `F5` only rescans a loaded folder.
 Explicit `PATH` and `--demo` arguments still load their requested data at startup.

The editable brand source is `web/icons/logo.svg`; the visual guidelines are in
 `web/landing/brand/identity.md`. Run `python3 scripts/make_icons.py` to regenerate
 the matching PNG icons, then `./scripts/build-web.sh` to update the committed web build.
