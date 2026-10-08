#!/usr/bin/env bash
# Builds the PWA into ./dist (serve it over https or http://localhost).
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -p spacehunter --lib --target wasm32-unknown-unknown --release
rm -rf dist && mkdir -p dist/app
wasm-bindgen --target web --no-typescript --out-dir dist/app target/wasm32-unknown-unknown/release/spacehunter.wasm
if command -v wasm-opt >/dev/null; then wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int -o dist/app/spacehunter_bg.wasm dist/app/spacehunter_bg.wasm; fi
# dist/        = landing page (+ a self-removing sw.js that retires the service worker older versions registered at the root)
# dist/app/    = the PWA itself
cp -r web/landing/. dist/
cp web/sw-retire.js dist/sw.js
cp -r web/index.html web/manifest.webmanifest web/icons dist/app/
BUILD=$(sha1sum dist/app/spacehunter_bg.wasm | cut -c1-10)
sed "s/__BUILD__/$BUILD/" web/sw.js > dist/app/sw.js
ls -la dist dist/app
