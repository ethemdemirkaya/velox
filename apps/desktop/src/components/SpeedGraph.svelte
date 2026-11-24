<script lang="ts">
  import { bytes } from "../lib/format";

  let {
    data,
    height = 40,
    max = 120,
    grid = false,
    labels = false,
  }: { data: number[]; height?: number; max?: number; grid?: boolean; labels?: boolean } = $props();

  let canvas: HTMLCanvasElement;
  let width = $state(0);

  const peak = $derived(Math.max(1, ...data));

  $effect(() => {
    if (!canvas || !width) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);

    // Nice ceiling so the line doesn't hug the top.
    const top = peak * 1.35;
    const pad = 1.5;
    const h = height - pad * 2;

    if (grid) {
      ctx.strokeStyle = "#1c1c1c";
      ctx.lineWidth = 1;
      for (let i = 1; i < 4; i++) {
        const y = Math.round(pad + (h * i) / 4) + 0.5;
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(width, y);
        ctx.stroke();
      }
    }

    if (data.length < 2 || !data.some((v) => v >= 1)) return;
    const step = width / (max - 1);
    const offset = (max - data.length) * step;
    const pts = data.map((v, i) => [offset + i * step, pad + h - (v / top) * h] as const);

    ctx.beginPath();
    ctx.moveTo(pts[0][0], height);
    for (const [x, y] of pts) ctx.lineTo(x, y);
    ctx.lineTo(pts[pts.length - 1][0], height);
    ctx.closePath();
    ctx.fillStyle = "rgba(255,255,255,0.045)";
    ctx.fill();

    ctx.beginPath();
    pts.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
    ctx.strokeStyle = "#e6e6e6";
    ctx.lineWidth = 1.25;
    ctx.lineJoin = "round";
    ctx.stroke();

    // Live head (only while something is moving)
    if (data[data.length - 1] >= 1) {
      const [hx, hy] = pts[pts.length - 1];
      ctx.fillStyle = "#ff3b30";
      ctx.fillRect(hx - 2, hy - 2, 4, 4);
    }
  });
</script>

<div class="graph" bind:clientWidth={width} style:height="{height}px">
  <canvas bind:this={canvas} style:width="{width}px" style:height="{height}px"></canvas>
  {#if labels}
    <span class="peak num">{bytes(peak)}/s</span>
  {/if}
</div>

<style>
  .graph {
    position: relative;
    width: 100%;
  }
  canvas {
    display: block;
  }
  .peak {
    position: absolute;
    top: -2px;
    right: 0;
    padding: 0 0 2px 6px;
    background: var(--bg-raised);
    font-size: 10.5px;
    color: var(--text-3);
  }
</style>
