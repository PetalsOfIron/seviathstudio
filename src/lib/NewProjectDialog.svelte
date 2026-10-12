<script lang="ts">
  // "New project" window: size (pixels or print units), DPI, color mode, background and
  // what kind of first layer to start with
  import { toPixels, fromPixels, type Unit, type ColorMode } from './color';

  export type NewProjectResult = {
    width: number;
    height: number;
    paper: 'white' | 'transparent';
    dpi: number;
    colorMode: ColorMode;
    layerKind: 'raster' | 'vector';
  } | null;

  let { ondone }: { ondone: (r: NewProjectResult) => void } = $props();

  type Preset = { name: string; w: number; h: number; unit?: Unit; dpi?: number; cmyk?: boolean; group: string };
  const PRESETS: Preset[] = [
    { group: 'Vtuber', name: 'Vtuber bust', w: 3000, h: 4000 },
    { group: 'Vtuber', name: 'Vtuber half body', w: 4000, h: 6000 },
    { group: 'Vtuber', name: 'Vtuber full body', w: 5000, h: 8000 },
    { group: 'Screen', name: 'Square post', w: 2048, h: 2048 },
    { group: 'Screen', name: 'Video 1080p', w: 1920, h: 1080 },
    { group: 'Screen', name: 'Video 4K', w: 3840, h: 2160 },
    { group: 'Screen', name: 'Phone / story', w: 1080, h: 1920 },
    { group: 'Screen', name: 'Twitch panel', w: 320, h: 160 },
    { group: 'Print', name: 'A4', w: 210, h: 297, unit: 'mm', dpi: 300, cmyk: true },
    { group: 'Print', name: 'A3', w: 297, h: 420, unit: 'mm', dpi: 300, cmyk: true },
    { group: 'Print', name: 'A5', w: 148, h: 210, unit: 'mm', dpi: 300, cmyk: true },
    { group: 'Print', name: 'Business card', w: 85, h: 55, unit: 'mm', dpi: 300, cmyk: true },
    { group: 'Print', name: 'Sticker 10 cm', w: 100, h: 100, unit: 'mm', dpi: 300, cmyk: true },
    { group: 'Print', name: 'T-shirt print', w: 300, h: 400, unit: 'mm', dpi: 300, cmyk: false },
    { group: 'Print', name: 'Comic page (B4)', w: 257, h: 364, unit: 'mm', dpi: 600, cmyk: false },
    { group: 'Print', name: 'Poster 50×70', w: 500, h: 700, unit: 'mm', dpi: 150, cmyk: true },
  ];
  const GROUPS = ['Vtuber', 'Screen', 'Print'];
  const DPIS = [72, 150, 300, 350, 600];

  let group = $state('Vtuber');
  let chosen = $state('Vtuber bust');
  let unit = $state<Unit>('px');
  let w = $state(3000); // in `unit`
  let h = $state(4000);
  let dpi = $state(300);
  let colorMode = $state<ColorMode>('rgb');
  let paper = $state<'white' | 'transparent'>('white');
  let layerKind = $state<'raster' | 'vector'>('raster');

  const MAX = 12000;
  let pxW = $derived(Math.round(toPixels(w || 0, unit, dpi)));
  let pxH = $derived(Math.round(toPixels(h || 0, unit, dpi)));
  let megapixels = $derived(((pxW * pxH) / 1e6).toFixed(1));
  let tooBig = $derived(pxW * pxH > 60e6);
  let outOfRange = $derived(pxW > MAX || pxH > MAX || pxW < 16 || pxH < 16);
  let printSize = $derived(`${fromPixels(pxW, 'cm', dpi).toFixed(1)} × ${fromPixels(pxH, 'cm', dpi).toFixed(1)} cm at ${dpi} DPI`);

  const fmt = (x: number) => Math.round(x * 100) / 100;

  function choose(p: Preset) {
    chosen = p.name;
    unit = p.unit ?? 'px';
    w = p.w;
    h = p.h;
    if (p.dpi) dpi = p.dpi;
    if (p.unit) colorMode = p.cmyk ? 'cmyk' : 'rgb';
  }

  // Changing the unit keeps the same size, just written differently
  function setUnit(u: Unit) {
    const pw = toPixels(w, unit, dpi);
    const ph = toPixels(h, unit, dpi);
    unit = u;
    w = fmt(fromPixels(pw, u, dpi));
    h = fmt(fromPixels(ph, u, dpi));
  }

  function swap() {
    [w, h] = [h, w];
    chosen = '';
  }

  function create() {
    if (outOfRange) return;
    ondone({ width: pxW, height: pxH, paper, dpi: Math.round(dpi) || 72, colorMode, layerKind });
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') ondone(null);
    if (e.key === 'Enter') create();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop" role="presentation" onpointerdown={() => ondone(null)}></div>
<div class="dialog" role="dialog" aria-label="New project">
  <strong>New project</strong>

  <div class="tabs">
    {#each GROUPS as g (g)}
      <button class:on={group === g} onclick={() => (group = g)}>{g}</button>
    {/each}
  </div>
  <div class="presets">
    {#each PRESETS.filter((p) => p.group === group) as p (p.name)}
      <button class:on={chosen === p.name} onclick={() => choose(p)}>
        <span>{p.name}</span>
        <span class="dim">{p.w} × {p.h}{p.unit ? ' ' + p.unit : ''}{p.dpi ? ` · ${p.dpi} dpi` : ''}</span>
      </button>
    {/each}
  </div>

  <div class="size">
    <label>Width <input type="number" min="0" step="any" bind:value={w} oninput={() => (chosen = '')} /></label>
    <button class="swap" onclick={swap} title="Swap width and height">⇄</button>
    <label>Height <input type="number" min="0" step="any" bind:value={h} oninput={() => (chosen = '')} /></label>
    <label class="unit">
      Unit
      <select value={unit} onchange={(e) => setUnit(e.currentTarget.value as Unit)}>
        <option value="px">px</option>
        <option value="mm">mm</option>
        <option value="cm">cm</option>
        <option value="in">inch</option>
      </select>
    </label>
  </div>

  <div class="row">
    <span class="label" title="Dots per inch: how many pixels go into 2.54 cm when printed. 300 is standard for print, 72 for screens.">Resolution</span>
    <div class="seg">
      {#each DPIS as d (d)}
        <button class:on={dpi === d} onclick={() => (dpi = d)}>{d}</button>
      {/each}
    </div>
    <input class="dpi" type="number" min="1" max="2400" bind:value={dpi} title="DPI" />
    <span class="dim">dpi</span>
  </div>
  <p class="dim info">
    {pxW} × {pxH} px · {megapixels} MP · {printSize}
    {#if tooBig}<br />Very large, may be slow on weaker computers.{/if}
    {#if outOfRange}<br /><span class="warn">Each side must be 16 – {MAX} px.</span>{/if}
  </p>

  <div class="row">
    <span class="label">Color</span>
    <div class="seg two">
      <button class:on={colorMode === 'rgb'} onclick={() => (colorMode = 'rgb')} title="For screens: streaming, web, social media, vtuber models">RGB · screen</button>
      <button class:on={colorMode === 'cmyk'} onclick={() => (colorMode = 'cmyk')} title="For printing: shows how colors will look in print and exports a CMYK TIFF">CMYK · print</button>
    </div>
  </div>
  {#if colorMode === 'cmyk'}
    <p class="dim note">You draw as usual; the canvas shows how ink on paper will look (very bright colors get duller), and Export can write a CMYK TIFF for the print shop.</p>
  {/if}

  <div class="row">
    <span class="label">First layer</span>
    <div class="seg two">
      <button class:on={layerKind === 'raster'} onclick={() => (layerKind = 'raster')} title="Pixels: textures, painting, blending">Raster (pixels)</button>
      <button class:on={layerKind === 'vector'} onclick={() => (layerKind = 'vector')} title="Lines you can re-shape later: change width, cut cleanly, never blurry">Vector (lines)</button>
    </div>
  </div>

  <div class="row">
    <span class="label">Background</span>
    <div class="seg two">
      <button class:on={paper === 'white'} onclick={() => (paper = 'white')}>White paper</button>
      <button class:on={paper === 'transparent'} onclick={() => (paper = 'transparent')}>Transparent</button>
    </div>
  </div>

  <div class="buttons">
    <span class="spacer"></span>
    <button onclick={() => ondone(null)}>Cancel</button>
    <button class="primary" onclick={create} disabled={outOfRange}>Create</button>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgba(0, 0, 0, 0.45);
  }
  .dialog {
    position: fixed;
    z-index: 61;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 480px;
    max-height: calc(100vh - 40px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 18px;
    background: #24242b;
    border: 1px solid #444;
    border-radius: 12px;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.6);
    color: #eee;
    font-family: system-ui, sans-serif;
    font-size: 13px;
  }
  .tabs {
    display: flex;
    gap: 4px;
  }
  .tabs button {
    flex: 1;
  }
  .presets {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }
  .presets button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    text-align: left;
  }
  button {
    padding: 7px 10px;
    border-radius: 6px;
    border: 1px solid #444;
    background: #2b2b33;
    color: #eee;
    cursor: pointer;
    font-size: 12px;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button.on,
  button.primary {
    background: #00b5c8;
    border-color: #00b5c8;
    color: #111;
  }
  button.on .dim {
    color: #134;
  }
  .dim {
    color: #888;
    font-size: 11px;
    margin: 0;
  }
  .info {
    margin-top: -4px;
    line-height: 1.5;
  }
  .note {
    margin-top: -4px;
    line-height: 1.4;
  }
  .warn {
    color: #ff8a8a;
  }
  .size {
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }
  .size label {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: #bbb;
  }
  .size label.unit {
    flex: 0 0 78px;
  }
  input,
  select {
    background: #15151a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #eee;
    padding: 6px 8px;
    font-size: 13px;
    min-width: 0;
  }
  .swap {
    padding: 6px 10px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    color: #bbb;
  }
  .label {
    width: 78px;
    flex-shrink: 0;
  }
  .seg {
    flex: 1;
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 2px;
  }
  .seg.two {
    grid-template-columns: 1fr 1fr;
  }
  .seg button {
    padding: 6px 2px;
  }
  .dpi {
    width: 64px;
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
