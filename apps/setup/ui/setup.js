const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const win = window.__TAURI__.window.getCurrentWindow();

const T = {
  en: {
    setup: "Velox Setup",
    tagline: "Fast downloads. Built in Rust.",
    desktop: "Desktop shortcut",
    extension: "Prepare the browser extension",
    location: "Location",
    install: "Install",
    update: "Update",
    reinstall: "Reinstall",
    installed: "Version {v} is installed – it will be updated.",
    same: "Velox {v} is already installed.",
    license: "Open source · MIT OR Apache-2.0",
    closing: "Closing Velox…",
    extracting: "Unpacking…",
    copying: "Copying files…",
    extension_step: "Preparing the extension…",
    shortcuts: "Creating shortcuts…",
    registering: "Registering…",
    done: "Done",
    ready: "Velox is ready",
    launch: "Launch Velox",
    close: "Close",
    extTitle: "Add the browser extension",
    ext1: "Open chrome://extensions (brave:// or edge://) and turn on Developer mode",
    ext2: "Click “Load unpacked” and choose this folder:",
    copy: "Copy",
    copied: "Copied",
    open: "Open",
    unTitle: "Remove Velox from this computer?",
    unData: "Also delete settings and download history",
    unKeep: "Downloaded files are never touched.",
    remove: "Remove",
    cancel: "Cancel",
    removing: "Removing…",
    removed: "Velox was removed",
    data: "Cleaning up…",
    failed: "Setup failed",
    nopayload: "This build has no Velox payload. Build the desktop app in release mode first.",
  },
  tr: {
    setup: "Velox Kurulumu",
    tagline: "Hızlı indirmeler. Rust ile yazıldı.",
    desktop: "Masaüstü kısayolu",
    extension: "Tarayıcı eklentisini hazırla",
    location: "Konum",
    install: "Kur",
    update: "Güncelle",
    reinstall: "Yeniden kur",
    installed: "{v} sürümü kurulu – güncellenecek.",
    same: "Velox {v} zaten kurulu.",
    license: "Açık kaynak · MIT OR Apache-2.0",
    closing: "Velox kapatılıyor…",
    extracting: "Açılıyor…",
    copying: "Dosyalar kopyalanıyor…",
    extension_step: "Eklenti hazırlanıyor…",
    shortcuts: "Kısayollar oluşturuluyor…",
    registering: "Kaydediliyor…",
    done: "Tamamlandı",
    ready: "Velox hazır",
    launch: "Velox'u başlat",
    close: "Kapat",
    extTitle: "Tarayıcı eklentisini ekle",
    ext1: "chrome://extensions (brave:// veya edge://) sayfasını açıp Geliştirici modunu aç",
    ext2: "“Paketlenmemiş öğe yükle”ye tıklayıp bu klasörü seç:",
    copy: "Kopyala",
    copied: "Kopyalandı",
    open: "Aç",
    unTitle: "Velox bu bilgisayardan kaldırılsın mı?",
    unData: "Ayarları ve indirme geçmişini de sil",
    unKeep: "İndirdiğin dosyalara asla dokunulmaz.",
    remove: "Kaldır",
    cancel: "İptal",
    removing: "Kaldırılıyor…",
    removed: "Velox kaldırıldı",
    data: "Temizleniyor…",
    failed: "Kurulum başarısız",
    nopayload: "Bu derlemede Velox dosyaları yok. Önce masaüstü uygulamasını release modunda derleyin.",
  },
};

const $ = (s) => document.querySelector(s);
let L = T.en;
let info;
const t = (k, vars = {}) => (L[k] ?? T.en[k] ?? k).replace(/\{(\w+)\}/g, (_, n) => vars[n] ?? "");

function show(id) {
  for (const s of document.querySelectorAll(".screen")) s.hidden = s.id !== id;
}
function buttons(primary, secondary, danger = false) {
  const p = $("#primary");
  const s = $("#secondary");
  p.textContent = primary.label;
  p.onclick = primary.run;
  p.disabled = !!primary.disabled;
  p.className = "btn " + (danger ? "danger" : "primary");
  p.hidden = !primary.label;
  s.hidden = !secondary;
  if (secondary) {
    s.textContent = secondary.label;
    s.onclick = secondary.run;
  }
}

const stepKey = { extension: "extension_step" };

async function runInstall() {
  show("s-progress");
  buttons({ label: t("install"), disabled: true });
  $("#foot").textContent = "";
  try {
    await invoke("install", { opts: { desktop: $("#o-desktop").checked, extension: $("#o-ext").checked } });
    show("s-done");
    $("#ext-help").hidden = !$("#o-ext").checked;
    buttons({ label: t("launch"), run: async () => (await invoke("launch"), invoke("quit")) }, { label: t("close"), run: () => invoke("quit") });
  } catch (e) {
    fail(e);
  }
}

async function runUninstall() {
  show("s-progress");
  buttons({ label: t("remove"), disabled: true }, null, true);
  try {
    await invoke("uninstall", { removeData: $("#o-data").checked });
    $("#step").textContent = t("removed");
    buttons({ label: t("close"), run: () => invoke("quit") });
  } catch (e) {
    fail(e);
  }
}

function fail(e) {
  show("s-error");
  $("#err").textContent = `${t("failed")}: ${e}`;
  buttons({ label: t("close"), run: () => invoke("quit") });
}

listen("progress", ({ payload }) => {
  $("#fill").style.width = payload.pct + "%";
  $("#pct").textContent = payload.pct + "%";
  $("#step").textContent = t(stepKey[payload.step] ?? payload.step);
});

$("#close").onclick = () => invoke("quit");
$("#copy").onclick = async () => {
  await navigator.clipboard.writeText(info.extensionDir).catch(() => {});
  $("#copy").textContent = t("copied");
  setTimeout(() => ($("#copy").textContent = t("copy")), 1500);
};
$("#openext").onclick = () => invoke("open_path", { path: info.extensionDir });

(async () => {
  info = await invoke("info");
  L = T[info.lang] ?? T.en;
  document.documentElement.lang = info.lang;
  for (const el of document.querySelectorAll("[data-t]")) el.textContent = t(el.dataset.t);
  $("#ver").textContent = info.version;
  $("#dir").textContent = info.dir;
  $("#extdir").textContent = info.extensionDir;
  $("#foot").textContent = t("license");
  document.title = t("setup");

  if (info.uninstall) {
    show("s-uninstall");
    buttons({ label: t("remove"), run: runUninstall }, { label: t("cancel"), run: () => invoke("quit") }, true);
    return;
  }
  show("s-install");
  if (!info.payload) {
    $("#note").textContent = t("nopayload");
    buttons({ label: t("install"), disabled: true });
    return;
  }
  if (info.installedVersion)
    $("#note").textContent = t(info.installedVersion === info.version ? "same" : "installed", { v: info.installedVersion });
  const label = !info.installedVersion ? t("install") : info.installedVersion === info.version ? t("reinstall") : t("update");
  buttons({ label, run: runInstall });
})();
