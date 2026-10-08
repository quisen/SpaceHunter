# Space Hunter delivery rules

## Keep Windows and web in sync

Space Hunter ships as a Windows portable executable AND a web app. The download
button uses `https://github.com/quisen/SpaceHunter/releases/latest/download/SpaceHunter.exe`.
Building only `dist/` does not update that executable.

For every app change (Rust code, UI, translations, behavior, embedded assets):
1. Run `cargo fmt --all --check`, `cargo test --workspace`, and `cargo clippy --workspace --all-targets`.
2. Regenerate affected icons/screenshots and run `./scripts/build-web.sh`. The web
   build in `dist/` is committed. Keep `Cargo.toml` / `Cargo.lock` version in sync.
3. When publishing is authorized, commit and push the reviewed changes to `main`.
   `.github/workflows/release.yml` builds Windows and publishes an immutable release
   with `SpaceHunter.exe`, `build-info.json`, and `SpaceHunter.exe.sha256`.
4. Wait for that workflow, then run `python3 scripts/verify-release.py --commit
   "$(git rev-parse HEAD)"`. This downloads the public .exe and checks its SHA-256
   against the release metadata. A pending/failed job is NOT an updated download.
5. Run `./scripts/deploy.sh` to publish the web build. This checks the Windows release
   matches HEAD first. Verify both the landing and `/app/` assets after deployment.
6. Report the release link and the verified version/commit. If publication was not
   authorized or failed, explicitly report that the public .exe is still pending.

Do not call an app update published or complete while its downloadable executable
still contains an older app build. Do not silently skip the Windows build.
Documentation-only or portfolio-only changes do not require a new app build unless
bundled with app changes. For local-only tasks, build/validate locally and make the
pending release explicit; these rules do not grant new publication permission.

## Portfolio reference

The related portfolio lives at `/mnt/c/Users/rquis/Documents/quisen.com.br` (Windows:
`C:\Users\rquis\Documents\quisen.com.br`). Its product carousels use actual app captures.
Keep Space Hunter screenshots and PT-BR/EN-US product copy consistent with releases.

To refresh the portfolio carousel after a visible UI change: serve this repo's `dist/` on
`localhost:8080`, then in the portfolio run `node scripts/capture-spacehunter.mjs` with
`CHROME_PATH` (Chrome or chrome-headless-shell) and `FFMPEG_PATH` (an ffmpeg with libx264, e.g.
the `ffmpeg-static` npm package). It writes posters to `assets/projects/spacehunter/` and H.264
loops to `public/spacehunter/`. The app accepts `?demo`, `?3d`, `?lang=pt-BR|en-US` and `?setup`
URL flags for deterministic screenshots. The portfolio deploys from `main` on push.
