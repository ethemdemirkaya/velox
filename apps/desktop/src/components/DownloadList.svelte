<script lang="ts">
  import Icon from "./Icon.svelte";
  import DownloadRow from "./DownloadRow.svelte";
  import { store, type Sort } from "../lib/store.svelte";
  import { t, type I18nKey } from "../lib/i18n.svelte";
  import { api } from "../lib/api";

  const titles: Record<string, I18nKey> = {
    all: "nav.all",
    active: "nav.active",
    queued: "nav.queued",
    completed: "nav.completed",
    failed: "nav.failed",
    video: "cat.video",
    music: "cat.music",
    documents: "cat.documents",
    programs: "cat.programs",
    compressed: "cat.compressed",
    images: "cat.images",
    other: "cat.other",
  };
  const sorts: Sort[] = ["added", "name", "size", "progress"];
  const sortLabel: Record<Sort, I18nKey> = { added: "detail.added", name: "col.name", size: "col.size", progress: "col.progress" };

  function cycleSort() {
    store.sort = sorts[(sorts.indexOf(store.sort) + 1) % sorts.length];
  }

  // Drop links or text onto the list to add them.
  let dragging = $state(false);
  function onDrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    const txt = e.dataTransfer?.getData("text/uri-list") || e.dataTransfer?.getData("text/plain") || "";
    if (txt.trim()) store.openAdd(txt.trim());
  }
</script>

<div class="toolbar">
  <div class="title">
    <h1>{t(titles[store.view])}</h1>
    <span class="faint num">{store.visible.length}</span>
  </div>
  <div class="actions">
    <button class="btn ghost sm" onclick={cycleSort} title="Sort">
      <Icon name="sort" size={14} />
      <span class="lbl">{t(sortLabel[store.sort])}</span>
    </button>
    <span class="sep"></span>
    <button class="btn ghost sm" onclick={() => api.resumeAll()}>
      <Icon name="play" size={13} />
      <span class="lbl">{t("action.resumeAll")}</span>
    </button>
    <button class="btn ghost sm" onclick={() => api.pauseAll()}>
      <Icon name="pause" size={14} />
      <span class="lbl">{t("action.pauseAll")}</span>
    </button>
    {#if (store.counts.completed ?? 0) > 0}
      <button class="btn ghost sm" onclick={() => api.clearCompleted()}>
        <Icon name="check" size={14} />
        <span class="lbl">{t("action.clear")}</span>
      </button>
    {/if}
  </div>
</div>

<div class="head">
  <span class="c-name">{t("col.name")}</span>
  <span class="c-size">{t("col.size")}</span>
  <span class="c-prog">{t("col.progress")}</span>
  <span class="c-speed">{t("col.speed")}</span>
  <span class="c-eta">{t("col.eta")}</span>
  <span class="c-act"></span>
</div>

<div
  class="list"
  class:dragging
  role="listbox"
  tabindex="-1"
  ondragover={(e) => {
    e.preventDefault();
    dragging = true;
  }}
  ondragleave={() => (dragging = false)}
  ondrop={onDrop}
>
  {#each store.visible as d (d.id)}
    <DownloadRow {d} />
  {:else}
    <div class="empty">
      {#if store.query}
        <img src="/empty-search.png" alt="" width="180" />
        <p class="muted">{t("empty.search")}</p>
      {:else}
        <img src="/empty-state.png" alt="" width="180" />
        <h2>{t("empty.title")}</h2>
        <p class="muted">{t("empty.body")}</p>
        <div class="empty-actions">
          <button class="btn primary" onclick={() => store.openAdd()}>
            <Icon name="plus" size={14} stroke={2} />
            {t("action.new")}
          </button>
          <span class="faint">
            <span class="kbd">Ctrl</span> <span class="kbd">V</span>
          </span>
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .toolbar {
    height: 54px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 16px 0 24px;
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-width: 0;
    white-space: nowrap;
  }
  h1 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: none;
  }
  /* Narrow (detail panel open / small window): icon-only actions, fewer columns */
  @container main (max-width: 860px) {
    .actions .btn :global(.lbl) {
      display: none;
    }
    .actions .btn {
      width: 28px;
      padding: 0;
    }
  }
  .sep {
    width: 1px;
    height: 16px;
    background: var(--border-2);
    margin: 0 6px;
  }
  .head {
    flex: none;
    display: grid;
    grid-template-columns: var(--cols);
    gap: 16px;
    align-items: center;
    height: 30px;
    padding: 0 16px 0 24px;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    font-size: 11px;
    color: var(--text-3);
    letter-spacing: 0.02em;
  }
  .c-size,
  .c-speed,
  .c-eta {
    text-align: right;
  }
  .list {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 4px 0 24px;
    outline: 1px dashed transparent;
    outline-offset: -8px;
    transition: outline-color 0.15s;
  }
  .list.dragging {
    outline-color: var(--border-3);
  }
  :global(.main) {
    --cols: minmax(200px, 1fr) 84px minmax(120px, 200px) 92px 72px 64px;
  }
  @container main (max-width: 860px) {
    .head {
      --cols: minmax(160px, 1fr) 84px minmax(90px, 160px) 84px;
    }
    .c-eta,
    .c-act {
      display: none;
    }
  }
  @container main (max-width: 620px) {
    .head {
      --cols: minmax(140px, 1fr) minmax(80px, 140px) 76px;
    }
    .c-size {
      display: none;
    }
  }
  .empty {
    height: 100%;
    min-height: 320px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 24px;
    animation: fade 0.3s var(--ease);
  }
  .empty img {
    opacity: 0.9;
    margin-bottom: 4px;
  }
  .empty h2 {
    margin: 0 0 4px;
    font-size: 15px;
    font-weight: 600;
  }
  .empty p {
    margin: 0;
    max-width: 320px;
  }
  .empty-actions {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-top: 18px;
  }
</style>
