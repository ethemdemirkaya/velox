<script lang="ts">
  let { checked, onchange }: { checked: boolean; onchange: (v: boolean) => void } = $props();
</script>

<button class="tg" class:on={checked} role="switch" aria-checked={checked} onclick={() => onchange(!checked)} aria-label="toggle">
  <span></span>
</button>

<style>
  .tg {
    flex: none;
    width: 34px;
    height: 20px;
    padding: 2px;
    border-radius: 10px;
    border: 1px solid var(--border-3);
    background: var(--surface);
    transition:
      background 0.15s var(--ease),
      border-color 0.15s var(--ease);
  }
  .tg span {
    display: block;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--text-3);
    transition:
      transform 0.18s var(--ease-out),
      background 0.15s;
  }
  .tg.on {
    background: var(--text);
    border-color: var(--text);
  }
  .tg.on span {
    transform: translateX(14px);
    background: #0a0a0a;
  }
</style>
