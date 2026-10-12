<script lang="ts">
  // Color wheel: hue ring + saturation/value square, with HEX, RGB, HSV fields and gray swatches
  let { color = $bindable('#222222') }: { color: string } = $props();

  const SIZE = 200;
  const C = SIZE / 2;
  const OUTER = 98;
  const INNER = 80;
  const SQ = 108; // square side, fits inside the ring
  const SQ0 = C - SQ / 2;

  let canvas = $state<HTMLCanvasElement>();
  let h = $state(0); // 0..360
  let s = $state(0); // 0..1
  let v = $state(0); // 0..1
  let lastHex = '';
  let dragging: 'ring' | 'square' | null = null;

  const grays = Array.from({ length: 11 }, (_, i) => {
    const c = Math.round((i / 10) * 255);
    return rgbToHex(c, c, c);
  });

  let rgb = $derived(hexToRgb(color) ?? [0, 0, 0]);

  // ---------- Color math ----------

  function hexToRgb(hex: string): [number, number, number] | null {
    let x = hex.trim().replace('#', '');
    if (x.length === 3) x = x.split('').map((c) => c + c).join('');
    if (!/^[0-9a-fA-F]{6}$/.test(x)) return null;
    const n = parseInt(x, 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }

  function rgbToHex(r: number, g: number, b: number) {
    return '#' + [r, g, b].map((c) => Math.round(c).toString(16).padStart(2, '0')).join('');
  }

  function hsvToRgb(h: number, s: number, v: number): [number, number, number] {
    const f = (n: number) => {
      const k = (n + h / 60) % 6;
      return v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
    };
    return [f(5) * 255, f(3) * 255, f(1) * 255];
  }

  function rgbToHsv(r: number, g: number, b: number) {
    r /= 255; g /= 255; b /= 255;
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const d = max - min;
    let hue = 0;
    if (d !== 0) {
      if (max === r) hue = ((g - b) / d) % 6;
      else if (max === g) hue = (b - r) / d + 2;
      else hue = (r - g) / d + 4;
      hue *= 60;
      if (hue < 0) hue += 360;
    }
    return { h: hue, s: max === 0 ? 0 : d / max, v: max };
  }

  const clamp = (x: number, lo = 0, hi = 1) => Math.min(hi, Math.max(lo, x));

  // ---------- Sync ----------

  // When the color changes from outside (e.g. the picker tool), update the wheel
  $effect(() => {
    const hex = color.toLowerCase();
    if (hex === lastHex) return;
    const c = hexToRgb(hex);
    if (!c) return;
    const hsv = rgbToHsv(...c);
    if (hsv.s > 0 && hsv.v > 0) h = hsv.h; // grays have no hue, so keep the current one
    s = hsv.s;
    v = hsv.v;
    lastHex = hex;
  });

  function setHsv(nh: number, ns: number, nv: number) {
    h = ((nh % 360) + 360) % 360;
    s = clamp(ns);
    v = clamp(nv);
    const hex = rgbToHex(...hsvToRgb(h, s, v));
    lastHex = hex;
    color = hex;
  }

  function setHex(hex: string) {
    const c = hexToRgb(hex);
    if (c) color = rgbToHex(...c);
  }

  function setRgb(i: number, value: number) {
    const c: [number, number, number] = [rgb[0], rgb[1], rgb[2]];
    c[i] = clamp(Math.round(value || 0), 0, 255);
    color = rgbToHex(...c);
  }

  // ---------- Drawing ----------

  $effect(() => {
    draw(h, s, v);
  });

  function draw(hue: number, sat: number, val: number) {
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    if (canvas.width !== SIZE * dpr) {
      canvas.width = SIZE * dpr;
      canvas.height = SIZE * dpr;
    }
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, SIZE, SIZE);

    // Hue ring (hue grows clockwise, red on the right)
    const ring = ctx.createConicGradient(0, C, C);
    for (let i = 0; i <= 6; i++) ring.addColorStop(i / 6, `hsl(${i * 60}, 100%, 50%)`);
    ctx.fillStyle = ring;
    ctx.beginPath();
    ctx.arc(C, C, OUTER, 0, Math.PI * 2);
    ctx.arc(C, C, INNER, 0, Math.PI * 2, true);
    ctx.fill();

    // Saturation / value square
    ctx.fillStyle = `hsl(${hue}, 100%, 50%)`;
    ctx.fillRect(SQ0, SQ0, SQ, SQ);
    const white = ctx.createLinearGradient(SQ0, 0, SQ0 + SQ, 0);
    white.addColorStop(0, '#fff');
    white.addColorStop(1, 'rgba(255,255,255,0)');
    ctx.fillStyle = white;
    ctx.fillRect(SQ0, SQ0, SQ, SQ);
    const black = ctx.createLinearGradient(0, SQ0, 0, SQ0 + SQ);
    black.addColorStop(0, 'rgba(0,0,0,0)');
    black.addColorStop(1, '#000');
    ctx.fillStyle = black;
    ctx.fillRect(SQ0, SQ0, SQ, SQ);

    // Markers
    const a = (hue * Math.PI) / 180;
    const rr = (OUTER + INNER) / 2;
    marker(ctx, C + Math.cos(a) * rr, C + Math.sin(a) * rr);
    marker(ctx, SQ0 + sat * SQ, SQ0 + (1 - val) * SQ);
  }

  function marker(ctx: CanvasRenderingContext2D, x: number, y: number) {
    ctx.lineWidth = 3;
    ctx.strokeStyle = '#000';
    ctx.beginPath();
    ctx.arc(x, y, 6, 0, Math.PI * 2);
    ctx.stroke();
    ctx.lineWidth = 1.5;
    ctx.strokeStyle = '#fff';
    ctx.stroke();
  }

  // ---------- Pointer ----------

  function pos(e: PointerEvent) {
    const r = canvas!.getBoundingClientRect();
    return { x: ((e.clientX - r.left) * SIZE) / r.width, y: ((e.clientY - r.top) * SIZE) / r.height };
  }

  function down(e: PointerEvent) {
    const p = pos(e);
    const d = Math.hypot(p.x - C, p.y - C);
    if (d >= INNER - 4 && d <= OUTER + 4) dragging = 'ring';
    else if (Math.abs(p.x - C) <= SQ / 2 + 4 && Math.abs(p.y - C) <= SQ / 2 + 4) dragging = 'square';
    else return;
    canvas!.setPointerCapture(e.pointerId);
    update(p);
  }

  function update(p: { x: number; y: number }) {
    if (dragging === 'ring') {
      let a = (Math.atan2(p.y - C, p.x - C) * 180) / Math.PI;
      if (a < 0) a += 360;
      setHsv(a, s, v);
    } else if (dragging === 'square') {
      setHsv(h, (p.x - SQ0) / SQ, 1 - (p.y - SQ0) / SQ);
    }
  }
