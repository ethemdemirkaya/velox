<script lang="ts">
  import Icon from "./Icon.svelte";
  import { toasts } from "../lib/toasts.svelte";
</script>

<div class="stack" aria-live="polite">
  {#each toasts.items as t (t.id)}
    <div class="toast" data-kind={t.kind}>
      <span class="ic">
        {#if t.kind === "success"}<Icon name="check" size={14} stroke={2} />{:else if t.kind === "error"}<Icon name="alert" size={14} />{:else}<Icon
            name="info"
            size={14}
          />{/if}
      </span>
      <div class="txt">
        <div class="title">{t.title}</div>
        {#if t.body}<div class="body">{t.body}</div>{/if}
      </div>
      {#if t.action}
        <button
          class="btn sm"
          onclick={() => {
            t.action!.run();
            toasts.dismiss(t.id);
          }}>{t.action.label}</button
        >
      {/if}
      <button class="x" onclick={() => toasts.dismiss(t.id)} aria-label="Dismiss"><Icon name="x" size={12} /></button>
    </div>
  {/each}
</div>

<style>
  .stack {
    position: fixed;
    right: 16px;
    bottom: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 100;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    width: 340px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 10px 10px 12px;
    background: var(--surface-2);
    border: 1px solid var(--border-2);
    border-radius: var(--radius-l);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
    animation: pop 0.2s var(--ease-out);
  }
  .ic {
    width: 22px;
    height: 22px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 50%;
    border: 1px solid var(--border-3);
    color: var(--text);
  }
  [data-kind="error"] .ic {
    border-color: var(--red);
    color: var(--red);
  }
  .txt {
    flex: 1;
    min-width: 0;
  }
  .title {
    font-weight: 500;
    font-size: 12.5px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .body {
    font-size: 11.5px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .x {
    width: 20px;
    height: 20px;
    display: grid;
    place-items: center;
    border-radius: 4px;
    color: var(--text-3);
  }
  .x:hover {
    background: var(--hover);
    color: var(--text);
  }
</style>
