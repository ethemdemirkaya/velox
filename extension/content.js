// Velox content script: Alt+click bypass and the floating "Download video" pill.
(() => {
  if (window.__velox) return;
  window.__velox = true;

  // Alt+click → let the browser handle this download.
  window.addEventListener(
    "click",
    (e) => {
      if (e.altKey) chrome.runtime.sendMessage({ type: "alt-click" }).catch?.(() => {});
    },
    true,
  );

  // ── Video pill ──────────────────────────────────────────────
  let host, root, pill, menu, current, hideTimer;

  const fmt = (n) => {
    if (!n) return "";
    const u = ["B", "KB", "MB", "GB"];
    let i = 0;
    while (n >= 1024 && i < 3) (n /= 1024), i++;
    return `${n.toFixed(n >= 100 ? 0 : 1)} ${u[i]}`;
  };

  function ensure() {
    if (host) return;
    host = document.createElement("velox-pill");
    host.style.cssText = "position:fixed;z-index:2147483647;top:0;left:0;pointer-events:none;";
    root = host.attachShadow({ mode: "closed" });
    root.innerHTML = `
      <style>
        :host { all: initial; }
        .p { pointer-events:auto; display:flex; align-items:center; gap:7px; height:28px; padding:0 11px 0 9px;
             background:#0a0a0a; color:#f2f2f2; border:1px solid #2a2a2a; border-radius:6px;
             font:500 12px/1 "Inter","Segoe UI",system-ui,sans-serif; cursor:pointer; user-select:none;
             transition:background .12s, border-color .12s, opacity .15s; opacity:0; }
        .p.show { opacity:1; }
        .p:hover { background:#161616; border-color:#3a3a3a; }
        .m { display:block; width:12px; height:12px; }
        .m { pointer-events:none; }
        .menu { pointer-events:auto; position:absolute; top:34px; right:0; min-width:260px; max-width:380px;
                background:#0f0f0f; border:1px solid #2a2a2a; border-radius:8px; padding:4px; display:none;
                box-shadow:0 16px 40px rgba(0,0,0,.5); font:12px/1.4 "Inter","Segoe UI",system-ui,sans-serif; color:#f2f2f2; }
        .menu.open { display:block; }
        .it { display:flex; gap:8px; align-items:center; padding:8px 9px; border-radius:5px; cursor:pointer; }
        .it:hover { background:#1f1f1f; }
        .k { font:600 9.5px/1 ui-monospace,Consolas,monospace; padding:3px 5px; border:1px solid #333; border-radius:3px; color:#a1a1a1; }
        .n { flex:1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:#d4d4d4; }
        .s { color:#6b6b6b; font-variant-numeric:tabular-nums; }
        .ok { color:#f2f2f2; }
      </style>
      <div class="wrap" style="position:relative">
        <div class="p"><svg class="m" viewBox="0 0 24 24"><path d="M3 3h4.1L12 12.4 16.9 3H21L12 18.6z" fill="#f2f2f2"/><rect x="10.6" y="19.6" width="2.8" height="2.8" fill="#ff3b30"/></svg><span class="t">Download</span></div>
        <div class="menu"></div>
      </div>`;
    pill = root.querySelector(".p");
    menu = root.querySelector(".menu");
    pill.addEventListener("mouseenter", () => clearTimeout(hideTimer));
    menu.addEventListener("mouseenter", () => clearTimeout(hideTimer));
    pill.addEventListener("mouseleave", scheduleHide);
    menu.addEventListener("mouseleave", scheduleHide);
    pill.addEventListener("click", onPill);
    (document.documentElement || document.body).appendChild(host);
  }

  function scheduleHide() {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      pill?.classList.remove("show");
      menu?.classList.remove("open");
      current = null;
    }, 1200);
  }

  async function items() {
    try {
      const r = await chrome.runtime.sendMessage({ type: "media" });
      return r?.items || [];
    } catch {
      return [];
    }
  }

  function place(video) {
    const r = video.getBoundingClientRect();
    if (r.width < 200 || r.height < 120) return false;
    host.style.transform = `translate(${Math.max(8, r.right - 130)}px, ${Math.max(8, r.top + 10)}px)`;
    return true;
  }

  function label(m) {
    try {
      const u = new URL(m.url);
      return decodeURIComponent(u.pathname.split("/").pop() || u.host);
    } catch {
      return m.url;
    }
  }

  async function download(m) {
    const title = document.title.replace(/[\\/:*?"<>|]+/g, " ").trim().slice(0, 120);
    const fileName = (m.kind === "hls" || m.kind === "dash") && title ? `${title}.mp4` : null;
    const r = await chrome.runtime.sendMessage({ type: "download", url: m.url, kind: m.kind, fileName, size: m.size, referrer: location.href });
    root.querySelector(".t").textContent = r?.ok ? "Sent to Velox" : "Velox not running";
    setTimeout(() => (root.querySelector(".t").textContent = "Download"), 2000);
    menu.classList.remove("open");
  }

  async function onPill(e) {
    e.preventDefault();
    e.stopPropagation();
    const list = await items();
    if (!list.length && current?.currentSrc && /^https?:/.test(current.currentSrc)) {
      return download({ url: current.currentSrc, kind: "http" });
    }
    if (list.length === 1) return download(list[0]);
    menu.innerHTML = "";
    for (const m of list) {
      const el = document.createElement("div");
      el.className = "it";
      el.innerHTML = `<span class="k"></span><span class="n"></span><span class="s"></span>`;
      el.querySelector(".k").textContent = m.kind === "site" ? "VIDEO" : m.kind.toUpperCase();
      el.querySelector(".n").textContent = m.kind === "site" ? m.title || label(m) : label(m);
      el.querySelector(".s").textContent = fmt(m.size);
      el.addEventListener("click", () => download(m));
      menu.appendChild(el);
    }
    menu.classList.toggle("open");
  }

  let lastCheck = 0;
  document.addEventListener(
    "mouseover",
    async (e) => {
      const v = e.target instanceof Element ? e.target.closest("video") : null;
      if (!v || v === current) return;
      if (Date.now() - lastCheck < 150) return;
      lastCheck = Date.now();
      const list = await items();
      const direct = v.currentSrc && /^https?:/.test(v.currentSrc);
      if (!list.length && !direct) return;
      ensure();
      if (!place(v)) return;
      current = v;
      clearTimeout(hideTimer);
      pill.classList.add("show");
      scheduleHide();
    },
    { passive: true, capture: true },
  );
  window.addEventListener("scroll", () => current && place(current), { passive: true });
})();
