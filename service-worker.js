const SHELL_CACHE = "spbftuit-shell-v10";
const DATA_CACHE = "spbftuit-data-v1";
const SHELL_FILES = [
  "./", "./index.html", "./miniapp/index.html", "./miniapp/week.html",
  "./miniapp/app.js", "./miniapp/styles.css", "./miniapp/native-bridge.js",
  "./week.html", "./manifest.webmanifest", "./offline.html", "./favicon.png",
  "./data/groups.json", "./data/teachers.json", "./data/campus.json",
  "./data/campus-park-scene.json", "./miniapp/vendor/maplibre-gl.js",
  "./miniapp/vendor/maplibre-gl.css", "./miniapp/vendor/maplibre-gl-LICENSE.txt"
];
self.addEventListener("install", (event) => {
  event.waitUntil((async () => {
    const cache = await caches.open(SHELL_CACHE);
    await Promise.all(SHELL_FILES.map(async (path) => {
      const url = new URL(path, self.registration.scope).href;
      try { const response = await fetch(url, { cache: "reload" }); if (response.ok) await cache.put(url, response); }
      catch { /* Continue with a partial shell if one optional file is unavailable. */ }
    }));
    await self.skipWaiting();
  })());
});
self.addEventListener("activate", (event) => {
  event.waitUntil((async () => {
    const names = await caches.keys();
    await Promise.all(names.filter((name) => name.startsWith("spbftuit-") && ![SHELL_CACHE, DATA_CACHE].includes(name)).map((name) => caches.delete(name)));
    await self.clients.claim();
  })());
});
async function networkFirst(request, cacheName, navigation = false) {
  const cache = await caches.open(cacheName);
  try {
    const response = await fetch(request);
    if (response.ok) await cache.put(request, response.clone());
    return response;
  } catch {
    const cached = await cache.match(request, { ignoreSearch: true }) || await caches.match(request, { ignoreSearch: true });
    if (cached) return cached;
    if (navigation) {
      const offline = await caches.match(new URL("./offline.html", self.registration.scope).href);
      if (offline) return offline;
    }
    return new Response(navigation ? "Нет подключения" : "Ресурс ещё не сохранён на устройстве", { status: 503, headers: { "Content-Type": "text/plain; charset=utf-8" } });
  }
}
self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin || url.pathname.includes("/api/")) return;
  if (url.pathname.includes("/data/") && url.pathname.endsWith(".json")) {
    event.respondWith(networkFirst(request, DATA_CACHE)); return;
  }
  if (request.mode === "navigate") {
    event.respondWith(networkFirst(request, SHELL_CACHE, true)); return;
  }
  if (/\.(?:js|css|html|webmanifest|png|jpe?g|svg|ico)$/i.test(url.pathname)) event.respondWith(networkFirst(request, SHELL_CACHE));
});
