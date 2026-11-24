<script lang="ts">
  import type { Segment } from "../lib/api";

  // A grid of cells, each representing total/N bytes. White = on disk,
  // red = being written right now, dim = pending.
  let { segments, total, done = false, active = false }: { segments: Segment[]; total: number | null; done?: boolean; active?: boolean } =
    $props();

  let canvas: HTMLCanvasElement;
  let width = $state(0);
  const CELL = 6;
  const GAP = 2;

  const cols = $derived(Math.max(1, Math.floor((width + GAP) / (CELL + GAP))));
  const rows = 9;
  const height = rows * (CELL + GAP) - GAP;

  $effect(() => {
    if (!canvas || !width) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);

    const n = cols * rows;
    const t = total ?? 0;
    // coverage[i] in 0..1, live[i] = a write head is inside the cell
    const cover = new Float32Array(n);
    const live = new Uint8Array(n);
    if (done) cover.fill(1);
    else if (t > 0 && segments.length && segments[0].end < 1e17) {
      const per = t / n;
      for (const s of segments) {
        const w = Math.min(s.written, s.end);
        let a = s.start;
        let i = Math.min(n - 1, Math.floor(a / per));
        while (a < w && i < n) {
          const b = i === n - 1 ? w : Math.min(w, (i + 1) * per);
          if (b > a) cover[i] += (b - a) / per;
          a = Math.max(a, b);
          i++;
        }
        if (active && s.written < s.end && s.written > s.start) live[Math.min(n - 1, Math.floor(s.written / per))] = 1;
      }
    } else if (t > 0) {
      const dl = segments.reduce((m, s) => Math.max(m, s.written), 0);
      const f = Math.min(1, dl / t) * n;
      for (let i = 0; i < n; i++) cover[i] = Math.max(0, Math.min(1, f - i));
    }

    for (let i = 0; i < n; i++) {
      const x = (i % cols) * (CELL + GAP);
      const y = Math.floor(i / cols) * (CELL + GAP);
      if (live[i]) ctx.fillStyle = "#ff3b30";
      else if (cover[i] >= 0.999) ctx.fillStyle = done ? "#8f8f8f" : "#e8e8e8";
      else if (cover[i] > 0) ctx.fillStyle = "#5a5a5a";
      else ctx.fillStyle = "#1d1d1d";
      ctx.fillRect(x, y, CELL, CELL);
    }
  });
</script>

<div class="map" bind:clientWidth={width} style:height="{height}px">
  <canvas bind:this={canvas} style:width="{width}px" style:height="{height}px"></canvas>
</div>

<style>
  .map {
    width: 100%;
  }
  canvas {
    display: block;
  }
</style>
