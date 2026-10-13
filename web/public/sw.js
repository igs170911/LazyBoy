const CACHE = "lazyboy-web-v3";

async function warmShell() {
  const cache = await caches.open(CACHE);
  const response = await fetch(new Request("/", {cache:"reload"}));
  if (!response.ok) throw new Error("App shell unavailable");
  const html = await response.clone().text();
  const assets = [...html.matchAll(/(?:src|href)=["'](\/assets\/[^"']+)["']/g)].map(match => match[1]);
  await cache.addAll([...new Set(assets)]);
  await cache.put("/", response);
}

self.addEventListener("install", event => {
  event.waitUntil(warmShell().then(() => self.skipWaiting()));
});
self.addEventListener("activate", (event) => {
  event.waitUntil(caches.keys()
    .then(keys => Promise.all(keys.filter(key => key.startsWith("lazyboy-web-") && key !== CACHE).map(key => caches.delete(key))))
    .then(() => self.clients.claim()));
});

async function cachedStatic(request) {
  const cache = await caches.open(CACHE).catch(() => null);
  const url = new URL(request.url);
  const fingerprinted = url.pathname.startsWith("/assets/");
  const cached = await cache?.match(request).catch(() => null);
  if (fingerprinted && cached) return cached;
  try {
    const response = await fetch(request);
    if (cache && response.ok && response.type !== "opaque") {
      // A full/disabled cache must not turn a successful response into an error.
      await cache.put(request, response.clone()).then(async () => {
        const keys = await cache.keys();
        if (keys.length > 256) await Promise.all(keys.filter(key => new URL(key.url).pathname.startsWith("/assets/")).slice(0, keys.length - 256).map(key => cache.delete(key)));
      }).catch(() => {});
    }
    if (response.status >= 500 && cached) return cached;
    return response;
  } catch {
    return cached || (request.mode === "navigate" ? await cache?.match("/").catch(() => null) : null)
      || new Response("Network unavailable. Please reconnect and retry.", {status:503,headers:{"content-type":"text/plain; charset=utf-8"}});
  }
}

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin || url.pathname.startsWith("/api/") || url.pathname.startsWith("/novnc") || event.request.method !== "GET") return;
  event.respondWith(cachedStatic(event.request));
});

// A notification about a bot opens (or focuses) the app on that bot.
self.addEventListener("notificationclick", event => {
  event.notification.close();
  const agentId = event.notification.data && event.notification.data.agentId;
  event.waitUntil(self.clients.matchAll({ type: "window", includeUncontrolled: true }).then(windows => {
    const open = windows[0];
    if (open) { open.postMessage({ type: "open-bot", agentId }); return open.focus(); }
    return self.clients.openWindow("/?bot=" + encodeURIComponent(agentId || ""));
  }));
});
