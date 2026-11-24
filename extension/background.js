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

function send(msg) {
  return new Promise((resolve) => {
    try {
      chrome.runtime.sendNativeMessage(HOST, msg, (resp) => {
        if (chrome.runtime.lastError) resolve({ ok: false, error: chrome.runtime.lastError.message });
        else resolve(resp ?? { ok: false, error: "empty response" });
      });
    } catch (e) {
      resolve({ ok: false, error: String(e) });
    }
  });
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

async function handoff({ url, fileName, referrer, kind, size, silent }) {
  const cookies = await cookiesFor(url);
  const r = await send({
    type: "add",
    url,
    fileName: fileName || null,
    referrer: referrer || null,
    cookies: cookies || null,
    userAgent: navigator.userAgent,
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
    if (d.tabId < 0 || d.statusCode >= 400) return;
    const mime = header(d.responseHeaders, "content-type");
    const kind = classify(d.url, mime);
    if (!kind) return;
    const size = +header(d.responseHeaders, "content-length") || 0;
    // Skip tiny media (thumbnails, beeps) and the many segment chunks of streams.
    if (kind === "http" && (size < 1024 * 1024 || /\.(ts|m4s)(\?|$)/i.test(d.url))) return;
    const range = header(d.responseHeaders, "content-range");
    const total = range ? +range.split("/")[1] || size : size;
    let list = media.get(d.tabId) || [];
    const key = d.url.split("#")[0];
    // Range requests of the same file show up many times – keep one entry.
    const base = kind === "http" ? key.replace(/[?&](range|bytes)=[^&]*/gi, "") : key;
    if (list.some((m) => m.key === base)) return;
    list = [{ key: base, url: d.url, kind, mime, size: total, ts: Date.now() }, ...list].slice(0, 25);
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

chrome.tabs.onUpdated.addListener((tabId, info) => {
  if (info.status === "loading" && info.url) {
    media.delete(tabId);
    persistMedia();
    updateBadge(tabId);
  }
});
chrome.tabs.onRemoved.addListener((tabId) => {
  media.delete(tabId);
  persistMedia();
});
chrome.tabs.onActivated.addListener(({ tabId }) => updateBadge(tabId));

function updateBadge(tabId) {
  const apply = (id) => {
    if (id == null) return;
    if (!state.connected && state.lastPing) {
      chrome.action.setBadgeText({ tabId: id, text: "!" });
      chrome.action.setBadgeBackgroundColor({ tabId: id, color: "#3a3a3a" });
      return;
    }
    const n = media.get(id)?.length || 0;
    chrome.action.setBadgeText({ tabId: id, text: n ? String(n) : "" });
    chrome.action.setBadgeBackgroundColor({ tabId: id, color: "#FF3B30" });
    chrome.action.setBadgeTextColor?.({ tabId: id, color: "#FFFFFF" });
  };
  if (tabId != null) apply(tabId);
  else chrome.tabs.query({ active: true }).then((tabs) => tabs.forEach((t) => apply(t.id)));
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
  chrome.action.setBadgeText({ text: "!" });
  chrome.action.setBadgeBackgroundColor({ color: "#FF3B30" });
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
        reply({ items: media.get(tabId) || [] });
        break;
      }
      case "download": {
        const tabId = msg.tabId ?? sender.tab?.id;
        let referrer = msg.referrer;
        if (!referrer && tabId != null) referrer = (await chrome.tabs.get(tabId).catch(() => null))?.url;
        reply(await handoff({ url: msg.url, fileName: msg.fileName, referrer, kind: msg.kind, size: msg.size, silent: msg.silent }));
        break;
      }
      case "show":
        reply(await send({ type: "show" }));
        break;
      default:
        reply({ ok: false });
    }
  })();
  return true;
});
