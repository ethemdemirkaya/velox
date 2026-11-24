<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import { store } from "../lib/store.svelte";
  import { t } from "../lib/i18n.svelte";

  const win = getCurrentWindow();
  let maximized = $state(false);

  $effect(() => {
    win.isMaximized().then((m) => (maximized = m));
    const un = win.onResized(async () => (maximized = await win.isMaximized()));
    return () => {
      un.then((f) => f());
    };
  });
</script>

<header class="bar" data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <Logo size={16} />
    <span class="name" data-tauri-drag-region>Velox</span>
  </div>

  <div class="center" data-tauri-drag-region>
    <label class="search">
      <Icon name="search" size={14} />
      <input id="search" placeholder={t("search.placeholder")} bind:value={store.query} spellcheck="false" autocomplete="off" />
      {#if store.query}
        <button class="clear" onclick={() => (store.query = "")} aria-label="Clear"><Icon name="x" size={12} /></button>
      {:else}
        <button class="hint" onclick={() => (store.paletteOpen = true)} tabindex="-1">
          <span class="kbd">Ctrl</span><span class="kbd">K</span>
        </button>
      {/if}
    </label>
  </div>

  <div class="controls">
    <button class="ctl" onclick={() => win.minimize()} aria-label="Minimize"><Icon name="min" size={14} /></button>
    <button class="ctl" onclick={() => win.toggleMaximize()} aria-label="Maximize">
      {#if maximized}
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"
          ><path d="M8 8h11v11H8zM5 16V5h11" /></svg
        >
      {:else}
        <Icon name="max" size={13} />
      {/if}
    </button>
    <button class="ctl close" onclick={() => win.close()} aria-label="Close"><Icon name="x" size={14} /></button>
  </div>
</header>

<style>
  .bar {
    height: var(--titlebar);
    flex: none;
    display: grid;
    grid-template-columns: var(--sidebar) 1fr auto;
    align-items: center;
    border-bottom: 1px solid var(--border);
    background: var(--bg);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
    padding-left: 16px;
    height: 100%;
  }
  .name {
    font-weight: 600;
    letter-spacing: -0.01em;
    font-size: 13px;
  }
  .center {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0 16px;
  }
  .search {
    width: min(440px, 100%);
    height: 28px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 6px 0 10px;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text-3);
    transition: border-color 0.12s var(--ease);
  }
  .search:hover {
    border-color: var(--border-2);
  }
  .search:focus-within {
    border-color: var(--border-3);
    color: var(--text-2);
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: none;
    border: 0;
    outline: none;
    color: var(--text);
    font-size: 12.5px;
  }
  .search input::placeholder {
    color: var(--text-3);
  }
  .hint {
    display: flex;
    gap: 3px;
  }
  .clear {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 4px;
    color: var(--text-3);
  }
  .clear:hover {
    background: var(--hover);
    color: var(--text);
  }
  .controls {
    display: flex;
    height: 100%;
  }
  .ctl {
    width: 46px;
    height: 100%;
    display: grid;
    place-items: center;
    color: var(--text-2);
    transition: background 0.1s;
  }
  .ctl:hover {
    background: var(--surface-3);
    color: var(--text);
  }
  .ctl.close:hover {
    background: var(--red);
    color: #fff;
  }
</style>
