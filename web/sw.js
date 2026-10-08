// SpaceHunter service worker: offline-first app shell. __BUILD__ is replaced by scripts/build-web.sh.
const CACHE = 'spacehunter-__BUILD__';
const SHELL = ['./', './index.html', './manifest.webmanifest', './spacehunter.js', './spacehunter_bg.wasm',
  './icons/logo.svg', './icons/icon-192.png', './icons/icon-512.png', './icons/maskable-512.png', './icons/favicon-32.png'];

self.addEventListener('install', (e) => {
  e.waitUntil(caches.open(CACHE).then((c) => c.addAll(SHELL)).then(() => self.skipWaiting()));
});
self.addEventListener('activate', (e) => {
  e.waitUntil(
    caches.keys().then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()));
});
// Network-first (so a new deploy is picked up immediately), cache as the offline fallback.
// Only successful same-origin responses are cached, so a 404 can never poison the cache.
self.addEventListener('fetch', (e) => {
  if (e.request.method !== 'GET' || new URL(e.request.url).origin !== location.origin) return;
  e.respondWith(
    fetch(e.request).then((res) => {
      if (res.ok) {
        const copy = res.clone();
        caches.open(CACHE).then((c) => c.put(e.request, copy));
      }
      return res;
    }).catch(() => caches.match(e.request, { ignoreSearch: true }).then((hit) => hit || caches.match('./index.html'))));
});
