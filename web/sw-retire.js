// Older versions registered the app's service worker at the site root. The app now lives in /app/ and
// the root is a plain landing page, so this version unregisters itself and clears the old caches.
self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => {
  e.waitUntil((async () => {
    for (const k of await caches.keys()) await caches.delete(k);
    await self.registration.unregister();
    for (const c of await self.clients.matchAll({ type: 'window' })) c.navigate(c.url);
  })());
});
