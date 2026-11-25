// Velox browser extension – service worker.
//
// * Captures browser downloads and hands them to the Velox app through the
//   `com.velox.host` native messaging host (Alt+click bypasses capture).
// * Detects HLS / DASH / direct media streams per tab.
// * Adds "Download with Velox" context menu entries.

const HOST = "com.velox.host";
const DEFAULT_EXT =
  "zip rar 7z gz xz tar iso exe msi dmg apk deb rpm mp4 mkv avi mov webm mp3 flac wav m4a pdf epub bin img".split(" ");

const state = {
  connected: false,
  app: false,
  version: null,
  capture: { enabled: true, extensions: DEFAULT_EXT, minSizeKb: 512 },
  lastPing: 0,
};

let bypassUntil = 0;
const bypassUrls = new Set();
/** tabId -> Array<{url, kind, mime, size, title, ts}> */
const media = new Map();

// ─────────────────────────── native messaging ───────────────────────────

// One long-lived native port instead of spawning velox-host per message:
// messages go out instantly and the open port keeps the worker alive.
let port = null;
let seq = 0;
const waiting = new Map();

function openPort() {
  if (port) return port;
  try {
    port = chrome.runtime.connectNative(HOST);
  } catch (e) {
    port = null;
    return null;
  }
  port.onMessage.addListener((m) => {
    const done = waiting.get(m?.requestId);
    if (done) {
      waiting.delete(m.requestId);
      done(m);
    }
  });
  port.onDisconnect.addListener(() => {
    const error = chrome.runtime.lastError?.message || "disconnected";
    port = null;
    for (const done of waiting.values()) done({ ok: false, error });
    waiting.clear();
  });
  return port;
}

function send(msg, timeout = msg.type === "ping" ? 3000 : 20000) {
  return new Promise((resolve) => {
    const p = openPort();
    if (!p) return resolve({ ok: false, error: "native host unavailable" });
    const id = ++seq;
    waiting.set(id, resolve);
    try {
      p.postMessage({ ...msg, requestId: id });
    } catch (e) {
      waiting.delete(id);
      port = null;
      return resolve({ ok: false, error: String(e) });
    }
    setTimeout(() => {
      if (waiting.delete(id)) resolve({ ok: false, error: "timeout" });
    }, timeout);
  });
}

// Video pages are resolved in the background as soon as they open, so the
// download dialog has title, size and qualities the moment you click.
const prefetched = new Map();
function prefetch(url) {
  if (!url || !state.connected) return;
  const last = prefetched.get(url);
  if (last && Date.now() - last < 9 * 60_000) return;
  prefetched.set(url, Date.now());
  send({ type: "prefetch", url }, 3000);
}

async function ping(force = false) {
  if (!force && Date.now() - state.lastPing < 10_000) return state;
  const r = await send({ type: "ping" });
  state.lastPing = Date.now();
  state.connected = !!r.ok;
  state.app = !!r.app;
  state.version = r.version ?? null;
  if (r.capture) state.capture = r.capture;
  updateBadge();
  return state;
}

async function isPaused() {
  const { paused } = await chrome.storage.local.get("paused");
  return !!paused;
}

