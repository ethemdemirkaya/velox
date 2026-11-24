<script lang="ts">
  import type { Category } from "../lib/api";
  import { ext } from "../lib/format";

  let { name, category, size = 34, live = false }: { name: string; category: Category; size?: number; live?: boolean } = $props();
  const label = $derived(ext(name) || category.slice(0, 3).toUpperCase());
</script>

<!-- A flat "file tag": extension in mono type, category shown by a corner notch. -->
<div class="fi" class:live style:width="{size}px" style:height="{size}px" data-cat={category}>
  <span class="mono">{label}</span>
  {#if live}<i></i>{/if}
</div>

<style>
  .fi {
    position: relative;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: var(--radius);
    border: 1px solid var(--border-2);
    background: var(--surface);
    color: var(--text-2);
  }
  .fi span {
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.02em;
  }
  .fi::after {
    content: "";
    position: absolute;
    top: -1px;
    right: -1px;
    width: 9px;
    height: 9px;
    border-left: 1px solid var(--border-2);
    border-bottom: 1px solid var(--border-2);
    border-bottom-left-radius: 3px;
    background: var(--bg);
  }
  .fi[data-cat="video"] span,
  .fi[data-cat="music"] span {
    color: var(--text);
  }
  .live i {
    position: absolute;
    bottom: -2px;
    right: -2px;
    width: 7px;
    height: 7px;
    background: var(--red);
    border: 2px solid var(--bg);
    box-sizing: content-box;
    border-radius: 1px;
  }
</style>
