const tr = {
  checking: "Kontrol ediliyor…",
  connected: "Bağlı",
  offline: "Bağlı değil",
  offTitle: "Velox uygulaması bulunamadı",
  offBody: "Tarayıcı köprüsünün kaydedilmesi için Velox masaüstü uygulamasını kurup bir kez açın.",
  get: "Velox'u indir",
  capture: "İndirmeleri yakala",
  altHint: "Atlamak için tıklarken Alt'a basılı tutun",
  media: "Bu sayfadaki medya",
  none: "Henüz video veya ses akışı bulunamadı. Bir video oynatmayı deneyin.",
  open: "Velox'u aç",
  dl: "İndir",
  sent: "Gönderildi",
};
const en = {
  checking: "Checking…",
  connected: "Connected",
  offline: "Not connected",
  dl: "Download",
  sent: "Sent",
};
const L = navigator.language.toLowerCase().startsWith("tr") ? tr : en;
const $ = (s) => document.querySelector(s);
const msg = (m) => chrome.runtime.sendMessage(m);

for (const el of document.querySelectorAll("[data-i18n]")) {
  const v = L[el.dataset.i18n];
  if (v) el.textContent = v;
}

function fmt(n) {
  if (!n) return "";
  const u = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (n >= 1024 && i < 3) (n /= 1024), i++;
  return `${n.toFixed(n >= 100 ? 0 : 1)} ${u[i]}`;
}

function name(url) {
  try {
    const u = new URL(url);
    return decodeURIComponent(u.pathname.split("/").filter(Boolean).pop() || u.host);
  } catch {
    return url;
  }
}

async function render() {
  const s = await msg({ type: "state", force: true });
  const status = $("#status");
  status.classList.toggle("ok", s.connected);
  status.classList.toggle("bad", !s.connected);
  status.querySelector("span").textContent = s.connected ? L.connected : L.offline;
  $("#offline").hidden = s.connected;
  $("#ver").textContent = s.version ? `v${s.version}` : "";
  $("#toggle").setAttribute("aria-checked", String(!s.paused));

  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  const { items } = await msg({ type: "media", tabId: tab?.id });
  const list = $("#list");
  list.innerHTML = "";
  $("#empty").hidden = items.length > 0;
  $("#count").textContent = items.length ? String(items.length) : "";
  for (const m of items) {
    const el = document.createElement("div");
    el.className = "item";
    el.innerHTML = `<span class="k"></span><div class="n"><div class="f"></div><div class="s"></div></div><button class="btn sm"></button>`;
    el.querySelector(".k").textContent = m.kind.toUpperCase();
    el.querySelector(".f").textContent = name(m.url);
    el.querySelector(".f").title = m.url;
    el.querySelector(".s").textContent = [fmt(m.size), m.title].filter(Boolean).join(" · ");
    const b = el.querySelector("button");
    b.textContent = L.dl;
    b.disabled = !s.connected;
    b.addEventListener("click", async () => {
      const title = (tab?.title || "").replace(/[\\/:*?"<>|]+/g, " ").trim().slice(0, 120);
      const fileName = m.kind !== "http" && title ? `${title}.mp4` : null;
      const r = await msg({ type: "download", tabId: tab?.id, url: m.url, kind: m.kind, size: m.size, fileName, referrer: tab?.url });
      b.textContent = r?.ok ? L.sent : "!";
      b.classList.add("done");
    });
    list.appendChild(el);
  }
}

$("#toggle").addEventListener("click", async (e) => {
  const on = e.currentTarget.getAttribute("aria-checked") !== "true";
  e.currentTarget.setAttribute("aria-checked", String(on));
  await msg({ type: "set-paused", paused: !on });
});
$("#open").addEventListener("click", async () => {
  await msg({ type: "show" });
  window.close();
});

render();
