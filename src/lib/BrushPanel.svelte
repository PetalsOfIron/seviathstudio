<script lang="ts">
  import { PRESETS, settingsOf, type BrushSettings, type BrushPreset } from './brush';

  let { brush = $bindable(), preset = $bindable() }: { brush: BrushSettings; preset: string } = $props();

  function choose(p: BrushPreset) {
    preset = p.name;
    Object.assign(brush, settingsOf(p));
  }

  const pct = (x: number) => Math.round(x * 100) + '%';

  // Brush size uses a logarithmic slider: 0.5 px … 500 px, so small sizes are easy to hit
  const MIN = 0.5;
  const MAX = 500;
  const QUICK_SIZES = [0.5, 1, 1.5, 2, 3, 4, 6, 10];

  function clampSize(x: number) {
    if (!isFinite(x)) return MIN;
    x = Math.min(MAX, Math.max(MIN, x));
    return roundSize(x);
  }
  // 0.1 px steps under 10 px, 0.5 px up to 50 px, whole pixels above
  function roundSize(x: number) {
    if (x < 10) return Math.round(x * 10) / 10;
    if (x < 50) return Math.round(x * 2) / 2;
    return Math.round(x);
  }
  const toSlider = (size: number) => Math.log(size / MIN) / Math.log(MAX / MIN);
  const fromSlider = (t: number) => clampSize(MIN * Math.pow(MAX / MIN, t));
</script>

<div class="brush-panel">
  <div class="presets">
    {#each PRESETS as p (p.name)}
      <button class:on={preset === p.name} onclick={() => choose(p)}>{p.name}</button>
    {/each}
  </div>

  <label>
    <span>Brush size</span>
    <input
      type="range"
      min="0"
      max="1"
      step="0.001"
      value={toSlider(brush.size)}
      oninput={(e) => (brush.size = fromSlider(+e.currentTarget.value))}
      title="Small sizes take up half of the slider"
    />
    <input
      class="num"
      type="number"
      min="0.5"
      max="500"
      step="0.1"
      value={brush.size}
      onchange={(e) => (brush.size = clampSize(+e.currentTarget.value))}
    />
  </label>
  <div class="quick">
    {#each QUICK_SIZES as q (q)}
      <button class:on={brush.size === q} onclick={() => (brush.size = q)}>{q}</button>
    {/each}
  </div>
  <label>
    <span>Opacity</span>
    <input type="range" min="0.01" max="1" step="0.01" bind:value={brush.opacity} />
    <span class="val">{pct(brush.opacity)}</span>
  </label>
  <label>
    <span>Flow</span>
    <input type="range" min="0.01" max="1" step="0.01" bind:value={brush.flow} />
    <span class="val">{pct(brush.flow)}</span>
  </label>
  <label>
    <span>Hardness</span>
    <input type="range" min="0" max="1" step="0.01" bind:value={brush.hardness} />
    <span class="val">{pct(brush.hardness)}</span>
  </label>
  <label>
    <span>Spacing</span>
    <input type="range" min="0.02" max="1" step="0.01" bind:value={brush.spacing} />
    <span class="val">{pct(brush.spacing)}</span>
  </label>
  <label title="0% = clean paint, 100% = only blends the colors already on the layer">
    <span>Color mix</span>
    <input type="range" min="0" max="1" step="0.01" bind:value={brush.mix} />
    <span class="val">{pct(brush.mix)}</span>
  </label>
  <label>
    <span>Paper texture</span>
    <input type="range" min="0" max="1" step="0.01" bind:value={brush.texture} />
    <span class="val">{pct(brush.texture)}</span>
  </label>
  <label>
    <span>Min size</span>
    <input type="range" min="0" max="1" step="0.01" bind:value={brush.minSize} disabled={!brush.sizePressure} />
    <span class="val">{pct(brush.minSize)}</span>
  </label>
  <label>
    <span>Stabilizer</span>
    <input type="range" min="0" max="1" step="0.01" bind:value={brush.stabilize} />
    <span class="val">{pct(brush.stabilize)}</span>
  </label>

  <div class="ends">
    <span>Line ends</span>
    <div class="seg">
      <button class:on={brush.ends === 'round'} onclick={() => (brush.ends = 'round')} title="Round ends">◖ Round</button>
      <button class:on={brush.ends === 'taper'} onclick={() => (brush.ends = 'taper')} title="Pointed ends on both sides">◁ Tapered</button>
      <button class:on={brush.ends === 'flat'} onclick={() => (brush.ends = 'flat')} title="Straight cut ends">▌ Flat</button>
    </div>
  </div>
  {#if brush.ends === 'taper'}
    <label>
      <span>Taper length</span>
      <input type="range" min="0.05" max="1" step="0.01" bind:value={brush.taper} />
      <span class="val">{pct(brush.taper)}</span>
    </label>
  {/if}

  <div class="checks">
    <label class="check"><input type="checkbox" bind:checked={brush.sizePressure} /> Pressure → size</label>
    <label class="check"><input type="checkbox" bind:checked={brush.opacityPressure} /> Pressure → opacity</label>
  </div>

  <p class="hint">Shift + click: straight line from the last point · [ ] change size</p>
</div>

<style>
  .brush-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12px;
  }
  .presets {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px;
    margin-bottom: 4px;
  }
  .presets button {
    padding: 6px 4px;
    font-size: 12px;
  }
  label {
    display: grid;
    grid-template-columns: 76px 1fr 46px;
    align-items: center;
    gap: 6px;
    color: #bbb;
  }
  input[type='range'] {
    width: 100%;
    min-width: 0;
  }
  .num {
    width: 100%;
    box-sizing: border-box;
    background: #15151a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #eee;
    padding: 2px 4px;
    font-size: 12px;
  }
  .quick {
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    gap: 2px;
    margin-top: -2px;
  }
  .quick button {
    padding: 3px 0;
    font-size: 11px;
  }
  .val {
    text-align: right;
    color: #888;
  }
  .ends {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: #bbb;
    margin-top: 2px;
  }
  .seg {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 2px;
  }
  .seg button {
    padding: 5px 2px;
    font-size: 11px;
  }
  .checks {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 4px;
  }
  .check {
    display: flex;
    gap: 6px;
  }
  .hint {
    margin: 6px 0 0;
    color: #777;
    line-height: 1.4;
  }
</style>
