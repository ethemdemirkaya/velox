// Velox content script
// * Alt+click → let the browser handle the download.
// * IDM-style "Download this video" button pinned to the top-right corner of
//   the main video on the page (follows scroll, resize and fullscreen).
(() => {
  if (window.__velox) return;
  window.__velox = true;

  window.addEventListener(
    "click",
    (e) => {
      if (e.altKey) chrome.runtime.sendMessage({ type: "alt-click" }).catch?.(() => {});
    },
    true,
  );

  const MIN_W = 220;
  const MIN_H = 124;
  const dismissed = new WeakSet();
  const L = navigator.language.toLowerCase().startsWith("tr")
    ? {
        dl: "Bu videoyu indir", sent: "Velox'a gönderildi", off: "Velox çalışmıyor", wait: "Gönderiliyor…",
        help: [["Tıkla", "Videoyu Velox ile indir"], ["Sürükle", "Düğmeyi istediğin yere taşı"], ["×", "Bu video için gizle"], ["Alt + tık", "Bağlantıyı tarayıcı indirsin"]],
        open: "Velox'u aç", hideSite: "Bu sitede gösterme", reset: "Konumu sıfırla",
      }
    : {
        dl: "Download this video", sent: "Sent to Velox", off: "Velox is not running", wait: "Sending…",
        help: [["Click", "Download the video with Velox"], ["Drag", "Move this button anywhere"], ["×", "Hide for this video"], ["Alt + click", "Let the browser download a link"]],
        open: "Open Velox", hideSite: "Don't show on this site", reset: "Reset position",
      };
  const HOST = location.hostname.replace(/^www\./, "");
  const KEY_OFF = "pillOffset:" + HOST;
  const KEY_HIDE = "pillHidden:" + HOST;
  let offset = { dx: 0, dy: 0 };
  let hiddenOnSite = false;
  chrome.storage?.local.get([KEY_OFF, KEY_HIDE]).then((v) => {
    if (v[KEY_OFF]) offset = v[KEY_OFF];
    hiddenOnSite = !!v[KEY_HIDE];
  }).catch(() => {});

  let host, root, pill, label, menu, info, current, busy = false;

  const fmt = (n) => {
    if (!n) return "";
    const u = ["B", "KB", "MB", "GB"];
    let i = 0;
    while (n >= 1024 && i < 3) (n /= 1024), i++;
    return `${n.toFixed(n >= 100 ? 0 : 1)} ${u[i]}`;
  };

  // ── Which page URL represents this particular video? ─────────────────────
  // Timelines (X, Reddit, Instagram feed) contain many videos; each one lives
  // inside a post that links to its own permalink.
  function postUrlFor(video) {
    const h = location.hostname.replace(/^(www|m|mobile)\./, "");
    if (h === "youtube.com") {
      // Watch / Shorts pages are the video itself.
      if (/^\/(watch|shorts\/|live\/)/.test(location.pathname)) return location.href;
      // Home/search/channel hover previews and feed items link to their video.
      const box = video.closest("ytd-video-preview, ytd-rich-item-renderer, ytd-video-renderer, ytd-reel-item-renderer, ytd-compact-video-renderer");
      const a = box?.querySelector('a[href*="/watch"], a[href*="/shorts/"]');
      return a ? new URL(a.getAttribute("href"), location.origin).href : null;
    }
    if (h === "x.com" || h === "twitter.com") {
      const art = video.closest("article");
      const a = art && ([...art.querySelectorAll('a[href*="/status/"]')].find((x) => x.querySelector("time")) || art.querySelector('a[href*="/status/"]'));
      if (a) return new URL(a.getAttribute("href").replace(/\/(photo|video|analytics)\/.*$/, ""), location.origin).href;
      return /\/status\//.test(location.pathname) ? location.href : null;
    }
    if (h === "instagram.com") {
      const art = video.closest("article") || document;
      const a = art.querySelector('a[href^="/p/"], a[href^="/reel/"]');
      if (/^\/(p|reels?|tv)\//.test(location.pathname)) return location.href;
      return a ? new URL(a.getAttribute("href"), location.origin).href : null;
    }
    if (h === "reddit.com") {
      const post = video.closest("shreddit-post, article, [data-testid='post-container']");
      const perm = post?.getAttribute?.("permalink") || post?.querySelector('a[href*="/comments/"]')?.getAttribute("href");
      if (perm) return new URL(perm, location.origin).href;
    }
    return null;
  }

  async function targetsFor(video) {
    const post = postUrlFor(video);
    if (post) return [{ kind: "site", url: post, title: "" }];
    let items = [];
    try {
      items = (await chrome.runtime.sendMessage({ type: "media" }))?.items || [];
    } catch {}
    if (items.length) return items;
    const src = video.currentSrc || video.src;
    if (src && /^https?:/.test(src)) return [{ kind: "http", url: src }];
    return [];
  }

  // ── UI (closed shadow root so pages can't restyle it) ────────────────────
  function ensure() {
    if (host) return;
    host = document.createElement("velox-download");
    host.style.cssText = "all:initial;position:fixed;z-index:2147483647;top:0;left:0;display:none;";
    root = host.attachShadow({ mode: "closed" });
    root.innerHTML = `
      <style>
        * { box-sizing: border-box; }
        .wrap { position: relative; font: 500 12px/1 "Inter","Segoe UI",system-ui,sans-serif; color: #f2f2f2; }
        .p { display:flex; align-items:center; height:30px; background:rgba(10,10,10,.92); border:1px solid #2e2e2e;
             border-radius:7px; overflow:hidden; box-shadow:0 6px 20px rgba(0,0,0,.45); backdrop-filter: blur(6px); }
        .go { display:flex; align-items:center; gap:8px; height:100%; padding:0 12px 0 10px; cursor:pointer; border:0;
              background:none; color:inherit; font:inherit; white-space:nowrap; }
        .go:hover { background:#1c1c1c; }
        .p { cursor: grab; touch-action: none; user-select: none; }
        .p.dragging { cursor: grabbing; opacity: .85; }
        .x, .q { width:26px; height:100%; border:0; border-left:1px solid #2a2a2a; background:none; color:#7a7a7a; cursor:pointer;
             font:600 12px/1 system-ui; }
        .x { font-size:14px; font-weight:400; }
        .q:hover, .q.on { color:#fff; background:#1f1f1f; }
        .x:hover { color:#fff; background:#ff3b30; }
        .info { position:absolute; top:36px; right:0; width:280px; background:#0f0f0f; border:1px solid #2a2a2a; border-radius:8px;
                padding:12px; display:none; box-shadow:0 16px 40px rgba(0,0,0,.5); font-weight:400; cursor:default; }
        .info.open { display:block; }
        .info h4 { margin:0 0 10px; display:flex; align-items:center; gap:8px; font:600 12.5px/1 "Inter","Segoe UI",system-ui,sans-serif; }
        .row { display:grid; grid-template-columns:78px 1fr; gap:8px; padding:4px 0; font-size:11.5px; line-height:1.35; }
        .row b { font:600 10.5px/1.6 ui-monospace,Consolas,monospace; color:#d4d4d4; }
        .row span { color:#a1a1a1; }
        .acts { display:flex; flex-wrap:wrap; gap:6px; margin-top:10px; padding-top:10px; border-top:1px solid #222; }
        .acts button { height:24px; padding:0 9px; border-radius:5px; border:1px solid #2e2e2e; background:#161616; color:#e6e6e6;
                       font:500 11px/1 "Inter","Segoe UI",system-ui,sans-serif; cursor:pointer; }
        .acts button:hover { background:#222; }
        .acts .primary { background:#f2f2f2; color:#0a0a0a; border-color:#f2f2f2; }
        svg { display:block; flex:none; }
        .menu { position:absolute; top:36px; right:0; min-width:270px; max-width:400px; background:#0f0f0f; border:1px solid #2a2a2a;
                border-radius:8px; padding:4px; display:none; box-shadow:0 16px 40px rgba(0,0,0,.5); font-weight:400; }
        .menu.open { display:block; }
        .it { display:flex; gap:8px; align-items:center; padding:8px 9px; border-radius:5px; cursor:pointer; }
        .it:hover { background:#1f1f1f; }
        .k { font:600 9.5px/1 ui-monospace,Consolas,monospace; padding:3px 5px; border:1px solid #333; border-radius:3px; color:#a1a1a1; }
        .n { flex:1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:#d4d4d4; }
        .s { color:#6b6b6b; font-variant-numeric:tabular-nums; }
      </style>
      <div class="wrap">
        <div class="p">
          <button class="go" title="Velox">
            <svg width="13" height="13" viewBox="0 0 24 24"><path d="M3 3h4.1L12 12.4 16.9 3H21L12 18.6z" fill="#f2f2f2"/><rect x="10.6" y="19.6" width="2.8" height="2.8" fill="#ff3b30"/></svg>
            <span class="t"></span>
          </button>
          <button class="q" title="?">?</button>
          <button class="x" title="✕">×</button>
        </div>
        <div class="menu"></div>
        <div class="info"></div>
      </div>`;
    pill = root.querySelector(".go");
    label = root.querySelector(".t");
    menu = root.querySelector(".menu");
    info = root.querySelector(".info");
    label.textContent = L.dl;
    buildInfo();
    pill.addEventListener("click", (e) => {
      if (dragMoved) {
        e.preventDefault();
        e.stopPropagation();
        return;
      }
      onClick(e);
    });
    const q = root.querySelector(".q");
    q.addEventListener("click", (e) => {
      e.stopPropagation();
      menu.classList.remove("open");
      q.classList.toggle("on", info.classList.toggle("open"));
    });
    enableDrag(root.querySelector(".p"));
    root.querySelector(".x").addEventListener("click", (e) => {
      e.stopPropagation();
      if (current) dismissed.add(current);
      hide();
    });
    for (const ev of ["mousedown", "mouseup", "pointerdown", "dblclick"]) host.addEventListener(ev, (e) => e.stopPropagation());
    mount();
  }

  function buildInfo() {
    info.innerHTML = `<h4><svg width="12" height="12" viewBox="0 0 24 24"><path d="M3 3h4.1L12 12.4 16.9 3H21L12 18.6z" fill="#f2f2f2"/><rect x="10.6" y="19.6" width="2.8" height="2.8" fill="#ff3b30"/></svg>Velox</h4>`;
    for (const [k, v] of L.help) {
      const r = document.createElement("div");
      r.className = "row";
      r.innerHTML = "<b></b><span></span>";
      r.querySelector("b").textContent = k;
      r.querySelector("span").textContent = v;
      info.appendChild(r);
    }
    const acts = document.createElement("div");
    acts.className = "acts";
    const mk = (text, cls, fn) => {
      const b = document.createElement("button");
      b.textContent = text;
      if (cls) b.className = cls;
      b.addEventListener("click", (e) => {
        e.stopPropagation();
        fn();
      });
      acts.appendChild(b);
    };
    mk(L.open, "primary", () => chrome.runtime.sendMessage({ type: "show" }).catch(() => {}));
    mk(L.reset, "", () => {
      offset = { dx: 0, dy: 0 };
      chrome.storage?.local.remove(KEY_OFF);
      place();
    });
    mk(L.hideSite, "", () => {
      hiddenOnSite = true;
      chrome.storage?.local.set({ [KEY_HIDE]: true });
      hide();
    });
    info.appendChild(acts);
  }

  // Drag the button anywhere; the offset (relative to the video's corner) is
  // remembered per site. A press that doesn't move is a normal click.
  let dragMoved = false;
  function enableDrag(handle) {
    let sx = 0, sy = 0, ox = 0, oy = 0, down = false;
    handle.addEventListener("pointerdown", (e) => {
      if (e.button !== 0 || e.target.closest(".x, .q")) return;
      down = true;
      dragMoved = false;
      sx = e.clientX;
      sy = e.clientY;
      ox = offset.dx;
      oy = offset.dy;
      handle.setPointerCapture(e.pointerId);
    });
    handle.addEventListener("pointermove", (e) => {
      if (!down) return;
      const dx = e.clientX - sx, dy = e.clientY - sy;
      if (!dragMoved && Math.abs(dx) + Math.abs(dy) < 5) return;
      dragMoved = true;
      handle.classList.add("dragging");
      menu.classList.remove("open");
      offset = { dx: ox + dx, dy: oy + dy };
      place();
    });
    const end = () => {
      if (!down) return;
      down = false;
      handle.classList.remove("dragging");
      if (dragMoved) chrome.storage?.local.set({ [KEY_OFF]: offset });
      setTimeout(() => (dragMoved = false), 0);
    };
    handle.addEventListener("pointerup", end);
    handle.addEventListener("pointercancel", end);
  }

  function mount() {
    const parent = document.fullscreenElement || document.documentElement;
    if (host && host.parentNode !== parent) parent.appendChild(host);
  }

  function hide() {
    if (host) host.style.display = "none";
    menu?.classList.remove("open");
    info?.classList.remove("open");
    root?.querySelector(".q")?.classList.remove("on");
    current = null;
  }

  function place() {
    if (!current || !host) return;
    const r = current.getBoundingClientRect();
    const visible = r.width >= MIN_W && r.height >= MIN_H && r.bottom > 40 && r.top < innerHeight - 40 && r.right > 0 && r.left < innerWidth;
    if (!visible || !current.isConnected) return hide();
    const w = host.offsetWidth || 180;
    const x = Math.min(innerWidth - w - 8, Math.max(8, r.right - w - 12 + offset.dx));
    const y = Math.min(innerHeight - 40, Math.max(8, r.top + 12 + offset.dy));
    host.style.transform = `translate(${Math.round(x)}px, ${Math.round(y)}px)`;
  }

  // The "main" video: largest visible one (prefer the one that is playing).
  function pickVideo() {
    let best = null;
    let bestScore = 0;
    for (const v of document.querySelectorAll("video")) {
      if (dismissed.has(v)) continue;
      const r = v.getBoundingClientRect();
      if (r.width < MIN_W || r.height < MIN_H) continue;
      const vis = Math.max(0, Math.min(r.bottom, innerHeight) - Math.max(r.top, 0)) * Math.max(0, Math.min(r.right, innerWidth) - Math.max(r.left, 0));
      const score = vis * (v.paused ? 1 : 2);
      if (score > bestScore) (best = v), (bestScore = score);
    }
    return best;
  }

  let refreshing = false;
  async function refresh() {
    if (refreshing || busy || dragMoved) return;
    if (hiddenOnSite) return hide();
    refreshing = true;
    try {
      const v = pickVideo();
      if (!v) return hide();
      if (v !== current) {
        const t = await targetsFor(v);
        if (!t.length) return hide();
        ensure();
        mount();
        current = v;
        menu.classList.remove("open");
        host.style.display = "block";
      }
      place();
    } finally {
      refreshing = false;
    }
  }

  async function send(m) {
    busy = true;
    label.textContent = L.wait;
    const title = document.title.replace(/[\\/:*?"<>|]+/g, " ").replace(/ - YouTube$/, "").trim().slice(0, 120);
    const fileName = (m.kind === "hls" || m.kind === "dash") && title ? `${title}.mp4` : null;
    let r;
    try {
      r = await chrome.runtime.sendMessage({ type: "download", url: m.url, kind: m.kind, fileName, size: m.size, referrer: location.href });
    } catch {}
    label.textContent = r?.ok ? L.sent : L.off;
    menu.classList.remove("open");
    setTimeout(() => {
      label.textContent = L.dl;
      busy = false;
      place();
    }, 2200);
  }

  async function onClick(e) {
    e.preventDefault();
    e.stopPropagation();
    if (!current || busy) return;
    const list = await targetsFor(current);
    // A single site entry (YouTube, X post…) or single stream: go straight away.
    const site = list.find((m) => m.kind === "site");
    if (site) return send(site);
    if (list.length === 1) return send(list[0]);
    menu.innerHTML = "";
    for (const m of list) {
      const el = document.createElement("div");
      el.className = "it";
      el.innerHTML = `<span class="k"></span><span class="n"></span><span class="s"></span>`;
      el.querySelector(".k").textContent = m.kind.toUpperCase();
      let name = m.url;
      try {
        name = decodeURIComponent(new URL(m.url).pathname.split("/").pop() || m.url);
      } catch {}
      el.querySelector(".n").textContent = name;
      el.querySelector(".s").textContent = fmt(m.size);
      el.addEventListener("click", () => send(m));
      menu.appendChild(el);
    }
    menu.classList.toggle("open");
  }

  // Keep up with SPAs (YouTube, X) without hammering the page.
  let raf = 0;
  const schedule = () => {
    if (!raf) raf = requestAnimationFrame(() => ((raf = 0), place()));
  };
  addEventListener("scroll", schedule, { passive: true, capture: true });
  addEventListener("resize", schedule, { passive: true });
  document.addEventListener("fullscreenchange", () => {
    mount();
    setTimeout(place, 50);
  });
  document.addEventListener("play", () => refresh(), true);
  document.addEventListener("loadedmetadata", () => refresh(), true);
  setInterval(refresh, 800);
  if (window.top === window) refresh();
})();
