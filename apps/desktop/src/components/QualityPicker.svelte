<script lang="ts">
  import type { QualityOption } from "../lib/api";
  import { t } from "../lib/i18n.svelte";
  import { bytes } from "../lib/format";

  let { options, value = $bindable() }: { options: QualityOption[]; value: string | null } = $props();

  function label(q: QualityOption) {
    if (q.id === "audio") return t("quality.audio");
    const h = q.height ?? 0;
    return h >= 2160 ? "4K" : h >= 1440 ? "1440p" : `${h}p`;
  }
</script>

<div class="qp" role="radiogroup" aria-label={t("add.quality")}>
  {#each options as q (q.id)}
    <button type="button" role="radio" aria-checked={value === q.id} class:on={value === q.id} onclick={() => (value = q.id)}>
      <span class="l">{label(q)}</span>
      {#if q.size}<span class="s num">{bytes(q.size)}</span>{/if}
    </button>
  {/each}
</div>

<style>
  .qp {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  button {
    display: inline-flex;
    align-items: baseline;
    gap: 6px;
    height: 28px;
    padding: 0 10px;
    border-radius: var(--radius);
    border: 1px solid var(--border-2);
    background: var(--bg);
    color: var(--text-2);
    font-size: 12px;
    font-weight: 500;
    transition:
      border-color 0.1s,
      background 0.1s,
      color 0.1s;
  }
  button:hover {
    border-color: var(--border-3);
    color: var(--text);
  }
  button.on {
    background: var(--text);
    border-color: var(--text);
    color: #0a0a0a;
  }
  .s {
    font-size: 10.5px;
    font-weight: 400;
    opacity: 0.7;
  }
</style>
