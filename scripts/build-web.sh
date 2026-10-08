#!/usr/bin/env bash
# Builds the PWA into ./dist (serve it over https or http://localhost).
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -p spacehunter --lib --target wasm32-unknown-unknown --release
rm -rf dist && mkdir -p dist
wasm-bindgen --target web --no-typescript --out-dir dist target/wasm32-unknown-unknown/release/spacehunter.wasm
if command -v wasm-opt >/dev/null; then wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int -o dist/spacehunter_bg.wasm dist/spacehunter_bg.wasm; fi
cp -r web/index.html web/manifest.webmanifest web/icons dist/
BUILD=$(sha1sum dist/spacehunter_bg.wasm | cut -c1-10)
sed "s/__BUILD__/$BUILD/" web/sw.js > dist/sw.js
ls -la dist
