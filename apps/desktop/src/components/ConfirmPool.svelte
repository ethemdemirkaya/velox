<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import ConfirmView from "./ConfirmView.svelte";

  // The pre-built confirm window: idle until the app hands it a download.
  let token = $state<string | null>(null);

  onMount(() => {
    const w = getCurrentWebviewWindow();
    const un = w.listen<string>("confirm-load", (e) => (token = e.payload));
    return () => {
      un.then((f) => f());
    };
  });
</script>

{#key token}
  {#if token}
    <ConfirmView {token} onclose={() => (token = null)} />
  {/if}
{/key}
