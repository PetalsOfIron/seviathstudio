<script lang="ts">
  // Draw your own brush tip. White = paint. Kaleidoscope mode repeats every mark around
  // the middle, so even quick scribbles turn into flowers, stars or snowflake tips.
  import { getTipMask, type BrushSettings } from './brush';

  let { brush, onuse }: { brush: BrushSettings; onuse: (dataUrl: string) => void } = $props();

  const N = 256;
  let view: HTMLCanvasElement;
  const ink = document.createElement('canvas'); // the tip itself: white with transparency
  ink.width = ink.height = N;
  const ictx = ink.getContext('2d')!;

  let size = $state(18);
  let soft = $state(0.3);
  let mirror = $state(1); // 1 = off, 2…8 = copies around the middle
  let erase = $state(false);
  let drawing = false;
  let last: { x: number; y: number } | null = null;
  let empty = $state(true);

  function show() {
    if (!view) return;
    const ctx = view.getContext('2d')!;
    ctx.fillStyle = '#15151a';
    ctx.fillRect(0, 0, N, N);
    // faint guide: the middle and the mirror spokes
    ctx.strokeStyle = 'rgba(255,255,255,0.08)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.arc(N / 2, N / 2, N * 0.45, 0, Math.PI * 2);
    if (mirror > 1)
      for (let k = 0; k < mirror; k++) {
        const a = (k / mirror) * Math.PI * 2 - Math.PI / 2;
        ctx.moveTo(N / 2, N / 2);
        ctx.lineTo(N / 2 + Math.cos(a) * N * 0.5, N / 2 + Math.sin(a) * N * 0.5);
      }
    ctx.stroke();
    ctx.drawImage(ink, 0, 0);
  }

  function pos(e: PointerEvent) {
    const r = view.getBoundingClientRect();
    return { x: ((e.clientX - r.left) / r.width) * N, y: ((e.clientY - r.top) / r.height) * N, p: e.pressure > 0 && e.pointerType === 'pen' ? e.pressure : 0.75 };
  }

  function dab(x: number, y: number, p: number) {
    const r = Math.max(0.6, (size * (0.35 + 0.65 * p)) / 2);
    for (let k = 0; k < mirror; k++) {
      const a = (k / mirror) * Math.PI * 2;
      const dx = x - N / 2, dy = y - N / 2;
      const px = N / 2 + dx * Math.cos(a) - dy * Math.sin(a);
      const py = N / 2 + dx * Math.sin(a) + dy * Math.cos(a);
      const g = ictx.createRadialGradient(px, py, 0, px, py, r);
      g.addColorStop(0, 'rgba(255,255,255,1)');
      g.addColorStop(Math.max(0.01, 1 - soft), 'rgba(255,255,255,1)');
      g.addColorStop(1, 'rgba(255,255,255,0)');
      ictx.fillStyle = g;
      ictx.beginPath();
      ictx.arc(px, py, r, 0, Math.PI * 2);
      ictx.fill();
    }
  }

  function line(a: { x: number; y: number }, b: { x: number; y: number; p: number }) {
    ictx.globalCompositeOperation = erase ? 'destination-out' : 'source-over';
    const d = Math.hypot(b.x - a.x, b.y - a.y);
    const step = Math.max(0.7, size * 0.12);
    for (let t = 0; t <= d; t += step) dab(a.x + ((b.x - a.x) * t) / (d || 1), a.y + ((b.y - a.y) * t) / (d || 1), b.p);
    ictx.globalCompositeOperation = 'source-over';
    empty = false;
    show();
  }

  function down(e: PointerEvent) {
    view.setPointerCapture(e.pointerId);
    drawing = true;
    const p = pos(e);
    last = p;
    line(p, p);
  }
  function move(e: PointerEvent) {
    if (!drawing || !last) return;
    const p = pos(e);
    line(last, p);
    last = p;
  }
  function up() {
    drawing = false;
    last = null;
  }

  function clear() {
    ictx.clearRect(0, 0, N, N);
    empty = true;
    show();
  }

  // Start from the brush's current tip (to edit it: erase holes, add bits)
  function fromCurrent() {
    ictx.clearRect(0, 0, N, N);
    ictx.drawImage(getTipMask(brush), 0, 0, N, N);
    empty = false;
    show();
  }

  function use() {
    if (empty) return;
    onuse(ink.toDataURL('image/png'));
  }

  $effect(() => {
    void mirror;
    show();
  });
</script>

<div class="pad">
  <canvas
    bind:this={view}
    width={N}
    height={N}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    title="Draw your tip here. White = paint."
  ></canvas>
  <div class="side">
    <div class="seg">
      <button class:on={!erase} onclick={() => (erase = false)}>✎ Draw</button>
      <button class:on={erase} onclick={() => (erase = true)}>⌫ Erase</button>
    </div>
    <label><span>Pen</span><input type="range" min="2" max="80" step="1" bind:value={size} /></label>
    <label><span>Soft</span><input type="range" min="0" max="1" step="0.01" bind:value={soft} /></label>
    <label title="Every mark is repeated around the middle: flowers, stars, snowflakes">
      <span>Kaleido</span>
      <select bind:value={mirror}>
        <option value={1}>off</option>
        {#each [2, 3, 4, 5, 6, 8, 12] as m (m)}<option value={m}>{m}×</option>{/each}
      </select>
    </label>
    <div class="seg">
      <button onclick={clear}>Clear</button>
      <button onclick={fromCurrent} title="Start from the tip the brush has now">Current tip</button>
    </div>
    <button class="use" onclick={use} disabled={empty}>✓ Use as brush tip</button>
  </div>
</div>

<style>
  .pad {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  canvas {
    width: 150px;
    height: 150px;
    align-self: center;
    border-radius: 6px;
    border: 1px solid #444;
    touch-action: none;
    cursor: crosshair;
    flex-shrink: 0;
  }
  .side {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    min-width: 0;
  }
  label {
    display: grid;
    grid-template-columns: 42px 1fr;
    align-items: center;
    gap: 4px;
    color: #bbb;
    font-size: 11px;
  }
  input[type='range'] {
    width: 100%;
    min-width: 0;
  }
  select {
    background: #15151a;
    color: #eee;
    border: 1px solid #444;
    border-radius: 4px;
    font-size: 11px;
  }
  .seg {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
  }
  button {
    padding: 3px 2px;
    font-size: 10px;
    border-radius: 5px;
    border: 1px solid #444;
    background: #2b2b33;
    color: #eee;
    cursor: pointer;
  }
  button.on {
    background: #00b5c8;
    border-color: #00b5c8;
    color: #111;
    font-weight: 600;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .use {
    padding: 5px 2px;
    font-size: 11px;
  }
</style>
