// Project library: every work is kept in the browser's own database (IndexedDB),
// so it is there next time you open SeviathStudio, like the CSP project manager.
import type { ProjectData, SavedLayer } from './fileio';

export type ProjectMeta = {
  id: string;
  name: string;
  width: number;
  height: number;
  layerCount: number;
  created: number;
  updated: number;
  thumb: Blob | null;
};

type StoredLayer = Omit<SavedLayer, 'canvas'> & { png: Blob };
type StoredData = {
  id: string;
  width: number;
  height: number;
  paper: ProjectData['paper'];
  layers: StoredLayer[]; // vector layers also keep their lines (`vector`)
  dpi?: number;
  colorMode?: ProjectData['colorMode'];
};

const DB_NAME = 'seviath-studio';
let dbPromise: Promise<IDBDatabase> | null = null;

function db(): Promise<IDBDatabase> {
  if (dbPromise) return dbPromise;
  dbPromise = new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => {
      const d = req.result;
      if (!d.objectStoreNames.contains('meta')) d.createObjectStore('meta', { keyPath: 'id' });
      if (!d.objectStoreNames.contains('data')) d.createObjectStore('data', { keyPath: 'id' });
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => {
      dbPromise = null;
      reject(req.error ?? new Error('Could not open the project library'));
    };
  });
  // Ask the browser not to clear our storage when the disk gets full
  navigator.storage?.persist?.().catch(() => {});
  return dbPromise;
}

function done(tx: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error ?? new Error('Saving was cancelled (the disk may be full)'));
  });
}

function get<T>(store: 'meta' | 'data', id: string): Promise<T | undefined> {
  return db().then(
    (d) =>
      new Promise((resolve, reject) => {
        const req = d.transaction(store).objectStore(store).get(id);
        req.onsuccess = () => resolve(req.result as T | undefined);
        req.onerror = () => reject(req.error);
      }),
  );
}

export function newId() {
  return Date.now().toString(36) + Math.random().toString(36).slice(2, 8);
}

export function canvasToBlob(canvas: HTMLCanvasElement, type = 'image/png', quality?: number): Promise<Blob> {
  return new Promise((resolve, reject) =>
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error('Could not encode image'))), type, quality),
  );
}

async function blobToCanvas(blob: Blob, w: number, h: number) {
  const canvas = document.createElement('canvas');
  canvas.width = w;
  canvas.height = h;
  const bmp = await createImageBitmap(blob);
  canvas.getContext('2d')!.drawImage(bmp, 0, 0);
  bmp.close();
  return canvas;
}

// A small picture of the whole drawing for the project list
export async function makeThumb(composite: HTMLCanvasElement, max = 360) {
  const s = Math.min(1, max / Math.max(composite.width, composite.height));
  const c = document.createElement('canvas');
  c.width = Math.max(1, Math.round(composite.width * s));
  c.height = Math.max(1, Math.round(composite.height * s));
  const ctx = c.getContext('2d')!;
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(composite, 0, 0, c.width, c.height);
  return canvasToBlob(c, 'image/png');
}

export async function listProjects(): Promise<ProjectMeta[]> {
  const d = await db();
  const all = await new Promise<ProjectMeta[]>((resolve, reject) => {
    const req = d.transaction('meta').objectStore('meta').getAll();
    req.onsuccess = () => resolve(req.result as ProjectMeta[]);
    req.onerror = () => reject(req.error);
  });
  return all.sort((a, b) => b.updated - a.updated);
}

// Saves a project. `pngFor` lets the caller reuse already-encoded layers that didn't change.
export async function saveToLibrary(
  id: string,
  name: string,
  data: ProjectData,
  composite: HTMLCanvasElement,
  pngFor: (layer: SavedLayer, index: number) => Promise<Blob> = (l) => canvasToBlob(l.canvas),
) {
  const layers: StoredLayer[] = [];
  for (let i = 0; i < data.layers.length; i++) {
    const { canvas, ...props } = data.layers[i];
    layers.push({ ...props, png: await pngFor(data.layers[i], i) });
  }
  const thumb = await makeThumb(composite);
  const old = await get<ProjectMeta>('meta', id);
  const now = Date.now();
  const meta: ProjectMeta = {
    id,
    name,
    width: data.width,
    height: data.height,
    layerCount: data.layers.length,
    created: old?.created ?? now,
    updated: now,
    thumb,
  };
  const stored: StoredData = {
    id,
    width: data.width,
    height: data.height,
    paper: { ...data.paper },
    layers,
    dpi: data.dpi ?? 72,
    colorMode: data.colorMode ?? 'rgb',
  };
  const tx = (await db()).transaction(['meta', 'data'], 'readwrite');
  tx.objectStore('meta').put(meta);
  tx.objectStore('data').put(stored);
  await done(tx);
  return meta;
}

export async function loadFromLibrary(id: string): Promise<{ meta: ProjectMeta; data: ProjectData }> {
  const meta = await get<ProjectMeta>('meta', id);
  const stored = await get<StoredData>('data', id);
  if (!meta || !stored) throw new Error('This project was not found in the library.');
  const layers: SavedLayer[] = [];
  for (const l of stored.layers) {
    const { png, ...props } = l;
    layers.push({ ...props, canvas: await blobToCanvas(png, stored.width, stored.height) });
  }
  return {
    meta,
    data: { width: stored.width, height: stored.height, paper: stored.paper, layers, dpi: stored.dpi ?? 72, colorMode: stored.colorMode ?? 'rgb' },
  };
}

export async function renameProject(id: string, name: string) {
  const meta = await get<ProjectMeta>('meta', id);
  if (!meta) return;
  const tx = (await db()).transaction('meta', 'readwrite');
  tx.objectStore('meta').put({ ...meta, name });
  await done(tx);
}

export async function deleteProject(id: string) {
  const tx = (await db()).transaction(['meta', 'data'], 'readwrite');
  tx.objectStore('meta').delete(id);
  tx.objectStore('data').delete(id);
  await done(tx);
}

export async function duplicateProject(id: string) {
  const meta = await get<ProjectMeta>('meta', id);
  const stored = await get<StoredData>('data', id);
  if (!meta || !stored) throw new Error('This project was not found in the library.');
  const copyId = newId();
  const now = Date.now();
  const tx = (await db()).transaction(['meta', 'data'], 'readwrite');
  tx.objectStore('meta').put({ ...meta, id: copyId, name: meta.name + ' copy', created: now, updated: now });
  tx.objectStore('data').put({ ...stored, id: copyId });
  await done(tx);
  return copyId;
}

// How much space the library uses (and how much the browser allows), in bytes
export async function storageInfo() {
  try {
    const e = await navigator.storage?.estimate?.();
    return e ? { used: e.usage ?? 0, quota: e.quota ?? 0 } : null;
  } catch {
    return null;
  }
}
