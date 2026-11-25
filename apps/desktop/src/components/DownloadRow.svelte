<script lang="ts">
  import Icon from "./Icon.svelte";
  import FileIcon from "./FileIcon.svelte";
  import SegmentBar from "./SegmentBar.svelte";
  import { api, type Download } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { t } from "../lib/i18n.svelte";
  import { bytes, speed, eta, percent, host } from "../lib/format";

  let { d }: { d: Download } = $props();

  const active = $derived(store.isActive(d));
  const pct = $derived(percent(d.downloaded, d.total));
  const done = $derived(d.status === "completed");
  const sub = $derived.by(() => {
    if (d.status === "failed") return d.error ?? t("status.failed");
    if (active) {
      const conns = d.activeConnections > 0 ? ` · ${d.activeConnections}×` : "";
      if (d.kind === "hls" && d.hlsCount) return `${t("status." + d.status as any)} · ${d.hlsDone}/${d.hlsCount}${conns}`;
      return `${t(("status." + d.status) as any)}${conns}`;
    }
    if (done) return host(d.url);
    return `${t(("status." + d.status) as any)} · ${host(d.url)}`;
  });
</script>

<div
  class="row"
  class:sel={store.isSelected(d.id)}
  class:multi={store.multi.length > 1 && store.isSelected(d.id)}
  class:active
  class:failed={d.status === "failed"}
  class:done
  data-row={d.id}
  role="option"
  aria-selected={store.isSelected(d.id)}
  tabindex="-1"
  onclick={(e) => store.click(d.id, e)}
  oncontextmenu={(e) => store.openMenu(d.id, e)}
  ondblclick={() => (done ? api.openFile(d.id) : store.toggle(d))}
  onkeydown={() => {}}
>
  <div class="c-name">
    <FileIcon name={d.fileName} category={d.category} live={active} />
    <div class="txt">
      <div class="fname" title={d.fileName}>{d.fileName}</div>
      <div class="sub" class:err={d.status === "failed"}>{sub}</div>
    </div>
  </div>

  <div class="c-size num">
    {#if done}
      {bytes(d.total)}
    {:else if d.total}
      <span class="muted">{bytes(d.downloaded)}</span><span class="faint"> / </span>{bytes(d.total)}
    {:else if d.downloaded}
      {bytes(d.downloaded)}
    {:else}
      <span class="faint">—</span>
    {/if}
  </div>

  <div class="c-prog">
    <SegmentBar segments={d.segments} total={d.total} downloaded={d.downloaded} {active} {done} height={3} />
    <span class="pct num">{done ? "100%" : d.total ? `${pct.toFixed(pct < 10 ? 1 : 0)}%` : ""}</span>
  </div>

  <div class="c-speed num" class:on={active && d.speed > 0}>{active ? speed(d.speed) : ""}</div>
  <div class="c-eta num muted">{active ? eta(d.eta) : ""}</div>

  <div class="c-act">
    {#if active}
      <button class="btn ghost sm icon" title={t("action.pause")} onclick={(e) => (e.stopPropagation(), api.pause(d.id))}>
        <Icon name="pause" size={14} />
      </button>
    {:else if d.status === "failed"}
      <button class="btn ghost sm icon" title={t("action.retry")} onclick={(e) => (e.stopPropagation(), api.resume(d.id))}>
        <Icon name="retry" size={14} />
      </button>
    {:else if !done}
      <button class="btn ghost sm icon" title={t("action.resume")} onclick={(e) => (e.stopPropagation(), api.resume(d.id))}>
        <Icon name="play" size={13} />
      </button>
    {:else}
      <button class="btn ghost sm icon" title={t("action.open")} onclick={(e) => (e.stopPropagation(), api.openFile(d.id))}>
        <Icon name="open" size={14} />
      </button>
    {/if}
    <button class="btn ghost sm icon" title={t("action.folder")} onclick={(e) => (e.stopPropagation(), api.showInFolder(d.id))}>
      <Icon name="folder" size={14} />
    </button>
    <button class="btn ghost sm icon rm" title={t("menu.removeList")} onclick={(e) => (e.stopPropagation(), store.removeFromList([d.id]))}>
      <Icon name="x" size={14} />
    </button>
  </div>
</div>

<style>
  .row {
    position: relative;
    display: grid;
    grid-template-columns: var(--cols);
    gap: 16px;
    align-items: center;
    height: 56px;
    padding: 0 16px 0 24px;
    border-bottom: 1px solid #141414;
    transition: background 0.08s;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.sel {
    background: var(--surface-2);
  }
  .row.multi {
    background: #1c1c1c;
  }
  .rm:hover {
    color: var(--red);
  }
  .row.sel::before {
    content: "";
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 2px;
    background: var(--text);
  }
  .c-name {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }
  .txt {
    min-width: 0;
  }
  .fname {
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .done .fname {
    color: var(--text-2);
  }
  .sub {
    font-size: 11.5px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub.err {
    color: var(--red);
  }
  .c-size {
    text-align: right;
    font-size: 12px;
    white-space: nowrap;
  }
  .c-prog {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .pct {
    width: 38px;
    flex: none;
    text-align: right;
    font-size: 11.5px;
    color: var(--text-2);
  }
  .c-speed {
    text-align: right;
    font-size: 12px;
    color: var(--text-3);
    white-space: nowrap;
  }
  .c-speed.on {
    color: var(--text);
    font-weight: 500;
  }
  .c-eta {
    text-align: right;
    font-size: 12px;
    white-space: nowrap;
  }
  .c-act {
    display: flex;
    justify-content: flex-end;
    gap: 0;
    opacity: 0;
    transition: opacity 0.1s;
  }
  .row:hover .c-act,
  .row.sel .c-act {
    opacity: 1;
  }
  @container main (max-width: 860px) {
    .row {
      --cols: minmax(160px, 1fr) 84px minmax(90px, 160px) 84px;
    }
    .c-eta,
    .c-act {
      display: none;
    }
  }
  @container main (max-width: 620px) {
    .row {
      --cols: minmax(140px, 1fr) minmax(80px, 140px) 76px;
    }
    .c-size {
      display: none;
    }
  }
</style>
