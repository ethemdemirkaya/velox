<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
  import BlockMap from "./BlockMap.svelte";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import FileIcon from "./FileIcon.svelte";
  import SegmentBar from "./SegmentBar.svelte";
  import { api, onEngine, type Download, type Settings } from "../lib/api";
  import { i18n, t } from "../lib/i18n.svelte";
  import { bytes, speed, eta, percent } from "../lib/format";

  // IDM-style window that follows a single download, then turns into the
  // "Download complete" dialog with Open / Open folder / Close.
  let { id }: { id: string } = $props();
  const win = getCurrentWindow();
  let d = $state<Download | null>(null);
  let settings = $state<Settings | null>(null);
  let details = $state(false);

  const W = 540;
  const H = 330;
  const H_DETAILS = 600;

  async function toggleDetails() {
    details = !details;
    await win.setSize(new LogicalSize(W, details ? H_DETAILS : H));
    if (settings) {
      settings = { ...settings, progressDetails: details };
      api.setSettings(settings);
    }
  }

  // Connections table: working ranges first, then waiting, then finished.
  const rows = $derived.by(() => {
    if (!d || d.total == null || !d.segments.length || d.segments[0].end > 1e17) return [];
    return d.segments
      .map((s, i) => ({
        n: i + 1,
        size: s.end - s.start,
        got: Math.min(s.written, s.end) - s.start,
        state: s.written >= s.end ? "done" : s.active ? "recv" : "wait",
      }))
      .sort((a, b) => ({ recv: 0, wait: 1, done: 2 })[a.state as "recv"] - ({ recv: 0, wait: 1, done: 2 })[b.state as "recv"] || a.n - b.n);
  });

  const active = $derived(!!d && ["connecting", "downloading", "merging"].includes(d.status));
  const done = $derived(d?.status === "completed");
  const failed = $derived(d?.status === "failed");
  const pct = $derived(d ? (done ? 100 : percent(d.downloaded, d.total)) : 0);

  onMount(() => {
    let un: (() => void) | undefined;
    (async () => {
      const s = await api.getSettings();
      settings = s;
      details = s.progressDetails;
      i18n.set(s.language);
      d = (await api.list()).find((x) => x.id === id) ?? null;
      un = await onEngine((e) => {
        if (e.type === "updated" && e.data.id === id) d = e.data;
        else if (e.type === "removed" && e.data === id) win.close();
        else if (e.type === "progress" && d) {
          const p = e.data.find((x) => x.id === id);
          if (p) Object.assign(d, p);
        }
      });
    })();
    return () => un?.();
  });

  $effect(() => {
    if (done) win.setFocus();
  });

  const title = $derived.by(() => {
    if (!d) return t("progress.resolving");
    if (done) return t("progress.done");
    if (failed) return t("progress.failed");
    if (d.status === "merging") return t("progress.merging");
    if (d.status === "paused") return t("status.paused");
    if (d.status === "connecting" || d.status === "queued") return t("progress.resolving");
    return `${pct.toFixed(pct < 10 ? 1 : 0)}%`;
  });

  async function open() {
    await api.openFile(id);
    win.close();
  }
  async function folder() {
    await api.showInFolder(id);
    win.close();
  }
  async function cancel() {
    await api.remove(id, true);
    win.close();
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key === "Escape") win.close();
    if (e.key === "Enter" && done) open();
  }}
/>

