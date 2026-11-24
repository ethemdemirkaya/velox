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
    ? { dl: "Bu videoyu indir", sent: "Velox'a gönderildi", off: "Velox çalışmıyor", wait: "Gönderiliyor…" }
    : { dl: "Download this video", sent: "Sent to Velox", off: "Velox is not running", wait: "Sending…" };

  let host, root, pill, label, menu, current, busy = false;

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
        .x { width:26px; height:100%; border:0; border-left:1px solid #2a2a2a; background:none; color:#7a7a7a; cursor:pointer;
             font:14px/1 system-ui; }
        .x:hover { color:#fff; background:#ff3b30; }
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
          <button class="x" title="✕">×</button>
        </div>
        <div class="menu"></div>
      </div>`;
    pill = root.querySelector(".go");
    label = root.querySelector(".t");
    menu = root.querySelector(".menu");
    label.textContent = L.dl;
    pill.addEventListener("click", onClick);
    root.querySelector(".x").addEventListener("click", (e) => {
      e.stopPropagation();
      if (current) dismissed.add(current);
      hide();
    });
    for (const ev of ["mousedown", "mouseup", "pointerdown", "dblclick"]) host.addEventListener(ev, (e) => e.stopPropagation());
    mount();
  }

  function mount() {
    const parent = document.fullscreenElement || document.documentElement;
    if (host && host.parentNode !== parent) parent.appendChild(host);
  }

  function hide() {
    if (host) host.style.display = "none";
    menu?.classList.remove("open");
    current = null;
  }

  function place() {
    if (!current || !host) return;
    const r = current.getBoundingClientRect();
    const visible = r.width >= MIN_W && r.height >= MIN_H && r.bottom > 40 && r.top < innerHeight - 40 && r.right > 0 && r.left < innerWidth;
    if (!visible || !current.isConnected) return hide();
    const w = host.offsetWidth || 180;
    const x = Math.min(innerWidth - w - 8, Math.max(8, r.right - w - 12));
    const y = Math.min(innerHeight - 40, Math.max(8, r.top + 12));
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
    if (refreshing || busy) return;
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
