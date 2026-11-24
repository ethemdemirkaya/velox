<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import FileIcon from "./FileIcon.svelte";
  import { api, type AddRequest, type ProbeInfo } from "../lib/api";
  import { i18n, t } from "../lib/i18n.svelte";
  import { bytes, host } from "../lib/format";

  let { token }: { token: string } = $props();
  const win = getCurrentWindow();

  let req = $state<AddRequest | null>(null);
  let probe = $state<ProbeInfo | null>(null);
  let error = $state("");
  let fileName = $state("");
  let dir = $state("");
  let busy = $state(false);

  onMount(async () => {
    try {
      const s = await api.getSettings();
      i18n.set(s.language);
      req = await api.getPending(token);
      // Show a name right away; the probe refines it.
      fileName = req.fileName ?? decodeURIComponent(new URL(req.url).pathname.split("/").filter(Boolean).pop() ?? "");
      probe = await api.probe(req);
      if (!req.fileName) fileName = probe.fileName;
      dir = probe.dir;
    } catch (e) {
      error = String(e);
    }
  });

  async function done(action: "download" | "later" | "cancel") {
    if (busy) return;
    busy = true;
    try {
      if (action === "cancel" || !req) await api.resolvePending(token, null);
      else
        await api.resolvePending(token, {
          ...req,
          fileName: fileName || null,
          dir: dir || null,
          kind: probe?.kind ?? req.kind ?? null,
          startPaused: action === "later",
        });
    } catch (e) {
      error = String(e);
      busy = false;
      return;
    }
    win.close();
  }

  async function pick() {
    const p = await api.pickFolder();
    if (p) dir = p;
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key === "Escape") done("cancel");
    if (e.key === "Enter" && probe) done("download");
  }}
/>

<div class="win">
  <header data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region><Logo size={14} /><span data-tauri-drag-region>{t("confirm.title")}</span></div>
    <button class="x" onclick={() => done("cancel")} aria-label={t("action.close")}><Icon name="x" size={14} /></button>
  </header>

  <div class="body">
    <div class="file">
      {#if probe || fileName}
        <FileIcon name={fileName} category={probe?.category ?? "other"} size={48} />
      {:else}
        <div class="sk"></div>
      {/if}
      <div class="info">
        <input class="name" bind:value={fileName} placeholder={t("add.probing")} spellcheck="false" />
        <div class="meta">
          {#if error}
            <span class="err">{error}</span>
          {:else if probe}
            <span class="num strong">{probe.size != null ? bytes(probe.size) : t("add.unknownSize")}</span>
            <span>·</span>
            <span
              >{probe.kind === "site"
                ? t("add.video")
                : probe.kind !== "http"
                  ? `${t("add.stream")} ${probe.kind.toUpperCase()}`
                  : probe.resumable
                    ? t("add.resumable")
                    : t("add.notResumable")}</span
            >
            <span>·</span>
            <span class="host">{host(probe.finalUrl)}</span>
          {:else}
            <span>{t("add.probing")}</span>
          {/if}
        </div>
      </div>
    </div>

    <div class="field">
      <span class="label">{t("add.saveTo")}</span>
      <div class="dir">
        <input class="input mono" bind:value={dir} spellcheck="false" />
        <button class="btn icon" onclick={pick} title={t("action.change")}><Icon name="folder" size={14} /></button>
      </div>
    </div>
  </div>

  <footer>
    <button class="btn ghost" onclick={() => done("cancel")}>{t("action.cancel")}</button>
    <div class="right">
      <button class="btn" onclick={() => done("later")} disabled={!probe || busy}>{t("action.later")}</button>
      <button class="btn primary" onclick={() => done("download")} disabled={!probe || busy}>
        <Icon name="down" size={14} stroke={1.8} />{t("action.download")}
      </button>
    </div>
  </footer>
</div>

<style>
  .win {
    height: 100vh;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border-2);
    background: var(--bg-raised);
  }
  header {
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 6px 0 14px;
    border-bottom: 1px solid var(--border);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    font-weight: 600;
    font-size: 12.5px;
  }
  .x {
    width: 28px;
    height: 28px;
    display: grid;
    place-items: center;
    border-radius: var(--radius);
    color: var(--text-2);
  }
  .x:hover {
    background: var(--red);
    color: #fff;
  }
  .body {
    flex: 1;
    padding: 20px 18px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }
  .file {
    display: flex;
    gap: 14px;
    align-items: center;
  }
  .sk {
    width: 48px;
    height: 48px;
    border-radius: var(--radius);
    background: var(--surface-2);
    animation: blink 1s ease-in-out infinite;
  }
  .info {
    flex: 1;
    min-width: 0;
  }
  .name {
    width: 100%;
    background: none;
    border: 0;
    border-bottom: 1px solid var(--border-2);
    outline: none;
    font-size: 15px;
    font-weight: 600;
    padding: 2px 0 5px;
  }
  .name:focus {
    border-bottom-color: var(--text-3);
  }
  .meta {
    display: flex;
    gap: 6px;
    margin-top: 6px;
    font-size: 12px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
  }
  .strong {
    color: var(--text);
    font-weight: 500;
  }
  .host {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .err {
    color: var(--red);
    white-space: normal;
  }
  .dir {
    display: flex;
    gap: 6px;
  }
  .dir .input {
    font-size: 11.5px;
  }
  footer {
    display: flex;
    justify-content: space-between;
    padding: 12px 14px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
  .right {
    display: flex;
    gap: 6px;
  }
</style>
