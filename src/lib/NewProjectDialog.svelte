<script lang="ts">
  // "New project" window: pick a canvas size and background
  type Result = { width: number; height: number; paper: 'white' | 'transparent' } | null;
  let { ondone }: { ondone: (r: Result) => void } = $props();

  const PRESETS = [
    { name: 'Vtuber bust', w: 3000, h: 4000 },
    { name: 'Vtuber half body', w: 4000, h: 6000 },
    { name: 'Vtuber full body', w: 5000, h: 8000 },
    { name: 'Illustration A4 (300 dpi)', w: 2480, h: 3508 },
    { name: 'Square post', w: 2048, h: 2048 },
    { name: 'Video 1080p', w: 1920, h: 1080 },
    { name: 'Video 4K', w: 3840, h: 2160 },
    { name: 'Phone / story', w: 1080, h: 1920 },
  ];

  let width = $state(3000);
  let height = $state(4000);
  let paper = $state<'white' | 'transparent'>('white');
  let chosen = $state('Vtuber bust');

  const MAX = 10000;
  let megapixels = $derived(((width * height) / 1e6).toFixed(1));
  let tooBig = $derived(width * height > 60e6);

  function choose(p: (typeof PRESETS)[number]) {
    chosen = p.name;
    width = p.w;
    height = p.h;
  }

  function swap() {
    [width, height] = [height, width];
    chosen = '';
  }

  function create() {
    const w = Math.round(Math.min(MAX, Math.max(16, width || 0)));
    const h = Math.round(Math.min(MAX, Math.max(16, height || 0)));
    ondone({ width: w, height: h, paper });
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

  <div class="presets">
    {#each PRESETS as p (p.name)}
      <button class:on={chosen === p.name} onclick={() => choose(p)}>
        <span>{p.name}</span>
        <span class="dim">{p.w} × {p.h}</span>
      </button>
    {/each}
  </div>

  <div class="size">
    <label>Width <input type="number" min="16" max={MAX} bind:value={width} oninput={() => (chosen = '')} /></label>
    <button class="swap" onclick={swap} title="Swap width and height">⇄</button>
    <label>Height <input type="number" min="16" max={MAX} bind:value={height} oninput={() => (chosen = '')} /></label>
  </div>
  <p class="dim">{megapixels} megapixels{tooBig ? ' · very large, may be slow on weaker computers' : ''}</p>

  <div class="paper">
    <span>Background</span>
    <button class:on={paper === 'white'} onclick={() => (paper = 'white')}>White paper</button>
    <button class:on={paper === 'transparent'} onclick={() => (paper = 'transparent')}>Transparent</button>
  </div>

  <div class="buttons">
    <span class="spacer"></span>
    <button onclick={() => ondone(null)}>Cancel</button>
    <button class="primary" onclick={create}>Create</button>
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
    gap: 12px;
    padding: 18px;
    background: #24242b;
    border: 1px solid #444;
    border-radius: 12px;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.6);
    color: #eee;
    font-family: system-ui, sans-serif;
    font-size: 13px;
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
  input {
    background: #15151a;
    border: 1px solid #444;
    border-radius: 4px;
    color: #eee;
    padding: 6px 8px;
    font-size: 13px;
  }
  .swap {
    padding: 6px 10px;
  }
  .paper {
    display: flex;
    align-items: center;
    gap: 6px;
    color: #bbb;
  }
  .paper span {
    margin-right: auto;
  }
  .buttons {
    display: flex;
    gap: 6px;
  }
  .spacer {
    flex: 1;
  }
</style>
