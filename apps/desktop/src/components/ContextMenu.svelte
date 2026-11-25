<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import { api } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { t } from "../lib/i18n.svelte";
  import { toasts } from "../lib/toasts.svelte";

  const m = $derived(store.menu!);
  const ids = $derived(m.ids);
  const one = $derived(ids.length === 1 ? store.byId[ids[0]] : null);
  const anyActive = $derived(ids.some((id) => store.byId[id] && store.isActive(store.byId[id])));
  const anyResumable = $derived(ids.some((id) => store.byId[id] && !store.isActive(store.byId[id]) && store.byId[id].status !== "completed"));

  let el: HTMLDivElement;
  let pos = $state({ x: 0, y: 0 });

  onMount(() => {
    const r = el.getBoundingClientRect();
    pos = { x: Math.min(m.x, innerWidth - r.width - 8), y: Math.min(m.y, innerHeight - r.height - 8) };
  });

  function close() {
    store.menu = null;
  }
  function run(fn: () => unknown) {
    close();
    fn();
  }
  function copy() {
    if (one) api.writeClipboard(one.url).then(() => toasts.push({ kind: "info", title: t("action.copyUrl"), body: one!.url, timeout: 2000 }));
  }
</script>

<svelte:window
  onmousedown={(e) => !el.contains(e.target as Node) && close()}
  onkeydown={(e) => e.key === "Escape" && close()}
  onblur={close}
  onresize={close}
/>

<div class="cm" bind:this={el} style:left="{pos.x}px" style:top="{pos.y}px" role="menu">
  {#if one?.status === "completed"}
    <button role="menuitem" onclick={() => run(() => api.openFile(one!.id))}><Icon name="open" size={14} /><span>{t("action.open")}</span><kbd>Enter</kbd></button>
  {/if}
  {#if one}
    <button role="menuitem" onclick={() => run(() => api.showInFolder(one!.id))}><Icon name="folder" size={14} /><span>{t("action.folder")}</span></button>
  {/if}
  {#if anyActive}
    <button role="menuitem" onclick={() => run(() => store.pauseMany(ids))}><Icon name="pause" size={14} /><span>{t("action.pause")}</span><kbd>Space</kbd></button>
  {/if}
  {#if anyResumable}
    <button role="menuitem" onclick={() => run(() => store.resumeMany(ids))}>
      <Icon name={one?.status === "failed" ? "retry" : "play"} size={13} /><span>{one?.status === "failed" ? t("action.retry") : t("action.resume")}</span>
    </button>
  {/if}
  {#if one}
    <button role="menuitem" onclick={() => run(copy)}><Icon name="link" size={14} /><span>{t("action.copyUrl")}</span></button>
  {/if}
  <div class="div"></div>
  <button role="menuitem" onclick={() => run(() => store.selectAll())}><Icon name="list" size={14} /><span>{t("menu.selectAll")}</span><kbd>Ctrl A</kbd></button>
  <div class="div"></div>
  <button role="menuitem" onclick={() => run(() => store.removeFromList(ids))}>
    <Icon name="x" size={14} /><span>{t("menu.removeList")}{ids.length > 1 ? ` (${ids.length})` : ""}</span>
  </button>
  <button role="menuitem" class="danger" onclick={() => run(() => store.askRemove(ids))}>
    <Icon name="trash" size={14} /><span>{t("menu.deleteFile")}</span><kbd>Del</kbd>
  </button>
</div>

<style>
  .cm {
    position: fixed;
    z-index: 60;
    min-width: 220px;
    padding: 4px;
    background: var(--surface-2);
    border: 1px solid var(--border-2);
    border-radius: var(--radius-l);
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.55);
    animation: pop 0.12s var(--ease-out);
  }
  button {
    width: 100%;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 10px;
    border-radius: var(--radius);
    color: var(--text-2);
    text-align: left;
  }
  button:hover {
    background: var(--surface-3);
    color: var(--text);
  }
  button span {
    flex: 1;
    white-space: nowrap;
  }
  kbd {
    font-family: var(--mono);
    font-size: 10px;
    color: var(--text-4);
  }
  .danger:hover {
    color: var(--red);
  }
  .div {
    height: 1px;
    margin: 4px 6px;
    background: var(--border-2);
  }
</style>
