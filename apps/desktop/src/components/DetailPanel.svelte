<script lang="ts">
  import Icon from "./Icon.svelte";
  import FileIcon from "./FileIcon.svelte";
  import BlockMap from "./BlockMap.svelte";
  import SpeedGraph from "./SpeedGraph.svelte";
  import { api } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { t, i18n } from "../lib/i18n.svelte";
  import { bytes, speed, eta, percent, relTime, parseRate } from "../lib/format";
  import { toasts } from "../lib/toasts.svelte";

  const d = $derived(store.current!);
  const active = $derived(store.isActive(d));
  const done = $derived(d.status === "completed");
  const pct = $derived(done ? 100 : percent(d.downloaded, d.total));
  const hist = $derived(store.history[d.id] ?? []);
  let limitText = $state("");

  $effect(() => {
    limitText = d.speedLimit ? bytes(d.speedLimit).replace(" ", "") : "";
  });

  function applyLimit() {
    api.setLimit(d.id, parseRate(limitText));
  }

  function copy(text: string) {
    api.writeClipboard(text);
    toasts.push({ kind: "info", title: t("action.copyUrl"), body: text, timeout: 2000 });
  }
</script>

<aside class="panel">
  {#key d.id}
    <div class="inner">
      <div class="head">
        <FileIcon name={d.fileName} category={d.category} size={40} live={active} />
        <button class="btn ghost sm icon close" onclick={() => (store.selected = null)} aria-label={t("action.close")}>
          <Icon name="x" size={14} />
        </button>
      </div>
      <h2 title={d.fileName}>{d.fileName}</h2>
      <div class="status">
        {#if active}<span class="dot-live"></span>{/if}
        <span class:err={d.status === "failed"}>{t(("status." + d.status) as any)}</span>
        {#if d.kind !== "http"}<span class="tag">{d.kind.toUpperCase()}</span>{/if}
      </div>

      <div class="big">
        <span class="pct num">{pct.toFixed(pct < 10 && pct > 0 ? 1 : 0)}<small>%</small></span>
        {#if active}<span class="spd num">{speed(d.speed)}</span>{/if}
      </div>

      <section>
        <div class="sec-h">
          <span class="label">{d.kind === "hls" ? t("detail.segmentsHls") : t("detail.segments")}</span>
          {#if active && d.kind === "http"}<span class="faint num small">{d.activeConnections} / {d.connections}</span>{/if}
          {#if d.kind === "hls" && d.hlsCount}<span class="faint num small">{d.hlsDone} / {d.hlsCount}</span>{/if}
        </div>
        <BlockMap segments={d.segments} total={d.total} {done} {active} />
      </section>

      {#if active || hist.length > 1}
        <section>
          <div class="sec-h"><span class="label">{t("detail.speedHistory")}</span></div>
          <SpeedGraph data={hist} height={64} grid labels />
        </section>
      {/if}

      <dl class="stats">
        <div><dt>{t("detail.size")}</dt><dd class="num">{bytes(d.total)}</dd></div>
        <div><dt>{t("detail.downloaded")}</dt><dd class="num">{bytes(d.downloaded)}</dd></div>
        <div><dt>{t("detail.eta")}</dt><dd class="num">{active ? eta(d.eta) : "—"}</dd></div>
        <div>
          <dt>{t("detail.resumable")}</dt>
          <dd>{d.resumable ? t("detail.yes") : t("detail.no")}</dd>
        </div>
        <div><dt>{t("detail.added")}</dt><dd>{relTime(d.createdAt, i18n.lang)}</dd></div>
        <div><dt>{t("detail.connections")}</dt><dd class="num">{d.connections}</dd></div>
      </dl>

      {#if d.status === "failed" && d.error}
        <div class="error"><Icon name="alert" size={14} /> <span>{d.error}</span></div>
      {/if}

      <section class="fields">
        <div class="field">
          <span class="label">{t("detail.source")}</span>
          <button class="value mono" title={d.url} onclick={() => copy(d.url)}>
            <span>{d.url}</span>
            <Icon name="copy" size={13} />
          </button>
        </div>
        <div class="field">
          <span class="label">{t("detail.saveTo")}</span>
          <button class="value mono" title={d.dir} onclick={() => api.showInFolder(d.id)}>
            <span>{d.dir}</span>
            <Icon name="folder" size={13} />
          </button>
        </div>
        {#if !done}
          <div class="field">
            <span class="label">{t("detail.limit")}</span>
            <input
              class="input"
              placeholder={t("detail.unlimited")}
              bind:value={limitText}
              onchange={applyLimit}
              onkeydown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
            />
          </div>
        {/if}
      </section>
    </div>

    <div class="actions">
      {#if active}
        <button class="btn" onclick={() => api.pause(d.id)}><Icon name="pause" size={14} />{t("action.pause")}</button>
      {:else if done}
        <button class="btn primary" onclick={() => api.openFile(d.id)}><Icon name="open" size={14} />{t("action.open")}</button>
      {:else}
        <button class="btn primary" onclick={() => api.resume(d.id)}>
          <Icon name={d.status === "failed" ? "retry" : "play"} size={13} />
          {d.status === "failed" ? t("action.retry") : t("action.resume")}
        </button>
      {/if}
      <button class="btn icon" title={t("action.folder")} onclick={() => api.showInFolder(d.id)}><Icon name="folder" size={14} /></button>
      <button class="btn icon danger" title={t("action.remove")} onclick={() => (store.removeTarget = d.id)}><Icon name="trash" size={14} /></button>
    </div>
  {/key}
</aside>

<style>
  .panel {
    width: var(--detail);
    flex: none;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--border);
    background: var(--bg-raised);
    animation: slide-in 0.2s var(--ease-out);
  }
  .inner {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 18px 20px;
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
  }
  h2 {
    margin: 14px 0 4px;
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.35;
    word-break: break-word;
    user-select: text;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--text-2);
    font-size: 12px;
  }
  .status .err {
    color: var(--red);
  }
  .tag {
    font-family: var(--mono);
    font-size: 10px;
    padding: 1px 5px;
    border: 1px solid var(--border-2);
    border-radius: 3px;
    color: var(--text-2);
  }
  .big {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin: 18px 0 14px;
  }
  .pct {
    font-size: 40px;
    font-weight: 600;
    letter-spacing: -0.04em;
    line-height: 1;
  }
  .pct small {
    font-size: 18px;
    color: var(--text-3);
    margin-left: 2px;
    font-weight: 500;
  }
  .spd {
    font-size: 14px;
    font-weight: 500;
  }
  section {
    margin-bottom: 18px;
  }
  .sec-h {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }
  .small {
    font-size: 11px;
  }
  .stats {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1px;
    margin: 0 0 18px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    background: var(--border);
  }
  .stats div {
    background: var(--bg-raised);
    padding: 9px 11px;
  }
  dt {
    font-size: 11px;
    color: var(--text-3);
  }
  dd {
    margin: 2px 0 0;
    font-size: 13px;
    font-weight: 500;
  }
  .error {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 10px 12px;
    margin-bottom: 18px;
    border: 1px solid var(--red-line);
    background: var(--red-soft);
    color: #ffb4ae;
    border-radius: var(--radius);
    font-size: 12px;
    user-select: text;
    word-break: break-word;
  }
  .error :global(svg) {
    color: var(--red);
    margin-top: 1px;
  }
  .fields {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 12px;
  }
  .value {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-2);
    text-align: left;
    transition:
      border-color 0.1s,
      color 0.1s;
  }
  .value span {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .value:hover {
    border-color: var(--border-3);
    color: var(--text);
  }
  .actions {
    display: flex;
    gap: 6px;
    padding: 12px 20px;
    border-top: 1px solid var(--border);
  }
  .actions .btn:first-child {
    flex: 1;
  }
</style>
