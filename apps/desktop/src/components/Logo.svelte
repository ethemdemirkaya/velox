<script lang="ts">
  let { size = 18 }: { size?: number } = $props();
</script>

<!-- Velox mark: a flat white V with the red "drop" beneath it. -->
<svg width={size} height={size} viewBox="0 0 24 24" aria-label="Velox">
  <path d="M3 3h4.1L12 12.4 16.9 3H21L12 18.6z" fill="var(--text)" />
  <rect x="10.6" y="19.6" width="2.8" height="2.8" fill="var(--red)" />
</svg>