async function cookiesFor(url) {
  try {
    const list = await chrome.cookies.getAll({ url });
    return list.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch {
    return "";
  }
}

async function handoff({ url, fileName, referrer, headers, kind, size, silent }) {
  const cookies = await cookiesFor(url);
  const r = await send({
    type: "add",
    url,
    fileName: fileName || null,
    referrer: referrer || null,
    cookies: cookies || null,
    userAgent: navigator.userAgent,
    headers: headers || [],
    kind: kind || null,
    sizeHint: size > 0 ? size : null,
    silent: !!silent,
  });
  if (r.ok) {
    state.connected = true;
    state.app = true;
    updateBadge();
  }
  return r;
}

// ─────────────────────────── download capture ───────────────────────────

function nameFromUrl(u) {
  try {
    const p = new URL(u).pathname.split("/").filter(Boolean).pop() || "";
    return decodeURIComponent(p);
  } catch {
    return "";
  }
}

function extOf(name) {
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(i + 1).toLowerCase() : "";
}

function shouldCapture(item) {
  const url = item.finalUrl || item.url;
  if (!/^https?:/i.test(url)) return false; // blob:, data:, file: stay in the browser
  if (item.byExtensionId === chrome.runtime.id) return false;
  if (Date.now() < bypassUntil) return false;
  if (bypassUrls.has(url) || bypassUrls.has(item.url)) {
    bypassUrls.delete(url);
    bypassUrls.delete(item.url);
    return false;
  }
  const cap = state.capture;
  if (!cap.enabled) return false;
  const name = (item.filename || "").split(/[\\/]/).pop() || nameFromUrl(url);
  const ext = extOf(name);
  const size = item.fileSize > 0 ? item.fileSize : item.totalBytes;
  if (size > 0 && size < cap.minSizeKb * 1024) return false;
  if (cap.extensions.includes(ext)) return true;
  // Unknown type but clearly big: let Velox accelerate it.
  return !ext && size > 8 * 1024 * 1024;
}

chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  (async () => {
    if (await isPaused()) return suggest();
    await ping();
    if (!state.connected || !shouldCapture(item)) return suggest();

    const url = item.finalUrl || item.url;
    const fileName = (item.filename || "").split(/[\\/]/).pop() || nameFromUrl(url);
    try {
      await chrome.downloads.cancel(item.id);
      await chrome.downloads.erase({ id: item.id });
    } catch {}
    suggest();

    const r = await handoff({
      url,
      fileName,
      referrer: item.referrer,
      size: item.fileSize > 0 ? item.fileSize : item.totalBytes,
    });
    if (!r.ok) {
      // Velox could not take it – give it back to the browser.
      bypassUrls.add(url);
      chrome.downloads.download({ url, filename: fileName || undefined });
    }
  })();
  return true; // we call suggest asynchronously
});

// ─────────────────────────── video sites ───────────────────────────
// Pages whose videos are resolved by Velox (yt-dlp) instead of sniffing.

// Mirrors velox-core `site::is_site_url`: only pages that are one video.
function isSiteUrl(url) {
  let u;
  try {
    u = new URL(url);
  } catch {
    return false;
  }
  const host = u.hostname.replace(/^(www|m|mobile)\./, "");
  const p = u.pathname;
  switch (host) {
    case "youtube.com":
    case "music.youtube.com":
      return /^\/(watch|shorts\/|live\/)/.test(p);
    case "youtu.be":
      return p.length > 1;
    case "x.com":
    case "twitter.com":
      return p.includes("/status/");
    case "instagram.com":
      return /^\/(p|reels?|tv)\//.test(p);
    case "tiktok.com":
      return p.includes("/video/");
    case "facebook.com":
      return p.includes("/videos/") || p.startsWith("/watch") || p.startsWith("/reel/");
    case "reddit.com":
    case "old.reddit.com":
      return p.includes("/comments/");
    case "twitch.tv":
      return p.includes("/videos/") || p.includes("/clip/");
    case "clips.twitch.tv":
    case "vimeo.com":
    case "dailymotion.com":
    case "soundcloud.com":
    case "bilibili.com":
    case "streamable.com":
      return p.length > 1;
    default:
      return false;
  }
}

/** Keep a "this page's video" entry at the top of the tab's media list. */
function syncSiteItem(tabId, url, title) {
  let list = (media.get(tabId) || []).filter((m) => m.kind !== "site");
  if (url && isSiteUrl(url)) {
    list = [{ key: "site:" + url, url, kind: "site", title: (title || "").replace(/ - YouTube$/, ""), size: 0, ts: Date.now() }, ...list];
    prefetch(url);
  }
  media.set(tabId, list);
  persistMedia();
  updateBadge(tabId);
}

// ─────────────────────────── stream detection ───────────────────────────

function header(headers, name) {
  const h = headers?.find((x) => x.name.toLowerCase() === name);
  return h ? h.value : "";
}

