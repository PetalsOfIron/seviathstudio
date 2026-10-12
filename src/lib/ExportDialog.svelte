<script lang="ts">
  // "Export" window: pick a file type and a few options
  import { FORMATS, type ExportOptions, type ExportFormat } from './export';

  let {
    width,
    height,
    dpi = 72,
    colorMode = 'rgb',
    initial,
    ondone,
  }: {
    width: number;
    height: number;
    dpi?: number;
    colorMode?: 'rgb' | 'cmyk';
    initial: ExportOptions;
    ondone: (r: ExportOptions | null) => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let opts = $state<ExportOptions>({ ...initial });
  let info = $derived(FORMATS.find((f) => f.id === opts.format)!);
  let outW = $derived(Math.max(1, Math.round(width * opts.scale)));
  let outH = $derived(Math.max(1, Math.round(height * opts.scale)));

  function choose(id: ExportFormat) {
    opts.format = id;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') ondone(null);
    if (e.key === 'Enter') ondone({ ...opts });
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop" role="presentation" onpointerdown={() => ondone(null)}></div>
<div class="dialog" role="dialog" aria-label="Export">
  <strong>Export</strong>

  <div class="sub">With layers</div>
  <div class="formats">
    {#each FORMATS.filter((f) => f.layers) as f (f.id)}
      <button class:on={opts.format === f.id} onclick={() => choose(f.id)}>{f.label}</button>
    {/each}
  </div>
  <div class="sub">One flat picture</div>
  <div class="formats">
    {#each FORMATS.filter((f) => !f.layers) as f (f.id)}
      <button class:on={opts.format === f.id} onclick={() => choose(f.id)}>{f.label}</button>
    {/each}
  </div>
  <p class="note">{info.note}</p>

  {#if !info.layers}
    <label>
      <span>Size</span>
      <input type="range" min="0.1" max="2" step="0.05" bind:value={opts.scale} />
      <span class="val">{Math.round(opts.scale * 100)}%</span>
    </label>
    <p class="dim">
      {opts.format === 'ico'
        ? 'Icon sizes: 16, 32, 48, 256 px'
        : `${outW} × ${outH} px · ${Math.round(dpi * opts.scale)} dpi · ${((width / dpi) * 2.54).toFixed(1)} × ${((height / dpi) * 2.54).toFixed(1)} cm`}
    </p>
    {#if info.lossy}
      <label>
        <span>Quality</span>
        <input type="range" min="0.3" max="1" step="0.01" bind:value={opts.quality} />
        <span class="val">{Math.round(opts.quality * 100)}%</span>
      </label>
    {/if}
    {#if info.alpha}
      <label class="check"><input type="checkbox" bind:checked={opts.transparent} /> Transparent background (leave the paper out)</label>
    {/if}
    {#if colorMode === 'cmyk'}
      {#if opts.format === 'tiff'}
        <label class="check" title="Real ink values for the print shop (approximate press profile)"><input type="checkbox" bind:checked={opts.cmyk} /> CMYK colors (for print)</label>
      {/if}
      {#if !(opts.format === 'tiff' && opts.cmyk)}
        <label class="check" title="Bright screen colors become the duller colors ink can print, as shown on the canvas"><input type="checkbox" bind:checked={opts.printColors} /> Save colors as they will print</label>
      {/if}
    {/if}
  {/if}

  <div class="buttons">
    <span class="spacer"></span>
    <button onclick={() => ondone(null)}>Cancel</button>
    <button class="primary" onclick={() => ondone({ ...opts })}>Export {info.label}</button>
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
    width: 420px;
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
  .sub {
    color: #999;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: -4px;
  }
  .formats {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 4px;
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
  button.on,
  button.primary {
    background: #00b5c8;
    border-color: #00b5c8;
    color: #111;
    font-weight: 600;
  }
  .note {
    margin: 0;
    color: #bbb;
    line-height: 1.4;
    min-height: 34px;
  }
  label {
    display: grid;
    grid-template-columns: 60px 1fr 46px;
    align-items: center;
    gap: 6px;
    color: #bbb;
  }
  label.check {
    display: flex;
  }
  .val {
    text-align: right;
    color: #888;
  }
  .dim {
    color: #888;
    font-size: 11px;
    margin: -4px 0 0 66px;
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
