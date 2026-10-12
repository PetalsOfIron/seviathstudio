<script lang="ts">
  // Change the colors of everything on one layer after it has been painted
  import ColorWheel from './ColorWheel.svelte';

  type Result = { before: ImageData; after: ImageData } | null;
  let {
    canvas,
    layerName,
    onchange,
    ondone,
  }: {
    canvas: HTMLCanvasElement;
    layerName: string;
    onchange: () => void; // the layer pixels changed (preview)
    ondone: (result: Result) => void; // null = cancelled
  } = $props();

  let mode = $state<'shift' | 'recolor'>('shift');
  let hue = $state(0); // -180..180 degrees
  let sat = $state(0); // -1..1
  let light = $state(0); // -1..1
  let target = $state('#3b82f6');
  let strength = $state(1); // 0..1, recolor amount

  // svelte-ignore state_referenced_locally
  const ctx = canvas.getContext('2d', { willReadFrequently: true })!;
  // svelte-ignore state_referenced_locally
  const original = ctx.getImageData(0, 0, canvas.width, canvas.height);
  const work = new ImageData(new Uint8ClampedArray(original.data), original.width, original.height);

  // Re-render the preview whenever a setting changes (at most once per frame)
  let frame = 0;
  $effect(() => {
    mode; hue; sat; light; target; strength;
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      process();
      onchange();
    });
  });

  // ---------- Color math (fast, inline) ----------

  function rgbToHsl(r: number, g: number, b: number, out: number[]) {
    r /= 255; g /= 255; b /= 255;
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const l = (max + min) / 2;
    let h = 0;
    let s = 0;
    const d = max - min;
    if (d > 0) {
      s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
      if (max === r) h = (g - b) / d + (g < b ? 6 : 0);
      else if (max === g) h = (b - r) / d + 2;
      else h = (r - g) / d + 4;
      h /= 6;
    }
    out[0] = h; out[1] = s; out[2] = l;
  }

  function hue2rgb(p: number, q: number, t: number) {
    if (t < 0) t += 1;
    if (t > 1) t -= 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  }

  function hslToRgb(h: number, s: number, l: number, out: number[]) {
    if (s === 0) {
      out[0] = out[1] = out[2] = l * 255;
      return;
    }
    const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
    const p = 2 * l - q;
    out[0] = hue2rgb(p, q, h + 1 / 3) * 255;
    out[1] = hue2rgb(p, q, h) * 255;
    out[2] = hue2rgb(p, q, h - 1 / 3) * 255;
  }

  // ---------- Processing ----------

  function process() {
    const src = original.data;
    const dst = work.data;
    const hsl = [0, 0, 0];
    const rgb = [0, 0, 0];

    // Recolor target as HSL
    const t = [0, 0, 0];
    const n = parseInt(target.replace('#', ''), 16) || 0;
    rgbToHsl((n >> 16) & 255, (n >> 8) & 255, n & 255, t);

    const dh = hue / 360;
    for (let i = 0; i < src.length; i += 4) {
      const a = src[i + 3];
      dst[i + 3] = a;
      if (a === 0) continue;
      rgbToHsl(src[i], src[i + 1], src[i + 2], hsl);

      if (mode === 'shift') {
        let h = hsl[0] + dh;
        h -= Math.floor(h);
        const s = Math.min(1, Math.max(0, hsl[1] * (1 + sat)));
        let l = hsl[2];
        l = light > 0 ? l + (1 - l) * light : l + l * light;
        hslToRgb(h, s, l, rgb);
        dst[i] = rgb[0]; dst[i + 1] = rgb[1]; dst[i + 2] = rgb[2];
      } else {
        // Keep the painting's light and shade, take hue and saturation from the new color.
        // Lightness is remapped so mid-tones match the new color: black stays black, white stays white.
        const L = hsl[2];
        const l = L <= 0.5 ? (L / 0.5) * t[2] : t[2] + ((L - 0.5) / 0.5) * (1 - t[2]);
        hslToRgb(t[0], t[1], l, rgb);
        dst[i] = src[i] + (rgb[0] - src[i]) * strength;
        dst[i + 1] = src[i + 1] + (rgb[1] - src[i + 1]) * strength;
        dst[i + 2] = src[i + 2] + (rgb[2] - src[i + 2]) * strength;
      }
    }
    ctx.putImageData(work, 0, 0);
  }

  function apply() {
    cancelAnimationFrame(frame);
    frame = 0;
    process();
    ondone({ before: original, after: ctx.getImageData(0, 0, canvas.width, canvas.height) });
  }

  function cancel() {
    cancelAnimationFrame(frame);
    frame = 0;
    ctx.putImageData(original, 0, 0);
    ondone(null);
  }

  function reset() {
    hue = 0;
    sat = 0;
    light = 0;
    strength = 1;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') cancel();
    if (e.key === 'Enter' && (e.target as HTMLElement).tagName !== 'INPUT') apply();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop" role="presentation"></div>
<div class="dialog" role="dialog" aria-label="Adjust colors">
  <div class="head">
    <strong>Adjust colors</strong>
    <span class="layer">{layerName}</span>
  </div>

  <div class="tabs">
    <button class:on={mode === 'shift'} onclick={() => (mode = 'shift')}>Shift</button>
    <button class:on={mode === 'recolor'} onclick={() => (mode = 'recolor')}>Recolor</button>
  </div>

  {#if mode === 'shift'}
    <!-- top: original colors, bottom: where they move to -->
    <div class="strip"></div>
    <div class="strip" style="filter: hue-rotate({hue}deg)"></div>
    <label>
      <span>Hue</span>
      <input type="range" min="-180" max="180" step="1" bind:value={hue} />
      <span class="val">{hue > 0 ? '+' : ''}{hue}°</span>
    </label>
    <label>
      <span>Saturation</span>
      <input type="range" min="-1" max="1" step="0.01" bind:value={sat} />
      <span class="val">{Math.round(sat * 100)}</span>
    </label>
    <label>
      <span>Lightness</span>
      <input type="range" min="-1" max="1" step="0.01" bind:value={light} />
      <span class="val">{Math.round(light * 100)}</span>
    </label>
  {:else}
    <p class="hint">Pick a new color. Shading and highlights on the layer are kept.</p>
    <ColorWheel bind:color={target} />
    <label>
      <span>Strength</span>
      <input type="range" min="0" max="1" step="0.01" bind:value={strength} />
      <span class="val">{Math.round(strength * 100)}%</span>
    </label>
  {/if}

  <div class="buttons">
    <button onclick={reset}>Reset</button>
    <span class="spacer"></span>
    <button onclick={cancel}>Cancel</button>
    <button class="primary" onclick={apply}>Apply</button>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgba(0, 0, 0, 0.15);
  }
  .dialog {
    position: fixed;
    z-index: 61;
    top: 72px;
    left: 236px;
    width: 260px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px;
    background: #24242b;
    border: 1px solid #444;
    border-radius: 10px;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.55);
    color: #eee;
    font-family: system-ui, sans-serif;
    font-size: 13px;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
  }
  .layer {
    color: #888;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tabs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px;
  }
  button {
    padding: 6px 10px;
    border-radius: 6px;
    border: 1px solid #444;
    background: #2b2b33;
    color: #eee;
    cursor: pointer;
  }
  button.on,
  button.primary {
    background: #00b5c8;
    border-color: #00b5c8;
    color: #111;
  }
  .strip {
    height: 8px;
    border-radius: 4px;
    margin-top: -4px;
    background: linear-gradient(90deg, red, yellow, lime, cyan, blue, magenta, red);
  }
  label {
    display: grid;
    grid-template-columns: 70px 1fr 40px;
    align-items: center;
    gap: 6px;
    color: #bbb;
    font-size: 12px;
  }
  input[type='range'] {
    width: 100%;
    min-width: 0;
  }
  .val {
    text-align: right;
    color: #888;
  }
  .hint {
    margin: 0;
    color: #999;
    font-size: 12px;
    line-height: 1.4;
  }
  .buttons {
    display: flex;
    gap: 6px;
    margin-top: 4px;
  }
  .spacer {
    flex: 1;
  }
</style>
