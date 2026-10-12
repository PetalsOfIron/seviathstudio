<script lang="ts">
  // Gradient settings: the colors (stops), shape, repeat, and how it's put on the picture
  import { SHAPES, PRESETS, cssGradient, colorAt, rgbHex, sortStops, type GradientSettings, type GradStop } from './gradient';

  export type GradApply = {
    target: 'new' | 'layer'; // a new layer above (safe) or straight onto the active layer
    where: 'all' | 'drawing'; // everywhere, or only where the layer already has paint
    blend: string;
  };

  let {
    grad = $bindable(),
    apply = $bindable(),
    color = $bindable(),
    color2 = $bindable(),
    blends,
    vectorActive = false,
    colorsOnly = false,
  }: {
    grad: GradientSettings;
    apply: GradApply;
    color: string;
    color2: string;
    blends: string[];
    vectorActive?: boolean;
    colorsOnly?: boolean; // only the colors (for painting with the gradient brush)
  } = $props();

  let selected = $state(0); // index into grad.stops
  let linkWheel = $state(true); // the big color wheel edits the selected stop
  // svelte-ignore state_referenced_locally
  let lastSync = color; // the color last copied between the wheel and the stop (start: don't copy)
  let bar = $state<HTMLDivElement>();
  let dragging = -1;

  let sel = $derived(grad.stops[selected] ?? grad.stops[0]);

  // Picking a stop shows its color on the color wheel…
  function selectStop(i: number) {
    selected = i;
    if (linkWheel && grad.stops[i]) {
      lastSync = grad.stops[i].color;
      color = grad.stops[i].color;
    }
  }

  // …and changing the color wheel recolors that stop
  $effect(() => {
    const c = color;
    if (!linkWheel || !grad.stops[selected]) return;
    if (c === lastSync) return;
    lastSync = c;
    grad.stops[selected].color = c;
  });

  function usePreset(name: string) {
    const p = PRESETS.find((x) => x.name === name)!;
    if (name === 'Color → transparent') {
      grad.stops = [
        { pos: 0, color, alpha: 1 },
        { pos: 1, color, alpha: 0 },
      ];
    } else if (name === 'Color → second color') {
      grad.stops = [
        { pos: 0, color, alpha: 1 },
        { pos: 1, color: color2, alpha: 1 },
      ];
    } else grad.stops = p.stops.map((s) => ({ ...s }));
    lastSync = color; // don't let the wheel overwrite the preset
    selected = 0;
  }

  function posFrom(e: PointerEvent) {
    const r = bar!.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  }

  // click on the bar: new color stop there, with the color the gradient already has
  function addStop(e: PointerEvent) {
    const pos = posFrom(e);
    const c = colorAt(grad.stops, pos, grad.smooth);
    grad.stops = [...grad.stops, { pos, color: rgbHex(c[0], c[1], c[2]), alpha: c[3] / 255 }];
    selectStop(grad.stops.length - 1);
  }

  function grabStop(e: PointerEvent, i: number) {
    e.stopPropagation();
    selectStop(i);
    dragging = i;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function moveStop(e: PointerEvent) {
    if (dragging < 0) return;
    grad.stops[dragging].pos = posFrom(e);
  }

  function removeStop() {
    if (grad.stops.length <= 2) return;
    grad.stops = grad.stops.filter((_, i) => i !== selected);
    selectStop(0);
  }

  function reverse() {
    grad.stops = sortStops(grad.stops).map((s) => ({ ...s, pos: 1 - s.pos }));
  }

  const pct = (x: number) => Math.round(x * 100) + '%';
</script>

<div class="grad-panel">
  <div class="bar-wrap">
    <div class="bar checker" bind:this={bar} role="presentation" onpointerdown={addStop} title="Click to add a color">
      <div class="fill" style="background: {cssGradient(grad.stops, grad.smooth)}"></div>
    </div>
    <div class="stops">
      {#each grad.stops as s, i (i)}
        <button
          class="stop"
          class:on={i === selected}
          style="left: {s.pos * 100}%; --c: {s.color}; opacity: {0.35 + s.alpha * 0.65}"
          title="Drag to move · {Math.round(s.pos * 100)}%"
          aria-label="Color stop"
          onpointerdown={(e) => grabStop(e, i)}
          onpointermove={moveStop}
          onpointerup={() => (dragging = -1)}
        ></button>
      {/each}
    </div>
  </div>

  <div class="stop-list">
    {#each grad.stops.map((s, i) => ({ s, i })).sort((a, b) => a.s.pos - b.s.pos) as { s, i } (i)}
      <div class="stop-row" class:on={i === selected}>
        <button class="pick" onclick={() => selectStop(i)} title="Select this color point">{i === selected ? '●' : '○'}</button>
        <input type="color" value={s.color} oninput={(e) => { grad.stops[i].color = e.currentTarget.value; selectStop(i); }} title="Choose this point's color" />
        <input class="num" type="number" min="0" max="100" value={Math.round(s.pos * 100)} onchange={(e) => (grad.stops[i].pos = Math.min(1, Math.max(0, +e.currentTarget.value / 100)))} title="Position %" />
        <span class="unit">%</span>
        <input class="alpha" type="range" min="0" max="1" step="0.01" bind:value={grad.stops[i].alpha} title="Opacity of this point: {pct(s.alpha)}" />
        <button class="x" onclick={() => { selected = i; removeStop(); }} disabled={grad.stops.length <= 2} title="Remove this point">✕</button>
      </div>
    {/each}
  </div>
  {#if sel}
    <div class="hex-row">
      <span>Point ● color</span>
      <input
        type="text"
        value={sel.color}
        maxlength="7"
        spellcheck="false"
        onchange={(e) => {
          const v = e.currentTarget.value.trim().replace(/^#?/, '#');
          if (/^#[0-9a-f]{6}$/i.test(v)) {
            grad.stops[selected].color = v.toLowerCase();
            selectStop(selected);
          }
        }}
        title="Type or paste a color code"
      />
      <button onclick={() => navigator.clipboard?.writeText(sel.color)} title="Copy the color code">⧉ Copy</button>
      <button
        onclick={async () => {
          const t = (await navigator.clipboard?.readText().catch(() => '')) ?? '';
          const v = t.trim().replace(/^#?/, '#');
          if (/^#[0-9a-f]{6}$/i.test(v)) {
            grad.stops[selected].color = v.toLowerCase();
            selectStop(selected);
          }
        }}
        title="Paste a color code">📋</button
      >
    </div>
  {/if}
  <label class="check" title="When on, the big color wheel on the right changes the selected point (●)">
    <input type="checkbox" bind:checked={linkWheel} /> Color wheel changes the selected point
  </label>
  <p class="hint">Click the bar to add a point, drag points to move them. Each point has its own color (click its swatch) and opacity.</p>

  <div class="two-colors">
    <span>Second color</span>
    <input type="color" bind:value={color2} title="Second color for 'Color → second color'" />
    <button onclick={reverse} title="Flip the gradient around">⇄ Reverse</button>
  </div>

  <select onchange={(e) => usePreset(e.currentTarget.value)} value="">
    <option value="" disabled>Presets…</option>
    {#each PRESETS as p (p.name)}
      <option value={p.name}>{p.name}</option>
    {/each}
  </select>

  {#if !colorsOnly}
  <div class="seg five">
    {#each SHAPES as s (s.id)}
      <button class:on={grad.shape === s.id} onclick={() => (grad.shape = s.id)} title={s.label}>{s.icon}<small>{s.label}</small></button>
    {/each}
  </div>
  <div class="seg three">
    <button class:on={grad.repeat === 'none'} onclick={() => (grad.repeat = 'none')}>Once</button>
    <button class:on={grad.repeat === 'repeat'} onclick={() => (grad.repeat = 'repeat')}>Repeat</button>
    <button class:on={grad.repeat === 'mirror'} onclick={() => (grad.repeat = 'mirror')}>Mirror</button>
  </div>
  <label class="row">
    <span>Opacity</span>
    <input type="range" min="0.01" max="1" step="0.01" bind:value={grad.opacity} />
    <span class="val">{pct(grad.opacity)}</span>
  </label>
  <label class="check" title="Blends colors so the middle stays clean instead of turning gray (OKLab)">
    <input type="checkbox" bind:checked={grad.smooth} /> Clean color blending
  </label>
  <label class="check" title="A tiny bit of noise that hides stripes in smooth gradients">
    <input type="checkbox" bind:checked={grad.dither} /> No banding (dither)
  </label>

  <div class="sub">Put it…</div>
  <div class="seg two">
    <button class:on={apply.target === 'new' || vectorActive} onclick={() => (apply.target = 'new')} title="On a new layer above: your drawing is not changed, the gradient can be moved, faded or deleted later">On a new layer ✓ safe</button>
    <button
      class:on={apply.target === 'layer' && !vectorActive}
      disabled={vectorActive}
      onclick={() => (apply.target = 'layer')}
      title="Straight into the active layer's pixels">Into this layer</button
    >
  </div>
  <div class="seg two">
    <button class:on={apply.where === 'drawing'} onclick={() => (apply.where = 'drawing')} title="Only where the layer already has paint: the shape of your drawing stays exactly the same">Only on the drawing</button>
    <button class:on={apply.where === 'all'} onclick={() => (apply.where = 'all')} title="Over the whole canvas (or the selection)">Everywhere</button>
  </div>
  <label class="row">
    <span>Mix mode</span>
    <select bind:value={apply.blend}>
      {#each blends as b (b)}
        <option value={b}>{b}</option>
      {/each}
    </select>
  </label>
  <p class="hint">
    Drag on the canvas from where the gradient starts to where it ends (Shift = straight angles).
    <b>Only on the drawing</b> + <b>Multiply</b> or <b>Overlay</b> shades your drawing without touching its lines or shape.
    With a selection, it goes only inside it.
  </p>
  {/if}
</div>

<style>
  .grad-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-size: 12px;
    color: #bbb;
  }
  .bar-wrap {
    position: relative;
    padding-bottom: 16px;
  }
  .stop-list {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .stop-row {
    display: grid;
    grid-template-columns: 22px 30px 40px 10px 1fr 24px;
    align-items: center;
    gap: 4px;
    padding: 2px 3px;
    border-radius: 5px;
    border: 1px solid transparent;
  }
  .stop-row.on {
    border-color: #00b5c8;
    background: #00b5c81a;
  }
  .stop-row .pick,
  .stop-row .x {
    padding: 2px 0;
  }
  .stop-row .num {
    width: 100%;
    box-sizing: border-box;
    background: #15151a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #eee;
    padding: 2px 3px;
    font-size: 11px;
  }
  .stop-row .alpha {
    width: 100%;
    min-width: 0;
  }
  .unit {
    color: #777;
    font-size: 10px;
  }
  .bar {
    position: relative;
    height: 26px;
    border-radius: 5px;
    border: 1px solid #555;
    cursor: copy;
    overflow: hidden;
  }
  .checker {
    background: repeating-conic-gradient(#666 0% 25%, #999 0% 50%) 50% / 10px 10px;
  }
  .fill {
    position: absolute;
    inset: 0;
  }
  .stops {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 16px;
  }
  .stop {
    position: absolute;
    top: 0;
    width: 16px;
    height: 16px;
    margin-left: -8px;
    padding: 0;
    border: 2px solid #ddd;
    border-radius: 3px 3px 6px 6px;
    background: var(--c);
    cursor: ew-resize;
    touch-action: none;
  }
  .stop.on {
    border-color: #00d8ee;
    transform: scale(1.2);
  }
  .hex-row {
    display: grid;
    grid-template-columns: auto 1fr auto auto;
    gap: 4px;
    align-items: center;
  }
  .hex-row input {
    min-width: 0;
    background: #15151a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #eee;
    padding: 4px 6px;
    font-family: ui-monospace, monospace;
    font-size: 12px;
  }
  .two-colors {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .two-colors span {
    flex: 1;
  }
  input[type='color'] {
    width: 34px;
    height: 24px;
    padding: 0;
    border: 1px solid #555;
    background: none;
  }
  button {
    padding: 5px 8px;
    border-radius: 6px;
    border: 1px solid #444;
    background: #2b2b33;
    color: #eee;
    cursor: pointer;
    font: inherit;
    font-size: 11px;
  }
  button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  button.on {
    background: #00b5c8;
    border-color: #00b5c8;
    color: #111;
    font-weight: 600;
  }
  select {
    background: #15151a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #eee;
    padding: 4px 6px;
    font-size: 12px;
  }
  .seg {
    display: grid;
    gap: 2px;
  }
  .seg.five {
    grid-template-columns: repeat(5, 1fr);
  }
  .seg.five button {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 4px 0;
    font-size: 14px;
  }
  .seg.five small {
    font-size: 9px;
  }
  .seg.three {
    grid-template-columns: repeat(3, 1fr);
  }
  .seg.two {
    grid-template-columns: 1fr 1fr;
  }
  .row {
    display: grid;
    grid-template-columns: 76px 1fr 40px;
    align-items: center;
    gap: 6px;
  }
  .row select {
    grid-column: 2 / 4;
  }
  .val {
    text-align: right;
    color: #888;
  }
  .check {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .sub {
    color: #888;
    font-size: 11px;
    margin-bottom: -4px;
  }
  .hint {
    margin: 0;
    color: #777;
    line-height: 1.4;
  }
</style>
