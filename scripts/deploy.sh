#!/usr/bin/env bash
# Publish the web app only after the matching Windows executable is publicly available.
# Requires Node >= 22 and an authenticated Wrangler session.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -n "$(git status --porcelain)" ]]; then
  echo 'Commit the reviewed app and generated dist changes before publishing.' >&2
  exit 1
fi
python3 scripts/verify-release.py --commit "$(git rev-parse HEAD)"
# dist/ is the reviewed, committed build generated before the Windows release.
npx wrangler@4.149.0 deploy
URL="${1:-https://spacehunter.rquisen.workers.dev}"
for f in "" app/ app/spacehunter.js app/spacehunter_bg.wasm app/sw.js app/manifest.webmanifest; do
  code=$(curl -s -o /dev/null -w '%{http_code}' "$URL/$f")
  echo "$code  $URL/$f"
  [ "$code" = 200 ] || { echo 'Deploy verification failed' >&2; exit 1; }
done
