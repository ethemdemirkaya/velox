<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import Toggle from "./Toggle.svelte";
  import { api, onEngine, type AppInfo, type Settings, type ToolsStatus } from "../lib/api";
  import { toasts } from "../lib/toasts.svelte";
  import { store } from "../lib/store.svelte";
  import { t, type I18nKey } from "../lib/i18n.svelte";
  import { bytes, parseRate } from "../lib/format";

  type Tab = "general" | "downloads" | "browser" | "scheduler" | "about";
  const tabs: { id: Tab; icon: string; label: I18nKey }[] = [
    { id: "general", icon: "sliders", label: "settings.general" },
    { id: "downloads", icon: "down", label: "settings.downloads" },
    { id: "browser", icon: "puzzle", label: "settings.browser" },
    { id: "scheduler", icon: "calendar", label: "settings.scheduler" },
    { id: "about", icon: "info", label: "settings.about" },
  ];
  let tab = $state<Tab>("general");
  let info = $state<AppInfo | null>(null);
  let update = $state<"idle" | "checking" | "none" | string>("idle");
  let installing = $state(false);
  let tools = $state<ToolsStatus | null>(null);
  let toolsBusy = $state(false);

  async function installTools() {
    toolsBusy = true;
    try {
      tools = await api.installTools();
    } catch (e) {
      toasts.push({ kind: "error", title: t("toast.error"), body: String(e) });
    } finally {
      toolsBusy = false;
    }
  }

  const s = $derived(store.settings!);
  let limitText = $state("");
  let extText = $state("");

  $effect(() => {
    limitText = s?.speedLimit ? bytes(s.speedLimit) : "";
    extText = s?.captureExtensions.join(" ") ?? "";
  });

  onMount(() => {
    api.appInfo().then((i) => (info = i));
    api.toolsStatus().then((s) => (tools = s));
    const un = onEngine((e) => {
      if (e.type === "tools") tools = e.data;
    });
    return () => {
      un.then((f) => f());
    };
  });

  const save = (patch: Partial<Settings>) => store.saveSettings(patch);

  async function pickDir() {
    const p = await api.pickFolder();
    if (p) save({ downloadDir: p });
  }

  async function checkUpdate() {
    update = "checking";
    try {
      update = (await api.checkUpdate()) ?? "none";
    } catch {
      update = "none";
    }
  }
</script>