</script>

<div class="color-panel">
  <canvas
    bind:this={canvas}
    style="width: {SIZE}px; height: {SIZE}px;"
    onpointerdown={down}
    onpointermove={(e) => dragging && update(pos(e))}
    onpointerup={() => (dragging = null)}
    onpointercancel={() => (dragging = null)}
  ></canvas>

  <div class="row">
    <div class="swatch" style="background: {color}"></div>
    <label class="hex">
      HEX
      <input type="text" value={color} onchange={(e) => setHex(e.currentTarget.value)} maxlength="7" spellcheck="false" />
    </label>
  </div>

  <div class="fields">
    <span>R</span>
    <input type="number" min="0" max="255" value={rgb[0]} onchange={(e) => setRgb(0, +e.currentTarget.value)} />
    <span>G</span>
    <input type="number" min="0" max="255" value={rgb[1]} onchange={(e) => setRgb(1, +e.currentTarget.value)} />
    <span>B</span>
    <input type="number" min="0" max="255" value={rgb[2]} onchange={(e) => setRgb(2, +e.currentTarget.value)} />

    <span>H</span>
    <input type="number" min="0" max="360" value={Math.round(h)} onchange={(e) => setHsv(+e.currentTarget.value || 0, s, v)} />
    <span>S</span>
    <input type="number" min="0" max="100" value={Math.round(s * 100)} onchange={(e) => setHsv(h, (+e.currentTarget.value || 0) / 100, v)} />
    <span>V</span>
    <input type="number" min="0" max="100" value={Math.round(v * 100)} onchange={(e) => setHsv(h, s, (+e.currentTarget.value || 0) / 100)} />
  </div>

  <div class="grays" title="Grayscale">
    {#each grays as g (g)}
      <button class="gray" class:on={color === g} style="background: {g}" title={g} aria-label={g} onclick={() => (color = g)}></button>
    {/each}
  </div>
</div>

<style>
  .color-panel {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }
  canvas {
    touch-action: none;
    cursor: crosshair;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
  }
  .swatch {
    width: 36px;
    height: 28px;
    border-radius: 6px;
    border: 1px solid #555;
    flex-shrink: 0;
  }
  .hex {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    color: #aaa;
    font-size: 12px;
  }
  .hex input {
    flex: 1;
    width: 100%;
    min-width: 0;
    box-sizing: border-box;
    font-family: ui-monospace, monospace;
  }
  .fields {
    display: grid;
    grid-template-columns: auto 1fr auto 1fr auto 1fr;
    gap: 4px 6px;
    align-items: center;
    width: 100%;
    font-size: 12px;
    color: #aaa;
  }
  input {
    background: #15151a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #eee;
    padding: 4px 6px;
    font-size: 12px;
    min-width: 0;
  }
  .fields input {
    width: 100%;
    box-sizing: border-box;
  }
  .grays {
    display: flex;
    width: 100%;
    gap: 2px;
  }
  .gray {
    flex: 1;
    height: 18px;
    padding: 0;
    border: 1px solid #444;
    border-radius: 3px;
    cursor: pointer;
  }
  .gray.on {
    outline: 2px solid #00b5c8;
    outline-offset: 1px;
  }
</style>