function classify(url, mime) {
  const path = url.split(/[?#]/)[0].toLowerCase();
  const m = (mime || "").toLowerCase();
  if (path.endsWith(".m3u8") || m.includes("mpegurl")) return "hls";
  if (path.endsWith(".mpd") || m.includes("dash+xml")) return "dash";
  if (m.startsWith("video/") || m.startsWith("audio/")) return "http";
  if (/\.(mp4|webm|mkv|mov|m4v|mp3|m4a|flac|ogg)$/.test(path)) return "http";
  return null;
}

// Players often live in an iframe on another domain and their CDN checks the
// Referer / Origin of that player. Remember exactly what the browser sent for
// each media request so Velox can replay it.
const sent = new Map(); // requestId -> { referer, origin }
const MEDIA_TYPES = ["media", "xmlhttprequest", "other"];
chrome.webRequest.onSendHeaders.addListener(
  (d) => {
    if (d.tabId < 0) return;
    const referer = header(d.requestHeaders, "referer");
    const origin = header(d.requestHeaders, "origin");
    if (referer || origin) sent.set(d.requestId, { referer, origin });
    if (sent.size > 2000) sent.delete(sent.keys().next().value);
  },
  { urls: ["<all_urls>"], types: MEDIA_TYPES },
  ["requestHeaders", "extraHeaders"],
);
const forget = (d) => sent.delete(d.requestId);
chrome.webRequest.onCompleted.addListener(forget, { urls: ["<all_urls>"], types: MEDIA_TYPES });
chrome.webRequest.onErrorOccurred.addListener(forget, { urls: ["<all_urls>"], types: MEDIA_TYPES });

// Pre-roll / ad networks – their clips are never what the user wants.
const AD_HOSTS = /(^|\.)(doubleclick\.net|googlesyndication\.com|googleadservices\.com|imasdk\.googleapis\.com|adnxs\.com|adsrvr\.org|advertising\.com|amazon-adsystem\.com|criteo\.com|taboola\.com|outbrain\.com|spotxchange\.com|springserve\.com|pubmatic\.com|rubiconproject\.com|vidoomy\.com|teads\.tv|smartadserver\.com|adform\.net)$/i;
function isAd(url) {
  try {
    const u = new URL(url);
    return AD_HOSTS.test(u.hostname) || /\/(ads?|vast|vpaid|preroll|commercials?)\//i.test(u.pathname);
  } catch {
    return false;
  }
}

const RANK = { site: 0, hls: 1, dash: 2, http: 3 };
/** Best candidates first: the page's video, streams (first seen = master
 *  playlist), then direct files by size. */
function rank(list) {
  return list.sort((a, b) => (RANK[a.kind] ?? 9) - (RANK[b.kind] ?? 9) || (a.kind === "http" ? (b.size || 0) - (a.size || 0) : a.ts - b.ts));
}

let saveTimer = 0;
function persistMedia() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    const obj = {};
    for (const [k, v] of media) obj[k] = v;
    chrome.storage.session.set({ media: obj }).catch(() => {});
  }, 300);
}

chrome.storage.session
  .get("media")
  .then(({ media: saved }) => {
    if (saved) for (const [k, v] of Object.entries(saved)) media.set(+k, v);
  })
  .catch(() => {});

chrome.webRequest.onHeadersReceived.addListener(
  (d) => {
    const req = sent.get(d.requestId);
    sent.delete(d.requestId);
    if (d.tabId < 0 || d.statusCode >= 400) return;
    if (isAd(d.url)) return;
    // YouTube's adaptive chunks are useless on their own – the page is
    // handled as a site download instead.
    if (/(^https?:\/\/[^/]*\.(googlevideo|ytimg)\.com\/)/.test(d.url)) return;
    const mime = header(d.responseHeaders, "content-type");
    const kind = classify(d.url, mime);
    if (!kind) return;
    const size = +header(d.responseHeaders, "content-length") || 0;
    // Players fetch big files in small ranges – judge by the full size.
    const range = header(d.responseHeaders, "content-range");
    const total = range ? +range.split("/")[1] || size : size;
    // Skip tiny media (thumbnails, beeps) and the many segment chunks of streams.
    if (kind === "http" && ((total > 0 && total < 1024 * 1024) || /\.(ts|m4s|aac)(\?|$)/i.test(d.url))) return;
    let list = media.get(d.tabId) || [];
    const key = d.url.split("#")[0];
    // Range requests of the same file show up many times – keep one entry.
    const base = kind === "http" ? key.replace(/[?&](range|bytes)=[^&]*/gi, "") : key;
    if (list.some((m) => m.key === base)) return;
    const item = {
      key: base,
      url: d.url,
      kind,
      mime,
      size: total,
      ts: Date.now(),
      frameId: d.frameId,
      referer: req?.referer || "",
      origin: req?.origin || "",
      // Fallback when the browser sent no Referer: the requesting document.
      initiator: d.initiator || "",
    };
    list = rank([item, ...list]).slice(0, 30);
    media.set(d.tabId, list);
    chrome.tabs.get(d.tabId).then((tab) => {
      for (const m of list) if (!m.title) m.title = tab.title;
      persistMedia();
    }, () => {});
    updateBadge(d.tabId);
  },
  { urls: ["<all_urls>"], types: ["media", "xmlhttprequest", "other"] },
  ["responseHeaders"],
);

