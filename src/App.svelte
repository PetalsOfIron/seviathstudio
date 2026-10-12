<script lang="ts">
  import { onMount } from 'svelte';

  // Otas iestatījumi
  let color = $state('#222222');
  let size = $state(12);
  let canvas = $state<HTMLCanvasElement>();

  let ctx: CanvasRenderingContext2D;
  let drawing = false;
  let last = { x: 0, y: 0, p: 0.5 };

  onMount(() => {
    if (!canvas) return;
    ctx = canvas.getContext('2d')!;
    resize();
    window.addEventListener('resize', resize);
    return () => window.removeEventListener('resize', resize);
  });

  // Audekls aizpilda logu un ir ass arī augstas izšķirtspējas ekrānos
  function resize() {
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    const old = document.createElement('canvas');
    old.width = canvas.width;
    old.height = canvas.height;
    old.getContext('2d')!.drawImage(canvas, 0, 0);

    canvas.width = window.innerWidth * dpr;
    canvas.height = (window.innerHeight - 56) * dpr;
    canvas.style.width = window.innerWidth + 'px';
    canvas.style.height = window.innerHeight - 56 + 'px';

    ctx.fillStyle = '#ffffff';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(old, 0, 0);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  // Pele nedod spiedienu, tāpēc tai izmanto 0.5
  function pressureOf(e: PointerEvent) {
    if (e.pointerType === 'mouse') return 0.5;
    return e.pressure || 0.01;
  }

  function point(e: PointerEvent) {
    const r = canvas!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top, p: pressureOf(e) };
  }

  function down(e: PointerEvent) {
    canvas!.setPointerCapture(e.pointerId);
    drawing = true;
    last = point(e);
    stamp(last);
  }

  function move(e: PointerEvent) {
    if (!drawing) return;
    // Coalesced events dod visus punktus starp kadriem, līnija ir gludāka
    const events = e.getCoalescedEvents ? e.getCoalescedEvents() : [e];
    for (const ev of events) {
      const next = point(ev);
      segment(last, next);
      last = next;
    }
  }

  function up() {
    drawing = false;
  }

  function stamp(pt: { x: number; y: number; p: number }) {
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(pt.x, pt.y, (size * pt.p) / 2, 0, Math.PI * 2);
    ctx.fill();
  }

  // Starp diviem punktiem uzliek apļus ar pakāpenisku izmēru, lai biezums mainās gludi
  function segment(a: typeof last, b: typeof last) {
    const dist = Math.hypot(b.x - a.x, b.y - a.y);
    const step = Math.max(1, (size * Math.min(a.p, b.p)) / 6);
    const n = Math.ceil(dist / step);
    for (let i = 1; i <= n; i++) {
      const t = i / n;
      stamp({
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        p: a.p + (b.p - a.p) * t,
      });
    }
  }

  function clearCanvas() {
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(0, 0, canvas!.width, canvas!.height);
    ctx.restore();
  }
</script>

<div class="toolbar">
  <strong>SeviathStudio</strong>
  <label>Krāsa <input type="color" bind:value={color} /></label>
  <label>Izmērs <input type="range" min="1" max="100" bind:value={size} /> {size}px</label>
  <button onclick={clearCanvas}>Notīrīt</button>
</div>

<canvas
  bind:this={canvas}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
></canvas>

<style>
  .toolbar {
    height: 56px;
    display: flex;
    align-items: center;
    gap: 20px;
    padding: 0 16px;
    background: #1e1e24;
    color: #eee;
    font-family: system-ui, sans-serif;
    font-size: 14px;
  }
  label {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  button {
    padding: 6px 12px;
    border-radius: 6px;
    border: none;
    cursor: pointer;
  }
  canvas {
    display: block;
    touch-action: none; /* lai planšete un skārienekrāns zīmē, nevis ritina lapu */
    cursor: crosshair;
  }
</style>
