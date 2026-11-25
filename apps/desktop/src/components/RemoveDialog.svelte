<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { t } from "../lib/i18n.svelte";

  const ids = $derived((store.removeTarget ?? []).filter((id) => store.byId[id]));
  const d = $derived(ids.length === 1 ? store.byId[ids[0]] : null);
  let alsoDelete = $state(false);
  let btn = $state<HTMLButtonElement>();

  onMount(() => btn?.focus());

  function close() {
    store.removeTarget = null;
  }
  async function confirm() {
    const list = ids;
    close();
    await Promise.all(list.map((id) => api.remove(id, alsoDelete)));
    store.clearSelection();
  }
</script>

{#if ids.length}
  <div class="overlay" role="presentation" onmousedown={(e) => e.target === e.currentTarget && close()}>
    <div class="modal" role="alertdialog" aria-modal="true" tabindex="-1" onkeydown={(e) => e.key === "Escape" && close()}>
      <div class="content">
        {#if d}
          <h3>{t("remove.title")}</h3>
          <p class="muted">{t("remove.body", { name: d.fileName })}</p>
        {:else}
          <h3>{t("remove.titleMany", { n: ids.length })}</h3>
          <p class="muted">{t("remove.bodyMany")}</p>
        {/if}
        <label class="check">
          <input type="checkbox" bind:checked={alsoDelete} />
          <span>{d ? t("remove.alsoDelete") : t("remove.alsoDeleteMany")}</span>
        </label>
      </div>
      <footer>
        <button class="btn ghost" onclick={close}>{t("action.cancel")}</button>
        <button class="btn danger" class:solid={alsoDelete} bind:this={btn} onclick={confirm}>
          {alsoDelete ? t("action.delete") : t("action.remove")}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .modal {
    width: min(420px, calc(100vw - 32px));
  }
  .content {
    padding: 20px 20px 16px;
  }
  h3 {
    margin: 0 0 6px;
    font-size: 14px;
    font-weight: 600;
  }
  p {
    margin: 0 0 16px;
    word-break: break-word;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 9px;
    color: var(--text-2);
  }
  .check input {
    accent-color: var(--red);
    width: 14px;
    height: 14px;
    margin: 0;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    padding: 12px 16px;
    border-top: 1px solid var(--border);
    background: var(--bg);
  }
</style>
