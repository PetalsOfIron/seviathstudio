<script lang="ts">
  // Project manager: all your works with a preview, like the CSP / Procreate gallery.
  // Works are kept in this browser (see library.ts) and saved automatically while you draw.
  import { onMount } from 'svelte';
  import {
    listProjects,
    loadFromLibrary,
    renameProject,
    deleteProject,
    duplicateProject,
    storageInfo,
    type ProjectMeta,
  } from './library';
  import { saveProject, ACCEPT } from './fileio';

  let {
    currentId = null,
    canClose = false,
    onopen,
    onnew,
    onfiles,
    onclose,
    onrenamed,
    ondeleted,
  }: {
    currentId?: string | null;
    canClose?: boolean;
    onopen: (id: string) => void;
    onnew: () => void;
    onfiles: (files: File[]) => void;
    onclose: () => void;
    onrenamed: (id: string, name: string) => void;
    ondeleted: (id: string) => void;
  } = $props();

  let projects = $state<ProjectMeta[]>([]);
  let thumbs = $state<Record<string, string>>({});
  let loading = $state(true);
  let error = $state('');
  let search = $state('');
  let sortBy = $state<'updated' | 'name' | 'created'>('updated');
  let menuFor = $state<string | null>(null);
  let renaming = $state<string | null>(null);
  let renameText = $state('');
  let busy = $state('');
  let dropping = $state(false);
  let storage = $state<{ used: number; quota: number } | null>(null);
  let fileInput = $state<HTMLInputElement>();

  let shown = $derived.by(() => {
    const q = search.trim().toLowerCase();
    const list = projects.filter((p) => !q || p.name.toLowerCase().includes(q));
    if (sortBy === 'name') return [...list].sort((a, b) => a.name.localeCompare(b.name));
    if (sortBy === 'created') return [...list].sort((a, b) => b.created - a.created);
    return list; // already newest first
  });

  async function refresh() {
    try {
      const list = await listProjects();
      for (const url of Object.values(thumbs)) URL.revokeObjectURL(url);
      const t: Record<string, string> = {};
      for (const p of list) if (p.thumb) t[p.id] = URL.createObjectURL(p.thumb);
      thumbs = t;
      projects = list;
      storage = await storageInfo();
      error = '';
    } catch (e) {
      error = 'Could not read the project library: ' + (e instanceof Error ? e.message : e) + '. Private / incognito windows and some browser shields block it.';
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    refresh();
    return () => {
      for (const url of Object.values(thumbs)) URL.revokeObjectURL(url);
    };
  });

  function when(t: number) {
    const s = (Date.now() - t) / 1000;
    if (s < 60) return 'just now';
    if (s < 3600) return `${Math.floor(s / 60)} min ago`;
    if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
    if (s < 86400 * 7) return `${Math.floor(s / 86400)} d ago`;
    return new Date(t).toLocaleDateString();
  }

  function mb(bytes: number) {
    return bytes > 1e9 ? (bytes / 1e9).toFixed(1) + ' GB' : (bytes / 1e6).toFixed(0) + ' MB';
  }

  async function act(label: string, f: () => Promise<void>) {
    menuFor = null;
    busy = label;
    try {
      await f();
    } catch (e) {
      alert(`${label} failed: ${e instanceof Error ? e.message : e}`);
    } finally {
      busy = '';
    }
    await refresh();
  }

  function startRename(p: ProjectMeta) {
    menuFor = null;
    renaming = p.id;
    renameText = p.name;
  }

  async function finishRename() {
    const id = renaming;
    renaming = null;
    const name = renameText.trim();
    if (!id || !name) return;
    await act('Renaming', () => renameProject(id, name));
    onrenamed(id, name);
  }

  const duplicate = (p: ProjectMeta) => act('Duplicating', async () => void (await duplicateProject(p.id)));

  const downloadIt = (p: ProjectMeta) =>
    act('Preparing download', async () => {
      const { data } = await loadFromLibrary(p.id);
      await saveProject(data, `${p.name}.seviath`);
    });

  function remove(p: ProjectMeta) {
    menuFor = null;
    if (!confirm(`Delete "${p.name}" for good? This can't be undone.\n(Download it first if you want to keep a copy.)`)) return;
    act('Deleting', async () => {
      await deleteProject(p.id);
      ondeleted(p.id);
    });
  }

  function pickFiles() {
    if (!fileInput) return;
    fileInput.value = '';
    fileInput.click();
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    dropping = false;
    const files = Array.from(e.dataTransfer?.files ?? []);
    if (files.length) onfiles(files);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (renaming) renaming = null;
      else if (menuFor) menuFor = null;
      else if (canClose) onclose();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div
  class="manager"
  class:dropping
  role="dialog"
  aria-label="Projects"
  tabindex="-1"
  ondragover={(e) => {
    e.preventDefault();
    dropping = true;
  }}
  ondragleave={(e) => {
    if (e.target === e.currentTarget) dropping = false;
  }}
  ondrop={onDrop}
>
  <header>
    <strong class="logo">SeviathStudio</strong>
    <span class="title">Projects</span>
    <input class="search" type="search" placeholder="Search…" bind:value={search} />
    <select bind:value={sortBy} title="Sort">
      <option value="updated">Last edited</option>
      <option value="created">Newest</option>
      <option value="name">Name</option>
    </select>
    <span class="spacer"></span>
    <button onclick={pickFiles} title="PSD, OpenRaster, Krita, PNG, JPG, WebP, GIF, SVG… or drop files here">📂 Open / import file…</button>
    <button class="primary" onclick={onnew}>＋ New project</button>
    {#if canClose}
      <button onclick={onclose} title="Back to drawing (Esc)">✕</button>
    {/if}
  </header>

  {#if busy}<div class="busy">{busy}…</div>{/if}

  <div class="body" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && (menuFor = null)}>
    {#if loading}
      <p class="empty">Loading…</p>
    {:else if error}
      <p class="empty err">{error}</p>
    {:else if projects.length === 0}
      <div class="empty">
        <p>No projects yet.</p>
        <p class="dim">Start a new one, or drop a PSD / image / .seviath file here.</p>
      </div>
    {:else}
      <div class="grid">
        <button class="card new-card" onclick={onnew}>
          <span class="plus">＋</span>
          <span>New project</span>
        </button>
        {#each shown as p (p.id)}
          <div class="card" class:current={p.id === currentId}>
            <button class="thumb" onclick={() => onopen(p.id)} title="Open">
              {#if thumbs[p.id]}<img src={thumbs[p.id]} alt="" />{/if}
            </button>
            <div class="info">
              {#if renaming === p.id}
                <!-- svelte-ignore a11y_autofocus -->
                <input
                  class="rename"
                  bind:value={renameText}
                  autofocus
                  onkeydown={(e) => {
                    if (e.key === 'Enter') finishRename();
                    e.stopPropagation();
                    if (e.key === 'Escape') renaming = null;
                  }}
                  onblur={finishRename}
                />
              {:else}
                <button class="name" ondblclick={() => startRename(p)} onclick={() => onopen(p.id)} title={p.name}>{p.name}</button>
              {/if}
              <span class="dim">{p.width} × {p.height} · {p.layerCount} layer{p.layerCount === 1 ? '' : 's'}</span>
              <span class="dim">{p.id === currentId ? 'Open now · ' : ''}{when(p.updated)}</span>
            </div>
            <button class="more" onclick={() => (menuFor = menuFor === p.id ? null : p.id)} title="More" aria-label="More">⋯</button>
            {#if menuFor === p.id}
              <div class="menu" role="menu">
                <button role="menuitem" onclick={() => onopen(p.id)}>Open</button>
                <button role="menuitem" onclick={() => startRename(p)}>Rename</button>
                <button role="menuitem" onclick={() => duplicate(p)}>Duplicate</button>
                <button role="menuitem" onclick={() => downloadIt(p)}>Download (.seviath)</button>
                <hr />
                <button role="menuitem" class="danger" onclick={() => remove(p)}>Delete…</button>
              </div>
            {/if}
          </div>
        {/each}
      </div>
      {#if shown.length === 0}<p class="empty dim">Nothing matches "{search}".</p>{/if}
    {/if}
  </div>

  <footer class="dim">
    Projects are saved automatically in this browser. Use Download (.seviath) or Export PSD for backups.
    {#if storage && storage.quota}· Using {mb(storage.used)} of {mb(storage.quota)}{/if}
  </footer>

  {#if dropping}
    <div class="drop-hint">Drop to open as a new project</div>
  {/if}
</div>

<input
  bind:this={fileInput}
  type="file"
  multiple
  accept={ACCEPT}
  hidden
  onchange={() => fileInput?.files?.length && onfiles(Array.from(fileInput.files))}
/>

<style>
  .manager {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    flex-direction: column;
    background: #1b1b20;
    color: #eee;
    font-family: system-ui, sans-serif;
    font-size: 13px;
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    border-bottom: 1px solid #333;
    background: #222228;
  }
  .logo {
    color: #00b5c8;
  }
  .title {
    font-size: 15px;
    margin-right: 12px;
  }
  .spacer {
    flex: 1;
  }
  .search,
  select,
  .rename {
    background: #15151a;
    border: 1px solid #444;
    border-radius: 6px;
    color: #eee;
    padding: 6px 8px;
    font-size: 13px;
  }
  .search {
    width: 220px;
  }
  button {
    padding: 7px 12px;
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
  button.primary {
    background: #00b5c8;
    border-color: #00b5c8;
    color: #111;
    font-weight: 600;
  }
  .body {
    flex: 1;
    overflow: auto;
    padding: 20px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 16px;
  }
  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    background: #26262d;
    border: 1px solid #36363f;
    border-radius: 10px;
    overflow: visible;
  }
  .card.current {
    border-color: #00b5c8;
  }
  .thumb {
    height: 190px;
    padding: 0;
    border: none;
    border-radius: 10px 10px 0 0;
    display: flex;
    align-items: center;
    justify-content: center;
    /* checkerboard so transparent works are visible */
    background:
      repeating-conic-gradient(#3a3a42 0% 25%, #2e2e35 0% 50%) 50% / 16px 16px;
    overflow: hidden;
  }
  .thumb img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.4);
  }
  .info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 34px 10px 10px;
    min-width: 0;
  }
  .name {
    padding: 0;
    border: none;
    background: none;
    text-align: left;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rename {
    padding: 2px 6px;
  }
  .more {
    position: absolute;
    right: 6px;
    bottom: 26px;
    padding: 2px 8px;
    background: none;
    border-color: transparent;
    font-size: 16px;
  }
  .menu {
    position: absolute;
    right: 6px;
    bottom: 56px;
    z-index: 5;
    display: flex;
    flex-direction: column;
    min-width: 170px;
    padding: 4px;
    background: #2b2b33;
    border: 1px solid #555;
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
  }
  .menu button {
    border: none;
    background: none;
    text-align: left;
    padding: 7px 10px;
  }
  .menu button:hover {
    background: #3a3a44;
  }
  .menu hr {
    border: none;
    border-top: 1px solid #444;
    margin: 4px 0;
  }
  .danger {
    color: #ff7b7b;
  }
  .new-card {
    min-height: 250px;
    border: 2px dashed #444;
    background: none;
    align-items: center;
    justify-content: center;
    gap: 6px;
    color: #aaa;
  }
  .new-card:hover {
    border-color: #00b5c8;
    color: #eee;
  }
  .plus {
    font-size: 36px;
  }
  .dim {
    color: #888;
    font-size: 12px;
  }
  .empty {
    text-align: center;
    margin-top: 80px;
    font-size: 15px;
  }
  .err {
    color: #ff9b9b;
  }
  footer {
    padding: 8px 16px;
    border-top: 1px solid #333;
  }
  .busy {
    position: absolute;
    top: 60px;
    left: 50%;
    transform: translateX(-50%);
    padding: 6px 14px;
    background: #00b5c8;
    color: #111;
    border-radius: 999px;
    z-index: 6;
  }
  .dropping .body {
    outline: 3px dashed #00b5c8;
    outline-offset: -10px;
  }
  .drop-hint {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 22px;
    pointer-events: none;
    background: rgba(0, 181, 200, 0.08);
  }
</style>
