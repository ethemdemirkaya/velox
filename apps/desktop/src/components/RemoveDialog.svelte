<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { store } from "../lib/store.svelte";
  import { t } from "../lib/i18n.svelte";

  const d = $derived(store.removeTarget ? store.byId[store.removeTarget] : null);
  let alsoDelete = $state(false);
  let btn = $state<HTMLButtonElement>();

  onMount(() => btn?.focus());

  function close() {
    store.removeTarget = null;
  }
  async function confirm() {
    if (d) await api.remove(d.id, alsoDelete);
    close();
  }
</script>

{#if d}
  <div class="overlay" role="presentation" onmousedown={(e) => e.target === e.currentTarget && close()}>
    <div class="modal" role="alertdialog" aria-modal="true" tabindex="-1" onkeydown={(e) => e.key === "Escape" && close()}>
      <div class="content">
        <h3>{t("remove.title")}</h3>
        <p class="muted">{t("remove.body", { name: d.fileName })}</p>
        <label class="check">
          <input type="checkbox" bind:checked={alsoDelete} />
          <span>{t("remove.alsoDelete")}</span>
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