chrome.tabs.onUpdated.addListener((tabId, info, tab) => {
  if (info.url) {
    // Real navigation clears sniffed streams; SPA navigations (YouTube) too.
    media.delete(tabId);
    syncSiteItem(tabId, info.url, tab.title);
  } else if (info.title || info.status === "complete") {
    syncSiteItem(tabId, tab.url, tab.title);
  }
});
chrome.tabs.onRemoved.addListener((tabId) => {
  media.delete(tabId);
  persistMedia();
});
chrome.tabs.onActivated.addListener(({ tabId }) => updateBadge(tabId));

// No counters on the toolbar icon – the popup lists what was found.
function updateBadge(tabId) {
  if (tabId != null) chrome.action.setBadgeText({ tabId, text: "" }).catch(() => {});
  else chrome.action.setBadgeText({ text: "" }).catch(() => {});
}

// ─────────────────────────── context menus ───────────────────────────

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({ id: "velox-link", title: "Download with Velox", contexts: ["link"] });
    chrome.contextMenus.create({ id: "velox-media", title: "Download media with Velox", contexts: ["video", "audio", "image"] });
  });
  ping(true);
});
chrome.runtime.onStartup.addListener(() => ping(true));

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const url = info.menuItemId === "velox-link" ? info.linkUrl : info.srcUrl;
  if (!url || !/^https?:/i.test(url)) return;
  const r = await handoff({ url, referrer: info.pageUrl || tab?.url, kind: classify(url, "") === "hls" ? "hls" : null });
  if (!r.ok) notifyError(r.error);
});

function notifyError(msg) {
  console.warn("[Velox]", msg);

}

// ─────────────────────────── messages from popup / content ───────────────────────────

chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  (async () => {
    switch (msg?.type) {
      case "alt-click":
        bypassUntil = Date.now() + 3000;
        reply({ ok: true });
        break;
      case "state":
        await ping(!!msg.force);
        reply({ ...state, paused: await isPaused() });
        break;
      case "set-paused":
        await chrome.storage.local.set({ paused: !!msg.paused });
        reply({ ok: true });
        break;
      case "media": {
        const tabId = msg.tabId ?? sender.tab?.id;
        const tab = tabId != null ? await chrome.tabs.get(tabId).catch(() => null) : null;
        // Tabs opened before the extension loaded never fired onUpdated.
        if (tab && isSiteUrl(tab.url) && !(media.get(tabId) || []).some((m) => m.kind === "site" && m.url === tab.url)) {
          syncSiteItem(tabId, tab.url, tab.title);
        }
        const items = media.get(tabId) || [];
        // The pill in an embedded player asks for its own streams first.
        const frameId = msg.tabId == null ? sender.frameId : undefined;
        const mine = frameId ? items.filter((m) => m.frameId === frameId || m.kind === "site") : [];
        reply({ items: mine.length ? mine : items });
        break;
      }
      case "download": {
        const tabId = msg.tabId ?? sender.tab?.id;
        // Replay the Referer / Origin the player itself used for this stream.
        const seen = (tabId != null ? media.get(tabId) || [] : []).find((m) => m.url === msg.url || m.key === msg.url);
        let referrer = seen?.referer || msg.referrer || (seen?.initiator ? seen.initiator + "/" : "");
        if (!referrer && tabId != null) referrer = (await chrome.tabs.get(tabId).catch(() => null))?.url;
        const headers = seen?.origin ? [["Origin", seen.origin]] : [];
        reply(await handoff({ url: msg.url, fileName: msg.fileName, referrer, headers, kind: msg.kind, size: msg.size, silent: msg.silent }));
        break;
      }
      case "prefetch":
        prefetch(msg.url);
        reply({ ok: true });
        break;
      case "show":
        reply(await send({ type: "show" }));
        break;
      default:
        reply({ ok: false });
    }
  })();
  return true;
});
