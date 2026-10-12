<script lang="ts">
  // Brush panel: brush types by category (with a preview stroke for each), then the
  // settings of the chosen brush, including tip shape and randomness.
  import {
    PRESETS,
    ERASER_PRESETS,
    CATEGORIES,
    TIP_SHAPES,
    SHAPE_BASES,
    EDGES,
    settingsOf,
    remixBrushes,
    surpriseBrush,
    renderBrushPreview,
    ensureTip,
    getTipMask,
    type BrushSettings,
    type BrushPreset,
  } from './brush';
  import TipPad from './TipPad.svelte';

  type MyBrush = BrushPreset & { id: string; source?: string };

  let {
    brush = $bindable(),
    preset = $bindable(),
    eraser = false,
    mine = [],
    color = '#1d1d24',
    onsave,
    onlibrary,
    ontipfromcanvas,
    onshare,
    hasSelection = false,
  }: {
    brush: BrushSettings;
    preset: string;
    eraser?: boolean;
    mine?: MyBrush[];
    color?: string;
    onsave?: () => void;
    onlibrary?: (tab?: 'mine' | 'import' | 'community') => void;
    ontipfromcanvas?: () => void;
    onshare?: (name: string, about: string | undefined, settings: BrushSettings) => void;
    hasSelection?: boolean;
  } = $props();

  let all = $derived<BrushPreset[]>(eraser ? ERASER_PRESETS : [...PRESETS, ...mine]);
  let current = $derived(all.find((p) => p.name === preset));
  // svelte-ignore state_referenced_locally
  let category = $state(PRESETS.find((p) => p.name === preset)?.category ?? 'Pens');
  let list = $derived<BrushPreset[]>(eraser ? ERASER_PRESETS : category === 'My brushes' ? mine : PRESETS.filter((p) => p.category === category));
  let showTip = $state(false);
  let tab = $state<'tip' | 'edges' | 'random' | 'make'>('tip');
  let remixWith = $state<string | undefined>(undefined);
  let remixT = $state(0.5);
  let remixFrom: BrushSettings | null = null; // the brush before remixing started

  // The other brush for remixing (first one that isn't this brush)
  let remixOther = $derived(all.find((p) => p.name === remixWith) ?? all.find((p) => p.name !== preset));

  function remix() {
    if (!remixOther) return;
    if (!remixFrom) remixFrom = $state.snapshot(brush) as BrushSettings;
    const keepImage = remixT < 0.5 ? { tipImage: remixFrom.tipImage, tipId: remixFrom.tipId } : { tipImage: remixOther.tipImage, tipId: remixOther.tipId };
    Object.assign(brush, remixBrushes(remixFrom, settingsOf(remixOther), remixT), keepImage);
    ensureTip(brush);
  }

  // 🎲 A random brush, ready to draw with. `part` randomizes only one group of settings.
  const PARTS = {
    tip: ['tip', 'shapeBase', 'sides', 'starInner', 'roundness', 'angle', 'followDirection', 'hardness'],
    edges: ['edge', 'edgeAmount', 'edgeDetail', 'wetRim', 'grain', 'variants', 'seed'],
    random: ['scatter', 'sizeJitter', 'angleJitter', 'opacityJitter', 'colorJitter', 'spacing'],
  } as const;
  function surprise(part?: keyof typeof PARTS) {
    remixFrom = null;
    const r = surpriseBrush() as Record<string, unknown>;
    if (eraser) r.colorJitter = 0;
    if (!part) {
      Object.assign(brush, { tipImage: undefined, tipId: undefined }, r);
      preset = 'Random brush'; // a new brush: save it with ★ Save as my brush to keep it
      return;
    }
    // an image tip stays unless the tip itself is randomized
    if (part === 'tip') Object.assign(brush, { tipImage: undefined, tipId: undefined });
    for (const k of PARTS[part]) (brush as Record<string, unknown>)[k] = r[k];
  }

  function useDrawnTip(url: string) {
    Object.assign(brush, { tip: 'image', tipImage: url, tipId: 'pad' + Date.now().toString(36), hardness: 1 });
    ensureTip(brush);
    remixFrom = null;
  }

  // Opens the share window (brush card, shared library, send to apps)
  function share() {
    onshare?.(preset || 'My brush', current?.about, $state.snapshot(brush) as BrushSettings);
  }

  function choose(p: BrushPreset) {
    preset = p.name;
    remixFrom = null;
    // clear image tips from the previous brush, then take every setting of the new one
    Object.assign(brush, { tipImage: undefined, tipId: undefined }, settingsOf(p));
    ensureTip(brush);
  }

  function reset() {
    if (current) choose(current);
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

  // Svelte action: paints the preview stroke of a brush
  function preview(node: HTMLCanvasElement, args: { p: BrushPreset; color: string }) {
    const draw = (a: { p: BrushPreset; color: string }) => {
      const s = settingsOf(a.p);
      ensureTip(s, () => renderBrushPreview(node, s, a.color));
      renderBrushPreview(node, s, a.color);
    };
    draw(args);
    return { update: draw };
  }

  // Svelte action: the current tip shape, big
  function tipView(node: HTMLCanvasElement, s: BrushSettings & { variant?: number }) {
    const draw = (b: BrushSettings & { variant?: number }) => {
      const ctx = node.getContext('2d')!;
      ctx.clearRect(0, 0, node.width, node.height);
      ctx.fillStyle = '#15151a';
      ctx.fillRect(0, 0, node.width, node.height);
      const m = getTipMask(b, b.variant ?? 0);
      ctx.save();
      ctx.translate(node.width / 2, node.height / 2);
      ctx.rotate((b.angle * Math.PI) / 180);
      ctx.scale(1, Math.max(0.05, b.roundness));
      const r = node.height * 0.42;
      ctx.drawImage(m, -r, -r, r * 2, r * 2);
      ctx.restore();
    };
    ensureTip(s, () => draw(s));
    draw(s);
    return { update: draw };
  }
</script>

<div class="brush-panel">
  {#if !eraser}
    <div class="cats">
      {#each CATEGORIES as c (c)}
        <button class:on={category === c} onclick={() => (category = c)}>{c}</button>
      {/each}
    </div>
  {/if}

  <div class="cards">
    {#each list as p, i (i + p.name)}
      <button class="card" class:on={preset === p.name} onclick={() => choose(p)} title={p.about ?? p.name}>
        <canvas width="150" height="40" use:preview={{ p, color }}></canvas>
        <span>{p.name}</span>
      </button>
    {/each}
    {#if category === 'My brushes' && !eraser && !mine.length}
      <p class="about">
        No brushes of your own yet. Change a brush and press <b>Save as my brush</b>, or import brushes from Photoshop (.abr), Krita (.kpp, .bundle), Clip Studio (.sut) or a PNG.
      </p>
    {/if}
  </div>

  {#if current?.about}
    <p class="about"><b>{preset}:</b> {current.about}</p>
  {/if}

  {#if !eraser}
    <div class="lib-row">
      <button onclick={() => onsave?.()} title="Keep these settings as a brush of your own">★ Save as my brush</button>
      <button onclick={() => onlibrary?.('mine')} title="Your brushes, import, and brushes shared by others">📚 Library…</button>
      <button onclick={() => onlibrary?.('import')} title="Photoshop .abr, Krita .kpp / .bundle, Clip Studio .sut, PNG">⇪ Import</button>
    </div>
  {/if}

  <label>
    <span>{eraser ? 'Eraser size' : 'Brush size'}</span>
    <input
      type="range"
      min="0"
      max="1"
      step="0.001"
      value={toSlider(brush.size)}
      oninput={(e) => (brush.size = fromSlider(+e.currentTarget.value))}
      title="Small sizes take up half of the slider"
    />
    <input class="num" type="number" min="0.5" max="500" step="0.1" value={brush.size} onchange={(e) => (brush.size = clampSize(+e.currentTarget.value))} />
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
    <input type="range" min="0.02" max="2" step="0.01" bind:value={brush.spacing} />
    <span class="val">{pct(brush.spacing)}</span>
  </label>
  {#if !eraser}
    <label title="0% = clean paint, 100% = only blends the colors already on the layer">
      <span>Color mix</span>
      <input type="range" min="0" max="1" step="0.01" bind:value={brush.mix} />
      <span class="val">{pct(brush.mix)}</span>
    </label>
    {#if brush.mix > 0}
      <div class="ends">
        <span>Mix with</span>
        <div class="seg two">
          <button class:on={brush.mixRefer === 'all'} onclick={() => (brush.mixRefer = 'all')} title="Mix with everything you see: all visible layers and the paper">All layers</button>
          <button class:on={brush.mixRefer === 'layer'} onclick={() => (brush.mixRefer = 'layer')} title="Mix only with the colors on the active layer">This layer</button>
        </div>
      </div>
    {/if}
  {/if}
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

  <button class="fold" onclick={() => (showTip = !showTip)}>{showTip ? '▾' : '▸'} Brush designer: tip, edges, randomness</button>
  {#if showTip}
    <div class="tabs">
      <button class:on={tab === 'tip'} onclick={() => (tab = 'tip')}>Tip</button>
      <button class:on={tab === 'edges'} onclick={() => (tab = 'edges')}>Edges</button>
      <button class:on={tab === 'random'} onclick={() => (tab = 'random')}>Random</button>
      <button class:on={tab === 'make'} onclick={() => (tab = 'make')}>✨ Make</button>
    </div>

    {#if tab === 'tip'}
      <div class="tip-box">
        <canvas class="tip-view" width="90" height="90" use:tipView={{ ...brush }}></canvas>
        <div class="tips">
          {#each TIP_SHAPES.filter((t) => t.id !== 'image' || brush.tipImage) as t (t.id)}
            <button class:on={brush.tip === t.id} onclick={() => (brush.tip = t.id)}>{t.label}</button>
          {/each}
        </div>
      </div>
      {#if brush.tip === 'shape'}
        <div class="shapes">
          {#each SHAPE_BASES as sb (sb.id)}
            <button class:on={brush.shapeBase === sb.id} onclick={() => (brush.shapeBase = sb.id)} title={sb.label}>{sb.icon}</button>
          {/each}
        </div>
        {#if brush.shapeBase === 'polygon' || brush.shapeBase === 'star'}
          <label>
            <span>{brush.shapeBase === 'star' ? 'Points' : 'Sides'}</span>
            <input type="range" min="3" max="12" step="1" bind:value={brush.sides} />
            <span class="val">{brush.sides}</span>
          </label>
        {/if}
        {#if brush.shapeBase === 'star'}
          <label title="How deep the cuts between the points are">
            <span>Star depth</span>
            <input type="range" min="0.1" max="0.9" step="0.01" bind:value={brush.starInner} />
            <span class="val">{pct(1 - brush.starInner)}</span>
          </label>
        {/if}
      {/if}
      <label title="1 = round, small = flat like a chisel marker or calligraphy nib">
        <span>Roundness</span>
        <input type="range" min="0.05" max="1" step="0.01" bind:value={brush.roundness} />
        <span class="val">{pct(brush.roundness)}</span>
      </label>
      <label>
        <span>Angle</span>
        <input type="range" min="-180" max="180" step="1" bind:value={brush.angle} />
        <span class="val">{brush.angle}°</span>
      </label>
      <label class="check"><input type="checkbox" bind:checked={brush.followDirection} /> Tip turns with the stroke</label>
    {:else if tab === 'edges'}
      <p class="about">The outline of the tip: works with every tip, imported ones too. Each dab can use a different version, so strokes never look stamped.</p>
      <div class="edges">
        {#each EDGES as ed (ed.id)}
          <button class:on={brush.edge === ed.id} onclick={() => (brush.edge = ed.id)} title={ed.about}>{ed.label}</button>
        {/each}
      </div>
      {#if brush.edge !== 'smooth'}
        <label>
          <span>Edge amount</span>
          <input type="range" min="0" max="1" step="0.01" bind:value={brush.edgeAmount} />
          <span class="val">{pct(brush.edgeAmount)}</span>
        </label>
        <label title="Left: big slow wobbles. Right: fine fringe">
          <span>Edge detail</span>
          <input type="range" min="0" max="1" step="0.01" bind:value={brush.edgeDetail} />
          <span class="val">{pct(brush.edgeDetail)}</span>
        </label>
      {/if}
      <label title="Paint collects at the edge and the middle stays lighter, like drying watercolor">
        <span>Wet rim</span>
        <input type="range" min="0" max="1" step="0.01" bind:value={brush.wetRim} />
        <span class="val">{pct(brush.wetRim)}</span>
      </label>
      <label title="Tiny holes inside the tip, like dry pastel or charcoal">
        <span>Grain</span>
        <input type="range" min="0" max="1" step="0.01" bind:value={brush.grain} />
        <span class="val">{pct(brush.grain)}</span>
      </label>
      <label title="How many different versions of the tip the brush switches between">
        <span>Variations</span>
        <input type="range" min="1" max="8" step="1" bind:value={brush.variants} />
        <span class="val">{brush.variants}</span>
      </label>
      <div class="variants">
        {#each Array.from({ length: Math.max(1, Math.min(4, brush.variants)) }) as _, v (v)}
          <canvas class="tip-mini" width="64" height="64" use:tipView={{ ...brush, variant: v }}></canvas>
        {/each}
        <button onclick={() => (brush.seed = Math.floor(Math.random() * 9999) + 1)} title="A new random pattern for the edges and grain">🎲 New pattern</button>
      </div>
    {:else if tab === 'random'}
      <button class="dice big" onclick={() => surprise()} title="A completely new random brush. Just draw with it; keep pressing until you like one, then ★ Save as my brush">🎲 Randomize everything</button>
      <div class="dice-row">
        <button class="dice" onclick={() => surprise('tip')} title="New random tip shape, angle and roundness">🎲 Tip</button>
        <button class="dice" onclick={() => surprise('edges')} title="New random edges, wet rim and grain">🎲 Edges</button>
        <button class="dice" onclick={() => surprise('random')} title="New random scatter, jitter and spacing">🎲 Scatter</button>
      </div>
      <label title="Dabs spread out to the sides">
        <span>Scatter</span>
        <input type="range" min="0" max="3" step="0.01" bind:value={brush.scatter} />
        <span class="val">{pct(brush.scatter)}</span>
      </label>
      <label>
        <span>Size random</span>
        <input type="range" min="0" max="1" step="0.01" bind:value={brush.sizeJitter} />
        <span class="val">{pct(brush.sizeJitter)}</span>
      </label>
      <label>
        <span>Angle random</span>
        <input type="range" min="0" max="1" step="0.01" bind:value={brush.angleJitter} />
        <span class="val">{pct(brush.angleJitter)}</span>
      </label>
      <label>
        <span>Paint random</span>
        <input type="range" min="0" max="1" step="0.01" bind:value={brush.opacityJitter} />
        <span class="val">{pct(brush.opacityJitter)}</span>
      </label>
      {#if !eraser}
        <label title="Each dab gets a slightly different shade">
          <span>Color random</span>
          <input type="range" min="0" max="1" step="0.01" bind:value={brush.colorJitter} />
          <span class="val">{pct(brush.colorJitter)}</span>
        </label>
      {/if}
    {:else}
      <div class="panel-sub">Draw your own tip</div>
      <TipPad {brush} onuse={useDrawnTip} />
      <button onclick={() => ontipfromcanvas?.()} title="The selected part of the active layer (or the whole layer) becomes the brush tip">
        ⬚ Tip from {hasSelection ? 'the selection' : 'the active layer'}
      </button>
      <div class="panel-sub">Ideas</div>
      <div class="seg two">
        <button onclick={() => surprise()} title="A random new brush. Keep pressing until you like one, then save it">🎲 Surprise me</button>
        {#if !eraser}
          <button onclick={share} title="Share with everyone: in the shared library, as a picture that carries the brush, or to other apps">⇪ Share brush</button>
        {/if}
      </div>
      <div class="panel-sub">Remix with another brush</div>
      <select class="pick" bind:value={remixWith} onchange={() => (remixFrom = null)}>
        {#each all.filter((p) => p.name !== preset) as p (p.name)}
          <option value={p.name}>{p.name}</option>
        {/each}
      </select>
      <label title="Left = your brush, right = the other brush">
        <span>Mix</span>
        <input type="range" min="0" max="1" step="0.01" bind:value={remixT} oninput={remix} />
        <span class="val">{pct(remixT)}</span>
      </label>
      <p class="about">Move the slider to blend the two brushes. Like the result? Press <b>★ Save as my brush</b>.</p>
    {/if}
  {/if}

  <div class="bottom-row">
    <button onclick={() => surprise()} title="A completely new random brush, ready to draw">🎲 Random brush</button>
    <button onclick={reset} disabled={!current} title="Back to how this brush was">↺ Reset brush</button>
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
  button {
    padding: 6px 10px;
    border-radius: 6px;
    border: 1px solid #444;
    background: #2b2b33;
    color: #eee;
    cursor: pointer;
    font: inherit;
  }
  button:hover {
    border-color: #666;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button.on {
    background: #00b5c8;
    border-color: #00b5c8;
    color: #111;
    font-weight: 600;
  }
  .cats {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
  }
  .cats button {
    padding: 4px 7px;
    font-size: 11px;
  }
  .cards {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px;
    max-height: 260px;
    overflow-y: auto;
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 2px;
    padding: 3px 3px 4px;
    background: #ececf0;
    color: #222;
    border-color: #ececf0;
  }
  .card.on {
    background: #ececf0;
    border-color: #00b5c8;
    box-shadow: 0 0 0 2px #00b5c8;
    color: #111;
  }
  .card canvas {
    width: 100%;
    height: 32px;
  }
  .card span {
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .about {
    margin: 0 0 2px;
    color: #999;
    line-height: 1.4;
    grid-column: 1 / -1;
  }
  .about b {
    color: #ddd;
  }
  .lib-row {
    display: grid;
    grid-template-columns: 1.3fr 1fr 0.8fr;
    gap: 3px;
  }
  .lib-row button {
    padding: 5px 2px;
    font-size: 11px;
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
  .seg.two {
    grid-template-columns: 1fr 1fr;
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
  .fold {
    text-align: left;
    background: none;
    border-color: #3a3a44;
    margin-top: 4px;
  }
  .tip-box {
    display: flex;
    gap: 6px;
  }
  .tip-view {
    width: 70px;
    height: 70px;
    border-radius: 6px;
    flex-shrink: 0;
  }
  .tips {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
    flex: 1;
  }
  .tips button {
    padding: 3px 2px;
    font-size: 10px;
  }
  .tabs {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 2px;
  }
  .tabs button {
    padding: 4px 2px;
    font-size: 11px;
  }
  .shapes {
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    gap: 2px;
  }
  .shapes button {
    padding: 3px 0;
    font-size: 15px;
    line-height: 1.1;
  }
  .edges {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 2px;
  }
  .edges button {
    padding: 4px 2px;
    font-size: 11px;
  }
  .variants {
    display: flex;
    gap: 3px;
    align-items: center;
  }
  .tip-mini {
    width: 38px;
    height: 38px;
    border-radius: 5px;
  }
  .variants button {
    flex: 1;
    padding: 4px 2px;
    font-size: 11px;
  }
  .dice.big {
    background: linear-gradient(90deg, #00b5c8, #8a5cf6);
    border: none;
    color: #fff;
    font-weight: 600;
    padding: 8px;
  }
  .dice-row {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 2px;
  }
  .dice-row button {
    padding: 4px 2px;
    font-size: 11px;
  }
  .panel-sub {
    color: #888;
    font-size: 11px;
    margin-top: 4px;
  }
  .pick {
    background: #15151a;
    color: #eee;
    border: 1px solid #444;
    border-radius: 4px;
    padding: 3px;
    font-size: 12px;
  }
  .bottom-row {
    display: flex;
    justify-content: flex-end;
    gap: 4px;
  }
  .bottom-row button {
    font-size: 11px;
    padding: 4px 8px;
  }
  .hint {
    margin: 6px 0 0;
    color: #777;
    line-height: 1.4;
  }
</style>
