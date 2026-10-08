#!/usr/bin/env bash
# Build the PWA and publish it to Cloudflare Workers, then verify the files are really served.
# Needs Node >= 22 for wrangler (nvm install 22) and `npx wrangler login` once.
set -euo pipefail
cd "$(dirname "$0")/.."
./scripts/build-web.sh
npx wrangler@latest deploy
URL="${1:-https://spacehunter.rquisen.workers.dev}"
for f in "" spacehunter.js spacehunter_bg.wasm sw.js; do
  code=$(curl -s -o /dev/null -w '%{http_code}' "$URL/$f")
  echo "$code  $URL/$f"
  [ "$code" = 200 ] || { echo "deploy verification FAILED"; exit 1; }
done