<div class="wrap">
  <nav class="tabs">
    <h1>{t("settings.title")}</h1>
    {#each tabs as x (x.id)}
      <button class="tab" class:on={tab === x.id} onclick={() => (tab = x.id)}>
        <Icon name={x.icon} size={15} />
        {t(x.label)}
      </button>
    {/each}
  </nav>

  {#if s}
    <div class="pane">
      {#key tab}
        <div class="sect">
          {#if tab === "general"}
            <div class="row">
              <div class="l"><div class="t">{t("settings.language")}</div></div>
              <div class="seg">
                {#each [["auto", t("settings.languageAuto")], ["tr", "Türkçe"], ["en", "English"]] as [v, label]}
                  <button class:on={s.language === v} onclick={() => save({ language: v })}>{label}</button>
                {/each}
              </div>
            </div>
            <div class="row">
              <div class="l">
                <div class="t">{t("settings.autostart")}</div>
                <div class="h">{t("settings.autostartHint")}</div>
              </div>
              <Toggle checked={s.autostart} onchange={(v) => save({ autostart: v })} />
            </div>
            <div class="row">
              <div class="l"><div class="t">{t("settings.closeToTray")}</div></div>
              <Toggle checked={s.closeToTray} onchange={(v) => save({ closeToTray: v })} />
            </div>
            <div class="row">
              <div class="l">
                <div class="t">{t("settings.progressWindow")}</div>
                <div class="h">{t("settings.progressWindowHint")}</div>
              </div>
              <Toggle checked={s.progressWindow} onchange={(v) => save({ progressWindow: v })} />
            </div>
            <div class="row">
              <div class="l">
                <div class="t">{t("settings.notifications")}</div>
                <div class="h">{t("settings.notificationsHint")}</div>
              </div>
              <Toggle checked={s.notifications} onchange={(v) => save({ notifications: v })} />
            </div>
            <div class="row">
              <div class="l">
                <div class="t">{t("settings.clipboard")}</div>
                <div class="h">{t("settings.clipboardHint")}</div>
              </div>
              <Toggle checked={s.clipboardWatch} onchange={(v) => save({ clipboardWatch: v })} />
            </div>
          {:else if tab === "downloads"}
            <div class="row col">
              <div class="l"><div class="t">{t("settings.dir")}</div></div>
              <div class="dir">
                <button class="value mono" onclick={() => api.openFolder(s.downloadDir)} title={s.downloadDir}>{s.downloadDir}</button>
                <button class="btn" onclick={pickDir}><Icon name="folder" size={14} />{t("action.change")}</button>
              </div>
            </div>
            <div class="row">
              <div class="l">
                <div class="t">{t("settings.categoryFolders")}</div>
                <div class="h">{t("settings.categoryFoldersHint")}</div>
              </div>
              <Toggle checked={s.useCategoryFolders} onchange={(v) => save({ useCategoryFolders: v })} />
            </div>
            <div class="row">
              <div class="l"><div class="t">{t("settings.concurrent")}</div></div>
              <div class="stepper">
                <button class="btn sm icon" onclick={() => save({ maxConcurrent: Math.max(1, s.maxConcurrent - 1) })}><Icon name="min" size={12} /></button>
                <span class="num">{s.maxConcurrent}</span>
                <button class="btn sm icon" onclick={() => save({ maxConcurrent: Math.min(20, s.maxConcurrent + 1) })}><Icon name="plus" size={12} /></button>
              </div>
            </div>
            <div class="row">
              <div class="l">
                <div class="t">{t("settings.speedLimit")}</div>
                <div class="h">{t("settings.speedLimitHint")}</div>
              </div>
              <input
                class="input short"
                placeholder={t("detail.unlimited")}
                bind:value={limitText}
                onchange={() => save({ speedLimit: parseRate(limitText) })}
              />
            </div>
            <div class="row col">
              <div class="l">
                <div class="t">{t("settings.proxy")}</div>
                <div class="h">{t("settings.proxyHint")}</div>
              </div>
              <input class="input mono" value={s.proxy} placeholder="socks5://127.0.0.1:1080" onchange={(e) => save({ proxy: e.currentTarget.value.trim() })} />
            </div>
            <div class="row col">
              <div class="l">
                <div class="t">{t("settings.userAgent")}</div>
                <div class="h">{t("settings.userAgentHint")}</div>
              </div>
              <input class="input mono" value={s.userAgent} placeholder="Mozilla/5.0 …" onchange={(e) => save({ userAgent: e.currentTarget.value })} />
            </div>
          {:else if tab === "browser"}
            <div class="card">
              <div class="card-h">
                <Icon name="puzzle" size={16} />
                <b>{t("settings.extension")}</b>
                {#if info}
                  <span class="pill" class:ok={info.hostRegistered}>
                    <i></i>{info.hostRegistered ? t("settings.hostOk") : t("settings.hostMissing")}
                  </span>
                {/if}
              </div>
              <p class="muted">{t("settings.extensionHint")}</p>
              {#if info}<p class="mono faint">ID: {info.extensionId}</p>{/if}
            </div>
            <div class="card">
              <div class="card-h">
                <Icon name="film" size={16} />
                <b>{t("settings.video")}</b>
                {#if tools}
                  <span class="pill" class:ok={tools.ytdlp && tools.ffmpeg}><i></i>yt-dlp {tools.ytdlp ? "✓" : "—"} · ffmpeg {tools.ffmpeg ? "✓" : "—"}</span>
                {/if}
              </div>
              <p class="muted">{t("settings.videoHint")}</p>
              {#if tools && (!tools.ytdlp || (!tools.ffmpeg && tools.ffmpegInstallable))}
                <button class="btn primary tools-btn" onclick={installTools} disabled={toolsBusy}>
                  <Icon name="down" size={14} stroke={1.8} />{toolsBusy ? t("settings.installing") : t("settings.installTools")}
                </button>
              {:else if tools && !tools.ffmpeg}
                <p class="faint">{t("settings.ffmpegManual")}</p>
              {/if}
            </div>
            <div class="row">
              <div class="l">
                <div class="t">{t("settings.capture")}</div>
                <div class="h">{t("settings.captureHint")}</div>
              </div>
              <Toggle checked={s.captureEnabled} onchange={(v) => save({ captureEnabled: v })} />
            </div>
            <div class="row">
              <div class="l">
                <div class="t">{t("settings.confirmNew")}</div>
                <div class="h">{t("settings.confirmNewHint")}</div>
              </div>
              <Toggle checked={s.confirmNew} onchange={(v) => save({ confirmNew: v })} />
            </div>
            <div class="row col">
              <div class="l"><div class="t">{t("settings.captureExt")}</div></div>
              <textarea
                class="input mono"
                rows="3"
                bind:value={extText}
                onchange={() =>
                  save({
                    captureExtensions: extText
                      .split(/[\s,]+/)
                      .map((x) => x.replace(/^\./, "").toLowerCase())
                      .filter(Boolean),
                  })}
              ></textarea>
            </div>
            <div class="row">
              <div class="l"><div class="t">{t("settings.captureMin")}</div></div>
              <input
                class="input short num"
                type="number"
                min="0"
                value={s.captureMinSizeKb}
                onchange={(e) => save({ captureMinSizeKb: Math.max(0, +e.currentTarget.value || 0) })}
              />
            </div>
          {:else if tab === "scheduler"}
            <div class="row">
              <div class="l"><div class="t">{t("settings.scheduleEnable")}</div></div>
              <Toggle checked={s.schedule.enabled} onchange={(v) => save({ schedule: { ...s.schedule, enabled: v } })} />
            </div>
            <div class="row" class:dim={!s.schedule.enabled}>
              <div class="l"><div class="t">{t("settings.scheduleStart")}</div></div>
              <input class="input short num" type="time" value={s.schedule.start} onchange={(e) => save({ schedule: { ...s.schedule, start: e.currentTarget.value } })} />
            </div>
            <div class="row" class:dim={!s.schedule.enabled}>
              <div class="l"><div class="t">{t("settings.scheduleStop")}</div></div>
              <input class="input short num" type="time" value={s.schedule.stop} onchange={(e) => save({ schedule: { ...s.schedule, stop: e.currentTarget.value } })} />
            </div>
            <div class="row col">
              <div class="l"><div class="t">{t("settings.afterQueue")}</div></div>
              <div class="seg wide">
                {#each ["nothing", "shutdown", "sleep"] as const as v}
                  <button class:on={s.afterQueue === v} class:danger={v !== "nothing" && s.afterQueue === v} onclick={() => save({ afterQueue: v })}
                    >{t(`settings.after.${v}`)}</button
                  >
                {/each}
              </div>
            </div>
          {:else if tab === "about"}
            <div class="about">
              <Logo size={56} />
              <h2>Velox</h2>
              <p class="muted">{t("app.tagline")}</p>
              <p class="faint num">{t("settings.version")} {info?.version ?? ""}</p>
              <div class="about-actions">
                {#if update === "idle" || update === "checking"}
                  <button class="btn" onclick={checkUpdate} disabled={update === "checking"}>
                    <Icon name="retry" size={14} />{t("settings.checkUpdate")}
                  </button>
                {:else if update === "none"}
                  <span class="muted"><Icon name="check" size={14} /> {t("settings.upToDate")}</span>
                {:else}
                  <span>{t("settings.updateAvailable", { v: update })}</span>
                  <button class="btn primary" disabled={installing} onclick={() => ((installing = true), api.installUpdate())}>{t("settings.install")}</button>
                {/if}
              </div>
            </div>
            <div class="row">
              <div class="l"><div class="t">{t("settings.ffmpeg")}</div></div>
              <span class="pill" class:ok={info?.ffmpeg}><i></i>{info?.ffmpeg ? t("settings.found") : t("settings.notFound")}</span>
            </div>
            <div class="row">
              <div class="l"><div class="t">GitHub</div></div>
              <span class="mono muted">github.com/ethemdemirkaya/velox</span>
            </div>
            <p class="faint lic">{t("settings.license")}</p>
          {/if}
        </div>
      {/key}
    </div>
  {/if}
</div>

<style>
  .wrap {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .tabs {
    width: 210px;
    flex: none;
    padding: 18px 10px;
    border-right: 1px solid var(--border);
  }
  h1 {
    margin: 0 10px 16px;
    font-size: 17px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }
  .tab {
    width: 100%;
    height: 32px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 10px;
    border-radius: var(--radius);
    color: var(--text-2);
  }
  .tab:hover {
    background: var(--hover);
    color: var(--text);
  }
  .tab.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .pane {
    flex: 1;
    overflow-y: auto;
    padding: 24px 32px 48px;
  }
  .sect {
    max-width: 640px;
    animation: fade 0.18s var(--ease);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 16px 0;
    border-bottom: 1px solid var(--border);
  }
  .row.col {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
  }
  .row.dim {
    opacity: 0.45;
  }
  .t {
    font-weight: 500;
  }
  .h {
    font-size: 12px;
    color: var(--text-3);
    margin-top: 2px;
  }
  .seg {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--border-2);
    border-radius: var(--radius);
    background: var(--bg-raised);
  }
  .seg.wide {
    display: flex;
  }
  .seg.wide button {
    flex: 1;
  }
  .seg button {
    height: 26px;
    padding: 0 12px;
    border-radius: 4px;
    color: var(--text-2);
    font-size: 12px;
  }
  .seg button:hover {
    color: var(--text);
  }
  .seg button.on {
    background: var(--surface-3);
    color: var(--text);
  }
  .seg button.on.danger {
    background: var(--red);
    color: #fff;
  }
  .dir {
    display: flex;
    gap: 8px;
  }
  .value {
    flex: 1;
    min-width: 0;
    text-align: left;
    padding: 0 10px;
    height: 30px;
    border: 1px solid var(--border-2);
    border-radius: var(--radius);
    background: var(--bg-raised);
    color: var(--text-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .value:hover {
    color: var(--text);
    border-color: var(--border-3);
  }
  .stepper {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .stepper span {
    width: 18px;
    text-align: center;
    font-weight: 600;
  }
  .short {
    width: 150px;
  }
  textarea {
    font-size: 11.5px;
  }
  .card {
    padding: 16px;
    border: 1px solid var(--border-2);
    border-radius: var(--radius-l);
    background: var(--bg-raised);
    margin-bottom: 8px;
  }
  .card-h {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .card-h .pill {
    margin-left: auto;
  }
  .tools-btn {
    margin-top: 12px;
  }
  .card p {
    margin: 10px 0 0;
    font-size: 12px;
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 22px;
    padding: 0 9px;
    border: 1px solid var(--border-2);
    border-radius: 11px;
    font-size: 11.5px;
    color: var(--text-2);
  }
  .pill i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--red);
  }
  .pill.ok i {
    background: var(--text);
  }
  .about {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: 24px 0 28px;
    border-bottom: 1px solid var(--border);
  }
  .about h2 {
    margin: 14px 0 2px;
    font-size: 22px;
    letter-spacing: -0.03em;
  }
  .about p {
    margin: 2px 0;
  }
  .about-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 16px;
  }
  .about-actions span {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .lic {
    margin-top: 20px;
    font-size: 12px;
  }
</style>
