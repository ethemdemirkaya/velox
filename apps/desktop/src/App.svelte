<script lang="ts">
  import { onMount } from "svelte";
  import { store } from "./lib/store.svelte";
  import TitleBar from "./components/TitleBar.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import DownloadList from "./components/DownloadList.svelte";
  import DetailPanel from "./components/DetailPanel.svelte";
  import AddDialog from "./components/AddDialog.svelte";
  import CommandPalette from "./components/CommandPalette.svelte";
  import SettingsPage from "./components/Settings.svelte";
  import Toasts from "./components/Toasts.svelte";
  import RemoveDialog from "./components/RemoveDialog.svelte";
  import ConfirmView from "./components/ConfirmView.svelte";
  import { api } from "./lib/api";

  const confirmToken = location.hash.match(/^#\/confirm\/(\w+)/)?.[1] ?? null;
  let ready = $state(false);

  onMount(async () => {
    if (confirmToken) return;
    await store.init();
    ready = true;
  });

  function isTyping(e: KeyboardEvent) {
    const el = e.target as HTMLElement;
    return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable;
  }

  function onKey(e: KeyboardEvent) {
    if (confirmToken) return;
    const mod = e.ctrlKey || e.metaKey;
    const k = e.key.toLowerCase();
    if (mod && k === "k") {
      e.preventDefault();
      store.paletteOpen = !store.paletteOpen;
      return;
    }
    if (mod && k === "n") {
      e.preventDefault();
      store.openAdd();
      return;
    }
    if (mod && k === ",") {
      e.preventDefault();
      store.page = "settings";
      return;
    }
    if (mod && k === "f") {
      e.preventDefault();
      document.getElementById("search")?.focus();
      return;
    }
    // Paste a link anywhere to add it.
    if (mod && k === "v" && !isTyping(e) && !store.addOpen) {
      api
        .readClipboard()
        .then((txt) => {
          if (/^https?:\/\//i.test(txt.trim())) store.openAdd(txt.trim());
        })
        .catch(() => {});
      return;
    }
    if (store.addOpen || store.paletteOpen || store.removeTarget || isTyping(e)) return;
    if (store.page === "settings") {
      if (k === "escape") store.page = "main";
      return;
    }
    const cur = store.current;
    switch (e.key) {
      case "ArrowDown":
      case "j":
        e.preventDefault();
        store.moveSelection(1);
        break;
      case "ArrowUp":
      case "k":
        e.preventDefault();
        store.moveSelection(-1);
        break;
      case " ":
        if (cur) {
          e.preventDefault();
          store.toggle(cur);
        }
        break;
      case "Enter":
        if (cur?.status === "completed") api.openFile(cur.id);
        break;
      case "Delete":
        if (cur) store.removeTarget = cur.id;
        break;
      case "Escape":
        store.selected = null;
        break;
    }
  }
</script>

<svelte:window onkeydown={onKey} />

{#if confirmToken}
  <ConfirmView token={confirmToken} />
{:else}
  <div class="shell">
    <TitleBar />
    <div class="body">
      <Sidebar />
      <main class="main">
        {#if store.page === "settings"}
          <SettingsPage />
        {:else if ready}
          <DownloadList />
        {/if}
      </main>
      {#if store.page === "main" && store.current}
        <DetailPanel />
      {/if}
    </div>
  </div>

  {#if store.addOpen}<AddDialog />{/if}
  {#if store.paletteOpen}<CommandPalette />{/if}
  {#if store.removeTarget}<RemoveDialog />{/if}
  <Toasts />
{/if}

<style>
  .shell {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .main {
    flex: 1;
    min-width: 0;
    container: main / inline-size;
    display: flex;
    flex-direction: column;
    background: var(--bg);
  }
</style>
