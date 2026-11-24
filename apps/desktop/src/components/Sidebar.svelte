<script lang="ts">
  import Icon from "./Icon.svelte";
  import SpeedGraph from "./SpeedGraph.svelte";
  import { store, type View } from "../lib/store.svelte";
  import { t, type I18nKey } from "../lib/i18n.svelte";
  import { speed, bytes } from "../lib/format";

  const library: { id: View; icon: string; label: I18nKey }[] = [
    { id: "all", icon: "list", label: "nav.all" },
    { id: "active", icon: "down", label: "nav.active" },
    { id: "queued", icon: "clock", label: "nav.queued" },
    { id: "completed", icon: "check", label: "nav.completed" },
    { id: "failed", icon: "alert", label: "nav.failed" },
  ];
  const cats: { id: View; icon: string; label: I18nKey }[] = [
    { id: "video", icon: "film", label: "cat.video" },
    { id: "music", icon: "music", label: "cat.music" },
    { id: "documents", icon: "doc", label: "cat.documents" },
    { id: "programs", icon: "app", label: "cat.programs" },
    { id: "compressed", icon: "archive", label: "cat.compressed" },
    { id: "images", icon: "image", label: "cat.images" },
    { id: "other", icon: "file", label: "cat.other" },
  ];

  function go(v: View) {
    store.view = v;
    store.page = "main";
  }

  const limit = $derived(store.settings?.speedLimit ?? 0);
</script>

<aside class="side">
  <div class="top">
    <button class="btn primary new" onclick={() => store.openAdd()}>
      <Icon name="plus" size={15} stroke={2} />
      {t("action.new")}
      <span class="sc">Ctrl N</span>
    </button>
  </div>

  <nav>
    <div class="group">{t("nav.library")}</div>
    {#each library as item (item.id)}
      {@const n = store.counts[item.id] ?? 0}
      <button class="item" class:on={store.view === item.id && store.page === "main"} onclick={() => go(item.id)}>
        <Icon name={item.icon} size={15} />
        <span class="lbl">{t(item.label)}</span>
        {#if item.id === "active" && n > 0}
          <span class="dot-live"></span>
        {/if}
        {#if n > 0}<span class="count num">{n}</span>{/if}
      </button>
    {/each}

    <div class="group">{t("nav.categories")}</div>
    {#each cats as item (item.id)}
      {@const n = store.counts[item.id] ?? 0}
      <button class="item" class:on={store.view === item.id && store.page === "main"} onclick={() => go(item.id)}>
        <Icon name={item.icon} size={15} />
        <span class="lbl">{t(item.label)}</span>
        {#if n > 0}<span class="count num">{n}</span>{/if}
      </button>
    {/each}
  </nav>

  <div class="foot">
    <div class="meter">
      <div class="row">
        <span class="label" style="margin:0">{t("total.speed")}</span>
        <span class="num speed" class:idle={store.totalSpeed < 1}>{speed(store.totalSpeed)}</span>
      </div>
      <SpeedGraph data={store.totalHistory} height={34} />
      <div class="row sub">
        <span class="faint">{t("total.active", { n: store.counts.active ?? 0 })}</span>
        <span class="faint num">{limit ? `≤ ${bytes(limit)}/s` : "∞"}</span>
      </div>
    </div>
    <button class="item settings" class:on={store.page === "settings"} onclick={() => (store.page = store.page === "settings" ? "main" : "settings")}>
      <Icon name="settings" size={15} />
      <span class="lbl">{t("action.settings")}</span>
      <span class="kbd">Ctrl ,</span>
    </button>
  </div>
</aside>

<style>
  .side {
    width: var(--sidebar);
    flex: none;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border);
    background: var(--bg);
  }
  .top {
    padding: 14px 12px 6px;
  }
  .new {
    width: 100%;
    height: 34px;
    justify-content: flex-start;
    padding: 0 10px;
    font-weight: 600;
  }
  .new .sc {
    margin-left: auto;
    font-family: var(--mono);
    font-size: 10.5px;
    font-weight: 400;
    color: #6b6b6b;
  }
  nav {
    flex: 1;
    overflow-y: auto;
    padding: 4px 8px;
  }
  .group {
    padding: 16px 10px 6px;
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-4);
  }
  .item {
    width: 100%;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 10px;
    border-radius: var(--radius);
    color: var(--text-2);
    text-align: left;
    transition:
      background 0.1s,
      color 0.1s;
  }
  .item:hover {
    background: var(--hover);
    color: var(--text);
  }
  .item.on {
    background: var(--surface-2);
    color: var(--text);
  }
  .lbl {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .count {
    font-size: 11px;
    color: var(--text-3);
  }
  .foot {
    padding: 8px;
    border-top: 1px solid var(--border);
  }
  .meter {
    padding: 8px 10px 10px;
    display: grid;
    gap: 6px;
  }
  .row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  .row.sub {
    font-size: 11px;
  }
  .speed {
    font-weight: 600;
    font-size: 13px;
  }
  .speed.idle {
    color: var(--text-3);
    font-weight: 400;
  }
  .settings .kbd {
    margin-left: auto;
  }
</style>
