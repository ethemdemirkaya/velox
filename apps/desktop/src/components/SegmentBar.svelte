<script lang="ts">
  import type { Segment } from "../lib/api";

  // Draws each connection's range: committed bytes in white, the live
  // write head of active connections as a red tick, remaining bytes as track.
  let {
    segments,
    total,
    downloaded,
    height = 3,
    active = false,
    done = false,
    gap = false,
  }: {
    segments: Segment[];
    total: number | null;
    downloaded: number;
    height?: number;
    active?: boolean;
    done?: boolean;
    gap?: boolean;
  } = $props();

  let canvas: HTMLCanvasElement;
  let width = $state(0);

  $effect(() => {
    if (!canvas || !width) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = "#1e1e1e";
    ctx.fillRect(0, 0, width, height);

    const fill = active ? "#ededed" : done ? "#8f8f8f" : "#6f6f6f";
    if (done) {
      ctx.fillStyle = fill;
      ctx.fillRect(0, 0, width, height);
      return;
    }
    const t = total ?? 0;
    if (!t || !segments.length || segments[0].end > 1e17) {
      // Unknown size or single stream: plain proportional bar.
      ctx.fillStyle = fill;
      if (t) ctx.fillRect(0, 0, (Math.min(downloaded, t) / t) * width, height);
      return;
    }
    const sx = width / t;
    for (const s of segments) {
      const x0 = s.start * sx;
      const x1 = Math.min(s.written, s.end) * sx;
      if (x1 > x0) {
        ctx.fillStyle = fill;
        ctx.fillRect(x0, 0, Math.max(x1 - x0, 0.5), height);
      }
      if (active && s.written < s.end && s.written > s.start) {
        ctx.fillStyle = "#ff3b30";
        ctx.fillRect(Math.max(0, x1 - 1), 0, 2, height);
      }
      if (gap && s.start > 0) {
        ctx.fillStyle = "#0a0a0a";
        ctx.fillRect(Math.round(x0), 0, 1, height);
      }
    }
  });
</script>

<div class="bar" bind:clientWidth={width} style:height="{height}px">
  <canvas bind:this={canvas} style:width="{width}px" style:height="{height}px"></canvas>
</div>

<style>
  .bar {
    width: 100%;
    overflow: hidden;
    border-radius: 1px;
  }
  canvas {
    display: block;
  }
</style>