<div class="win" class:done>
  <header data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <Logo size={13} />
      <span data-tauri-drag-region>{title}</span>
    </div>
    <button class="x" onclick={() => win.close()} aria-label={t("action.close")}><Icon name="x" size={14} /></button>
  </header>

  {#if d}
    <div class="body">
      <div class="file">
        <FileIcon name={d.fileName} category={d.category} size={42} live={active} />
        <div class="meta">
          <div class="name" title={d.fileName}>{d.fileName}</div>
          <div class="sub mono" title={d.dir}>{d.dir}</div>
        </div>
      </div>

      {#if done}
        <div class="complete">
          <span class="check"><Icon name="check" size={16} stroke={2} /></span>
          <span class="num">{bytes(d.total)}</span>
        </div>
      {:else}
        <div class="bar">
          <SegmentBar segments={d.segments} total={d.total} downloaded={d.downloaded} {active} height={6} gap />
        </div>
        <div class="stats">
          <div><span>{t("detail.downloaded")}</span><b class="num">{bytes(d.downloaded)} <i>/ {bytes(d.total)}</i></b></div>
          <div><span>{t("detail.speed")}</span><b class="num">{active ? speed(d.speed) : "—"}</b></div>
          <div><span>{t("detail.eta")}</span><b class="num">{active ? eta(d.eta) : "—"}</b></div>
          <div><span>{t("detail.connections")}</span><b class="num">{active ? d.activeConnections : "—"}</b></div>
        </div>
        {#if failed && d.error}<div class="err">{d.error}</div>{/if}
        {#if details}
          <div class="details">
            <BlockMap segments={d.segments} total={d.total} {done} {active} />
            <div class="table">
              <div class="th"><span>#</span><span>{t("progress.colProgress")}</span><span>{t("detail.downloaded")}</span><span>{t("progress.colState")}</span></div>
              <div class="tbody">
                {#each rows as r (r.n)}
                  <div class="tr" class:recv={r.state === "recv"} class:fin={r.state === "done"}>
                    <span class="num">{r.n}</span>
                    <span class="mini"><i style:width="{r.size ? (r.got / r.size) * 100 : 0}%"></i></span>
                    <span class="num">{bytes(r.got)} <em>/ {bytes(r.size)}</em></span>
                    <span>{r.state === "recv" ? t("progress.receiving") : r.state === "done" ? t("progress.segDone") : t("progress.waiting")}</span>
                  </div>
                {:else}
                  <div class="empty faint">{t("progress.noSegments")}</div>
                {/each}
              </div>
            </div>
          </div>
        {/if}
      {/if}
    </div>

    <footer>
      {#if done}
        <button class="btn ghost" onclick={() => win.close()}>{t("action.close")}</button>
        <div class="right">
          <button class="btn" onclick={folder}><Icon name="folder" size={14} />{t("progress.openFolder")}</button>
          <button class="btn primary" onclick={open}><Icon name="open" size={14} />{t("action.open")}</button>
        </div>
      {:else}
        <div class="left">
          <button class="btn ghost" onclick={toggleDetails}>
            <span class="chev" class:open={details}><Icon name="chevron" size={13} /></span>
            {details ? t("progress.hideDetails") : t("progress.details")}
          </button>
          <button class="btn ghost danger" onclick={cancel}>{t("action.cancel")}</button>
        </div>
        <div class="right">
          <button class="btn" onclick={() => win.close()}>{t("progress.hide")}</button>
          {#if active}
            <button class="btn primary" onclick={() => api.pause(id)}><Icon name="pause" size={14} />{t("action.pause")}</button>
          {:else}
            <button class="btn primary" onclick={() => api.resume(id)}>
              <Icon name={failed ? "retry" : "play"} size={13} />{failed ? t("action.retry") : t("action.resume")}
            </button>
          {/if}
        </div>
      {/if}
    </footer>
  {/if}
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
    height: 38px;
    flex: none;
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
    font-variant-numeric: tabular-nums;
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
    padding: 18px 18px 8px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .file {
    display: flex;
    gap: 13px;
    align-items: center;
  }
  .meta {
    min-width: 0;
    flex: 1;
  }
  .name {
    font-size: 14px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-top: 2px;
  }
  .stats {
    display: grid;
    grid-template-columns: 1.4fr 1fr 1fr 0.8fr;
    gap: 1px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    background: var(--border);
  }
  .stats div {
    background: var(--bg-raised);
    padding: 8px 10px;
    min-width: 0;
  }
  .stats span {
    display: block;
    font-size: 10.5px;
    color: var(--text-3);
  }
  .stats b {
    font-size: 12.5px;
    font-weight: 600;
    white-space: nowrap;
  }
  .stats i {
    font-style: normal;
    color: var(--text-3);
    font-weight: 400;
  }
  .complete {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border: 1px solid var(--border-2);
    border-radius: var(--radius);
    color: var(--text-2);
  }
  .check {
    width: 24px;
    height: 24px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--text);
    color: #0a0a0a;
  }
  .err {
    color: var(--red);
    font-size: 12px;
    word-break: break-word;
  }
  footer {
    display: flex;
    justify-content: space-between;
    padding: 11px 14px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
  .right,
  .left {
    display: flex;
    gap: 6px;
  }
  .chev {
    display: inline-flex;
    transition: transform 0.15s var(--ease);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .details {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-height: 0;
    flex: 1;
    animation: fade 0.15s var(--ease);
  }
  .table {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }
  .th,
  .tr {
    display: grid;
    grid-template-columns: 28px 1fr 130px 100px;
    gap: 10px;
    align-items: center;
    padding: 0 10px;
  }
  .th {
    height: 26px;
    font-size: 10.5px;
    color: var(--text-3);
    border-bottom: 1px solid var(--border);
    background: var(--bg);
  }
  .tbody {
    overflow-y: auto;
    max-height: 168px;
  }
  .tr {
    height: 24px;
    font-size: 11.5px;
    color: var(--text-2);
  }
  .tr:nth-child(even) {
    background: rgba(255, 255, 255, 0.015);
  }
  .tr.recv {
    color: var(--text);
  }
  .tr.fin {
    color: var(--text-3);
  }
  .tr em {
    font-style: normal;
    color: var(--text-3);
  }
  .mini {
    height: 3px;
    background: var(--track);
    border-radius: 1px;
    overflow: hidden;
  }
  .mini i {
    display: block;
    height: 100%;
    background: var(--fill-dim);
  }
  .tr.recv .mini i {
    background: var(--fill);
    box-shadow: 2px 0 0 var(--red);
  }
  .empty {
    padding: 14px;
    font-size: 12px;
  }
</style>
