<script lang="ts">
  // Pen pressure of one vector line as a graph: left = start of the line, right = end,
  // higher = thicker. Drag up or down to make that part of the line thicker or thinner.
  let {
    pts,
    onchange,
  }: {
    pts: number[]; // x, y, pressure, ...
    onchange: (pressures: number[], done: boolean) => void;
  } = $props();

  const W = 220;
  const H = 72;
  let canvas = $state<HTMLCanvasElement>();
  let dragging = false;
  let work: number[] = [];
  let frac: number[] = []; // where each point is along the line, 0..1

  // positions along the line and the pressures
  function read() {
    const n = pts.length / 3;
    const d = [0];
    for (let i = 1; i < n; i++) d.push(d[i - 1] + Math.hypot(pts[i * 3] - pts[i * 3 - 3], pts[i * 3 + 1] - pts[i * 3 - 2]));
    const total = d[n - 1] || 1;
    frac = d.map((v) => v / total);
    return Array.from({ length: n }, (_, i) => pts[i * 3 + 2]);
  }

  $effect(() => {
    if (!dragging) work = read();
    draw();
  });

  function draw() {
    if (!canvas) return;
    const ctx = canvas.getContext('2d')!;
    ctx.clearRect(0, 0, W, H);
    ctx.fillStyle = '#15151a';
    ctx.fillRect(0, 0, W, H);
    ctx.strokeStyle = '#333';
    for (const g of [0.25, 0.5, 0.75]) {
      ctx.beginPath();
      ctx.moveTo(0, H - g * H);
      ctx.lineTo(W, H - g * H);
      ctx.stroke();
    }
    // the line's width along its length, drawn as a filled shape
    ctx.fillStyle = 'rgba(0, 181, 200, 0.35)';
    ctx.strokeStyle = '#00d8ee';
    ctx.beginPath();
    ctx.moveTo(0, H);
    work.forEach((p, i) => ctx.lineTo(frac[i] * W, H - p * (H - 6) - 3));
    ctx.lineTo(W, H);
    ctx.closePath();
    ctx.fill();
    ctx.beginPath();
    work.forEach((p, i) => (i ? ctx.lineTo(frac[i] * W, H - p * (H - 6) - 3) : ctx.moveTo(0, H - p * (H - 6) - 3)));
    ctx.stroke();
  }

  function at(e: PointerEvent) {
    const r = canvas!.getBoundingClientRect();
    return { u: (e.clientX - r.left) / r.width, v: 1 - (e.clientY - r.top) / r.height };
  }

  // pulls the pressure near the pointer toward its height, softly
  function paint(e: PointerEvent) {
    const { u, v } = at(e);
    const target = Math.min(1, Math.max(0.02, v));
    work = work.map((p, i) => {
      const w = Math.exp(-(((frac[i] - u) / 0.07) ** 2));
      return p + (target - p) * w * 0.6;
    });
    draw();
    onchange(work, false);
  }

  // ready-made shapes
  function preset(kind: 'even' | 'taper' | 'middle' | 'start' | 'end') {
    work = frac.map((f) => {
      if (kind === 'even') return 1;
      if (kind === 'taper') return Math.min(1, Math.sin(Math.PI * f) * 1.6 + 0.05);
      if (kind === 'middle') return 0.25 + 0.75 * Math.sin(Math.PI * f);
      if (kind === 'start') return 1 - 0.85 * f;
      return 0.15 + 0.85 * f;
    });
    draw();
    onchange(work, true);
  }
</script>

<div class="pressure">
  <canvas
    bind:this={canvas}
    width={W}
    height={H}
    style="width: 100%; height: {H}px;"
    onpointerdown={(e) => {
      dragging = true;
      canvas!.setPointerCapture(e.pointerId);
      paint(e);
    }}
    onpointermove={(e) => dragging && paint(e)}
    onpointerup={() => {
      if (!dragging) return;
      dragging = false;
      onchange(work, true);
    }}
  ></canvas>
  <div class="presets">
    <button onclick={() => preset('even')} title="Same width everywhere">▬ Even</button>
    <button onclick={() => preset('taper')} title="Thin at both ends">◆ Taper</button>
    <button onclick={() => preset('middle')} title="Thick in the middle">⬮ Middle</button>
    <button onclick={() => preset('start')} title="Thick start, thin end">◣ Fade out</button>
    <button onclick={() => preset('end')} title="Thin start, thick end">◢ Fade in</button>
  </div>
</div>

<style>
  .pressure {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  canvas {
    border: 1px solid #444;
    border-radius: 4px;
    cursor: ns-resize;
    touch-action: none;
  }
  .presets {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 2px;
  }
  button {
    padding: 4px 2px;
    font-size: 10px;
    border-radius: 4px;
    border: 1px solid #444;
    background: #2b2b33;
    color: #eee;
    cursor: pointer;
  }
</style>
