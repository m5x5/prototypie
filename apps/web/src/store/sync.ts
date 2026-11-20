"use client";
// Shared state sync: the server (data/state.json via /api/state) is the source of truth for canvas
// data, IndexedDB is this browser's offline cache, and the zustand store is the working copy.
//
// Model
// - Every shared store key is split into records "<key>/<id>" (see KEY_POLICY in ./key-policy.ts).
//   Records merge one by one: the server keeps the last write per record and tombstones for deletes.
// - `base` holds, per record, the value we last know the server has. A record in the store that
//   differs from `base` (by reference: the store never mutates, so a changed record is a new object)
//   is a local edit waiting to be pushed. Pushes are debounced (PUSH_DEBOUNCE), changed records only.
// - Deletes are never inferred from "in base but not in the store". Only a store action that removes
//   a record the store held (so one this page received from the server or created) records it in
//   `removed`, and only `removed` paths are sent as deletes. A record missing from the store for any
//   other reason (HMR re-created the store, a stale cache, a race) is restored from `base` instead.
// - The server announces each new version over SSE (/api/state/events); we then pull the records
//   changed since our cursor. A remote record never overwrites a record with a pending local change.
// - The IndexedDB cache stores one entry per record and writes only records whose identity changed
//   (see the cache section), when idle. It records which edits and deletes are still unpushed, so a
//   reload or offline work loses nothing and stale cache can't win over the server.
// - Hydration waits for the server (up to INIT_TIMEOUT) and merges it into the cached state, so the
//   first render already shows the shared canvas. A browser that never synced before uploads every
//   record the server doesn't have yet (its unique work), and takes the server's copy of the rest.
// - Runs (in-flight generations) are shared so every browser shows their placeholders, but only the
//   browser that started a run drives it (see isOwnRun); others just display what it pushes.
import { clear, createStore, del, entries, get, promisifyRequest, type UseStore } from "idb-keyval";
import type { StoreApi } from "zustand";
import type { PersistStorage, StorageValue } from "zustand/middleware";
import type { AppState } from "./index";
import { KEY_POLICY, LIST_ORDER, SHARED_KEYS, type DataKey } from "./key-policy";

const SHARED = SHARED_KEYS;
const PUSH_DEBOUNCE = 500;
/** Trailing debounce for cache writes (IDB_MAX_WAIT caps it during continuous changes). */
const IDB_DEBOUNCE = 1000;
/** How long hydration waits for the server before showing the cache (and merging once it answers). */
const INIT_TIMEOUT = 10_000;
const MAX_BATCH_CHARS = 4_000_000;
/** Sent with every PUT so the server's audit log can tell engine versions apart. */
const ENGINE = "sync-v2";

type Persisted = Partial<AppState>;
type Records = Map<string, unknown>;
interface ServerRecord {
  rev: number;
  value?: unknown;
  deleted?: true;
}
interface ServerState {
  epoch: string;
  version: number;
  records: Record<string, ServerRecord>;
}
interface Pending {
  /** Records edited or created locally and not yet acknowledged by the server. */
  dirty: Set<string>;
  /** Records a store action removed and the server hasn't tombstoned yet. */
  removed: Set<string>;
}
interface Cached extends StorageValue<Persisted> {
  /** Absent until this browser synced once: then every record the server lacks is its own work. */
  sync?: { dirty: string[]; removed?: string[]; ownRuns?: string[] };
}

/** Everything the persist middleware stores: data keys that aren't transient. */
export function persistable(s: AppState): Persisted {
  return Object.fromEntries(
    Object.entries(s).filter(
      ([k, v]) => typeof v !== "function" && KEY_POLICY[k as DataKey] !== "transient",
    ),
  ) as Persisted;
}

// ---------------------------------------------------------------------------------------------
// Records <-> store keys

const keyOf = (p: string) => p.slice(0, p.indexOf("/")) as DataKey;
const isShared = (p: string) => p.includes("/") && SHARED.includes(keyOf(p));

function idsOf(k: DataKey, v: unknown): Map<string, unknown> {
  const out = new Map<string, unknown>();
  if (!v) return out;
  if (KEY_POLICY[k] === "sharedList") for (const item of v as { id: string }[]) out.set(item.id, item);
  else for (const [id, rec] of Object.entries(v as Record<string, unknown>)) out.set(id, rec);
  return out;
}

function recordsOf(state: Persisted): Records {
  const out: Records = new Map();
  for (const k of SHARED) for (const [id, rec] of idsOf(k, state[k])) out.set(`${k}/${id}`, rec);
  return out;
}

/** Store patch for `updates` (path -> value, undefined = delete), built on top of `state`. */
function patchFor(state: Persisted, updates: Map<string, unknown>): Persisted {
  const byKey = new Map<DataKey, [string, unknown][]>();
  for (const [p, v] of updates) {
    const k = keyOf(p);
    if (!byKey.has(k)) byKey.set(k, []);
    byKey.get(k)!.push([p.slice(k.length + 1), v]);
  }
  const patch: Record<string, unknown> = {};
  for (const [k, entries] of byKey) {
    const map = Object.fromEntries(idsOf(k, state[k]));
    for (const [id, v] of entries) {
      if (v === undefined) delete map[id];
      else map[id] = v;
    }
    patch[k] =
      KEY_POLICY[k] === "sharedList" ? Object.values(map).sort(LIST_ORDER[k] as (a: unknown, b: unknown) => number) : map;
  }
  return patch as Persisted;
}

/** Paths whose store value differs from what the server has (edits and creates, never deletes). */
function edited(cur: Records, against: Records): string[] {
  const out: string[] = [];
  for (const [p, v] of cur) if (against.get(p) !== v) out.push(p);
  return out;
}

// ---------------------------------------------------------------------------------------------
// Engine state, one per page. Module state survives HMR of other modules; when this module itself
// is re-evaluated it starts fresh and re-reads the cache (flushed by the previous engine first).

const g = globalThis as {
  __prototypieSync?: { stop: () => void; flush: () => Promise<void> };
};
const clientId = typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : String(Math.random());
let base: Records = new Map();
let cursor = { epoch: "", version: 0 };
/** True once this page merged with the server. Until then pushes wait for a full resync. */
let connected = false;
/** False until this browser has synced once (first-time import rules apply until then). */
let everSynced = false;
const removed = new Set<string>();
/** Runs started by this browser: the only ones it reattaches to after a reload. */
const ownRuns = new Set<string>();
const ownRevs = new Set<number>();
/** The store the engine is attached to. The cache is only ever written from this store. */
let live: StoreApi<AppState> | null = null;
let cacheName = "prototypie";

export const isOwnRun = (runId: string) => ownRuns.has(runId);

/**
 * Merge a full server snapshot into `local`. `pending` null = this browser never synced: local
 * records the server lacks are uploaded, everything else comes from the server. Otherwise local
 * edits and deletes still unpushed win; the rest comes from the server.
 */
function mergeServer(local: Persisted, pending: Pending | null, server: ServerState): Persisted {
  const localRecs = recordsOf(local);
  const nextBase: Records = new Map();
  const result: Records = new Map();
  for (const [p, r] of Object.entries(server.records)) {
    if (!isShared(p)) continue; // a key this client version doesn't know
    if (!r.deleted) nextBase.set(p, r.value);
    if (pending?.removed.has(p)) continue; // our delete is newer
    if (pending?.dirty.has(p) && localRecs.has(p)) result.set(p, localRecs.get(p));
    else if (!r.deleted) result.set(p, r.value);
  }
  // Never seen by the server (no record, no tombstone): local work, keep it so it gets pushed.
  for (const [p, v] of localRecs) if (!(p in server.records) && !pending?.removed.has(p)) result.set(p, v);
  base = nextBase;
  cursor = { epoch: server.epoch, version: server.version };
  connected = true;
  everSynced = true;
  removed.clear();
  for (const p of pending?.removed ?? []) if (!result.has(p)) removed.add(p);
  const updates = new Map<string, unknown>(result);
  for (const p of localRecs.keys()) if (!result.has(p)) updates.set(p, undefined);
  return { ...local, ...patchFor(local, updates) };
}

async function fetchState(since = 0, timeoutMs?: number): Promise<ServerState> {
  const qs = since > 0 ? `?since=${since}&epoch=${encodeURIComponent(cursor.epoch)}` : "";
  const res = await fetch(`/api/state${qs}`, {
    cache: "no-store",
    signal: timeoutMs ? AbortSignal.timeout(timeoutMs) : undefined,
  });
  if (!res.ok) throw new Error(`GET /api/state: ${res.status}`);
  return (await res.json()) as ServerState;
}

// ---------------------------------------------------------------------------------------------
// IndexedDB cache: one entry per shared record ("r:<path>") plus one small "meta" entry (per-browser
// keys and the pending lists), in a database of its own. A write only puts the records whose object
// identity changed since the last write, all in one transaction, so a pan, a drag or an incoming
// remote change costs a few small puts instead of structured-cloning megabytes of HTML. Writes are a
// trailing debounce (IDB_DEBOUNCE, at most IDB_MAX_WAIT apart) run when the main thread is idle, and
// always read the live store (never a value handed over before hydration or by an HMR-orphaned store).

const META = "meta";
const REC = "r:";
/** Single-entry cache written by the first engine versions (whole state in the default idb-keyval store). */
const legacyKey = () => cacheName;
let db: UseStore | null = null;
const cacheDb = () => (db ??= createStore(`${cacheName}-cache`, "kv"));
/** Record path -> object last written to IDB (identity), to write only what changed. */
let written = new Map<string, unknown>();
let legacyDropped = false;

interface CacheMeta {
  format: 2;
  version: number;
  /** Per-browser keys (KEY_POLICY "local"); small. */
  local: Persisted;
  /** When `local` was taken (ms), to compare with the unload snapshot below. */
  localAt?: number;
  sync?: Cached["sync"];
}

// An IndexedDB transaction started in pagehide doesn't survive a reload (the page unloads before it
// commits), so text typed less than IDB_DEBOUNCE before a reload was lost. On hide the small per-browser
// keys are also written synchronously to localStorage; hydration takes them when newer than the cache.
const localSnapshotKey = () => `${cacheName}-local`;
const localOf = (state: Persisted) =>
  Object.fromEntries(Object.entries(state).filter(([k]) => KEY_POLICY[k as DataKey] === "local")) as Persisted;
function snapshotLocal() {
  if (!live) return;
  try {
    localStorage.setItem(localSnapshotKey(), JSON.stringify({ at: Date.now(), local: localOf(persistable(live.getState())) }));
  } catch {
    // storage full or blocked: the IDB write below is still attempted
  }
}
function readLocalSnapshot(): { at: number; local: Persisted } | null {
  try {
    const raw = localStorage.getItem(localSnapshotKey());
    return raw ? (JSON.parse(raw) as { at: number; local: Persisted }) : null;
  } catch {
    return null;
  }
}

let idbTimer: ReturnType<typeof setTimeout> | undefined;
let idbFirstAt = 0;
let idleHandle: number | undefined;
let writing: Promise<void> = Promise.resolve();

function writeCache(): Promise<void> {
  clearTimeout(idbTimer);
  idbTimer = undefined;
  idbFirstAt = 0;
  if (idleHandle !== undefined && typeof cancelIdleCallback === "function") cancelIdleCallback(idleHandle);
  idleHandle = undefined;
  if (!live) return writing; // before hydration finished: the store holds no real state yet
  const state = persistable(live.getState());
  const recs = recordsOf(state);
  const puts: [string, unknown][] = [];
  for (const [p, v] of recs) if (written.get(p) !== v) puts.push([p, v]);
  const dels: string[] = [];
  for (const p of written.keys()) if (!recs.has(p)) dels.push(p);
  const meta: CacheMeta = {
    format: 2,
    version: 0,
    local: localOf(state),
    localAt: Date.now(),
  };
  if (everSynced) meta.sync = { dirty: edited(recs, base), removed: [...removed], ownRuns: [...ownRuns] };
  // Optimistic: the next write diffs against this one. On failure, rewrite everything next time.
  for (const [p, v] of puts) written.set(p, v);
  for (const p of dels) written.delete(p);
  const dropLegacy = !legacyDropped;
  writing = writing
    .then(() =>
      cacheDb()("readwrite", (os) => {
        for (const [p, v] of puts) os.put(v, REC + p);
        for (const p of dels) os.delete(REC + p);
        os.put(meta, META);
        return promisifyRequest(os.transaction);
      }),
    )
    .then(async () => {
      if (dropLegacy) {
        legacyDropped = true;
        await del(legacyKey());
      }
    })
    .catch((err) => {
      console.error("[sync] IndexedDB write failed:", err);
      written = new Map();
    });
  return writing;
}

const IDB_MAX_WAIT = 5000;
function scheduleCache() {
  const now = Date.now();
  idbFirstAt ||= now;
  clearTimeout(idbTimer);
  const wait = Math.max(0, Math.min(IDB_DEBOUNCE, idbFirstAt + IDB_MAX_WAIT - now));
  idbTimer = setTimeout(() => {
    idbTimer = undefined;
    if (typeof requestIdleCallback === "function") {
      idleHandle ??= requestIdleCallback(() => void writeCache(), { timeout: 2000 });
    } else void writeCache();
  }, wait);
}

/** Cache contents as one persisted value (new per-record format, or the legacy single entry). */
async function readCache(): Promise<Cached | null> {
  const all = await entries<string, unknown>(cacheDb());
  const meta = all.find(([k]) => k === META)?.[1] as CacheMeta | undefined;
  if (meta) {
    const recs = new Map<string, unknown>();
    for (const [k, v] of all) if (k.startsWith(REC)) recs.set(k.slice(REC.length), v);
    written = new Map(recs);
    const snap = readLocalSnapshot();
    const local = snap && snap.at > (meta.localAt ?? 0) ? { ...meta.local, ...snap.local } : meta.local;
    return { state: { ...local, ...patchFor({}, recs) }, version: meta.version, sync: meta.sync };
  }
  written = new Map();
  const raw = await get<Cached | string>(legacyKey());
  return typeof raw === "string" ? (JSON.parse(raw) as Cached) : (raw ?? null);
}

/** zustand persist storage backed by the server plus the per-record IndexedDB cache above. */
export const syncedStorage: PersistStorage<Persisted> = {
  getItem: async (name) => {
    if (typeof window === "undefined") return null;
    if (name !== cacheName) db = null;
    cacheName = name;
    // A new store is hydrating (first load, or HMR re-created the store). Detach the previous
    // engine first and flush its cache, so it can't touch the state this merge builds on.
    const prev = g.__prototypieSync;
    g.__prototypieSync = undefined;
    if (prev) {
      prev.stop();
      await prev.flush();
    }
    live = null;
    await writing;

    let cached: Cached | null = null;
    try {
      cached = await readCache();
    } catch (err) {
      console.error("[sync] IndexedDB read failed:", err);
    }
    const pending: Pending | null = cached?.sync
      ? { dirty: new Set(cached.sync.dirty), removed: new Set(cached.sync.removed ?? []) }
      : null;
    for (const id of cached?.sync?.ownRuns ?? []) ownRuns.add(id);
    const local: Persisted = cached?.state ?? {};
    try {
      const server = await fetchState(0, INIT_TIMEOUT);
      return { state: mergeServer(local, pending, server), version: cached?.version ?? 0 };
    } catch (err) {
      console.warn("[sync] server state unavailable, using the local cache:", err);
      // Offline: assume the server has what the cache doesn't list as unpushed.
      everSynced = !!pending;
      connected = false;
      base = pending ? recordsOf(local) : new Map();
      for (const p of pending?.dirty ?? []) base.delete(p);
      removed.clear();
      for (const p of pending?.removed ?? []) removed.add(p);
      return cached ? { state: local, version: cached.version ?? 0 } : null;
    }
  },
  // The middleware hands over a snapshot on every set; we only use it as a trigger (see writeCache).
  setItem: () => scheduleCache(),
  removeItem: async () => {
    written = new Map();
    await clear(cacheDb());
    await del(legacyKey());
  },
};

// ---------------------------------------------------------------------------------------------
// Live sync

export function startSync(store: StoreApi<AppState>) {
  g.__prototypieSync?.stop();
  live = store;

  let pushTimer: ReturnType<typeof setTimeout> | undefined;
  let pushing = false;
  let pulling = false;
  let pullAgain = false;
  let retryMs = 1000;
  let stopped = false;
  /** Set while the engine itself writes to the store, so those writes aren't taken as user actions. */
  let applying = false;
  const apply = (patch: Persisted) => {
    applying = true;
    try {
      store.setState(patch as Partial<AppState>);
    } finally {
      applying = false;
    }
    // Runs can arrive after hydration, through either a pull or a full resync. Check foreign jobs
    // without following them; reap outside `applying` so removals/errors are pushed back to the server.
    if (patch.runs) void store.getState().reapLostRuns((run) => !isOwnRun(run.id));
  };

  const schedulePush = (ms = PUSH_DEBOUNCE) => {
    if (pushTimer || stopped) return;
    pushTimer = setTimeout(() => {
      pushTimer = undefined;
      void push();
    }, ms);
  };

  /** Records the server has that the store lost without a delete action: put them back. */
  function heal(cur: Records) {
    const missing = new Map<string, unknown>();
    for (const [p, v] of base) if (!cur.has(p) && !removed.has(p)) missing.set(p, v);
    if (!missing.size) return;
    console.warn(`[sync] restoring ${missing.size} record(s) missing from the store without a delete:`, [...missing.keys()]);
    apply(patchFor(store.getState(), missing));
  }

  async function push() {
    if (pushing || stopped) return;
    if (!connected) return void resync();
    const cur = recordsOf(store.getState());
    heal(cur);
    const upserts = edited(cur, base);
    for (const p of removed) if (cur.has(p)) removed.delete(p); // re-created since
    const deletes = [...removed];
    if (!upserts.length && !deletes.length) return;
    pushing = true;
    try {
      // Large first-time imports go up in several requests.
      let batch: { path: string; value?: unknown; deleted?: true }[] = [];
      let size = 0;
      const send = async () => {
        if (!batch.length) return;
        const body = JSON.stringify({ clientId, reason: ENGINE, changes: batch });
        const res = await fetch("/api/state", { method: "PUT", headers: { "Content-Type": "application/json" }, body });
        if (!res.ok) throw new Error(`PUT /api/state: ${res.status} ${await res.text()}`);
        const ack = (await res.json()) as { epoch: string; version: number };
        if (ack.epoch !== cursor.epoch) {
          connected = false; // server state was reset: merge again, our changes still count as pending
          return;
        }
        ownRevs.add(ack.version);
        if (cursor.version === ack.version - 1) cursor.version = ack.version;
        for (const c of batch) {
          if (c.deleted) {
            base.delete(c.path);
            removed.delete(c.path);
          } else base.set(c.path, c.value);
        }
        batch = [];
        size = 0;
      };
      for (const p of deletes) batch.push({ path: p, deleted: true });
      for (const p of upserts) {
        const v = cur.get(p);
        const est = JSON.stringify(v).length;
        if (size + est > MAX_BATCH_CHARS) await send();
        batch.push({ path: p, value: v });
        size += est;
      }
      await send();
      retryMs = 1000;
      scheduleCache(); // refresh the cached pending lists
    } catch (err) {
      console.warn("[sync] push failed, retrying:", err);
      retryMs = Math.min(retryMs * 2, 30_000);
      schedulePush(retryMs);
    } finally {
      pushing = false;
    }
    if (!connected) return void resync();
    if (removed.size || edited(recordsOf(store.getState()), base).length) schedulePush();
    if (pullAgain) void pull();
  }

  /** Full merge on the live store (server reset, or we started offline). */
  async function resync() {
    if (pulling || stopped) return;
    pulling = true;
    try {
      const server = await fetchState(0);
      if (stopped) return;
      const state = store.getState();
      const pending: Pending | null = everSynced
        ? { dirty: new Set(edited(recordsOf(state), base)), removed: new Set(removed) }
        : null;
      const merged = mergeServer(state, pending, server);
      apply(Object.fromEntries(SHARED.map((k) => [k, merged[k]])) as Persisted);
      retryMs = 1000;
      schedulePush(0);
    } catch {
      retryMs = Math.min(retryMs * 2, 30_000);
      setTimeout(() => void resync(), retryMs);
    } finally {
      pulling = false;
    }
  }

  async function pull() {
    if (stopped) return;
    if (!connected) return void resync();
    // Wait for our own push to be acknowledged, or its echo would look like a conflicting change.
    if (pulling || pushing) {
      pullAgain = true;
      return;
    }
    pulling = true;
    pullAgain = false;
    try {
      const res = await fetchState(cursor.version);
      if (stopped) return;
      if (res.epoch !== cursor.epoch) {
        connected = false;
        pulling = false;
        return void resync();
      }
      const state = store.getState();
      const cur = recordsOf(state);
      const updates = new Map<string, unknown>();
      for (const [p, r] of Object.entries(res.records)) {
        if (!isShared(p) || ownRevs.has(r.rev)) continue;
        const pending = removed.has(p) || cur.get(p) !== base.get(p);
        if (r.deleted) base.delete(p);
        else base.set(p, r.value);
        if (!pending) updates.set(p, r.deleted ? undefined : r.value); // else: ours is newer, gets pushed
      }
      cursor.version = Math.max(cursor.version, res.version);
      if (updates.size) apply(patchFor(state, updates));
    } catch (err) {
      console.warn("[sync] pull failed:", err);
    } finally {
      pulling = false;
    }
    if (pullAgain) void pull();
  }

  const unsub = store.subscribe((s, prev) => {
    let changed = false;
    for (const k of SHARED) {
      if (s[k] === prev[k]) continue;
      changed = true;
      if (applying) continue;
      // A store action changed this key: note removals (records it held) and new runs it started.
      const before = idsOf(k, prev[k]);
      const after = idsOf(k, s[k]);
      for (const id of before.keys()) if (!after.has(id)) removed.add(`${k}/${id}`);
      for (const id of after.keys()) {
        if (before.has(id)) continue;
        removed.delete(`${k}/${id}`);
        if (k === "runs") ownRuns.add(id);
      }
    }
    if (changed && !applying) schedulePush();
  });

  let es: EventSource | null = null;
  const connect = () => {
    es = new EventSource("/api/state/events");
    es.onmessage = (ev) => {
      const e = JSON.parse(ev.data) as { epoch: string; version: number; clientId?: string };
      if (e.epoch !== cursor.epoch) {
        connected = false;
        void resync();
      } else if (e.version > cursor.version && !(e.clientId === clientId && ownRevs.has(e.version))) void pull();
    };
    // EventSource reconnects by itself; the hello message on reconnect triggers a catch-up pull.
  };
  connect();

  const flushCache = () => {
    snapshotLocal();
    void writeCache();
  };
  const onHide = () => {
    if (document.visibilityState === "hidden") {
      flushCache(); // the tab may be discarded
      clearTimeout(pushTimer);
      pushTimer = undefined;
      void push();
    } else void pull();
  };
  document.addEventListener("visibilitychange", onHide);
  window.addEventListener("pagehide", flushCache);

  // Hydration doesn't go through setItem: write the merged state once now.
  scheduleCache();
  if (!connected) void resync();
  else schedulePush(0); // upload what hydration found unpushed (first-time import, offline edits)

  const stop = () => {
    stopped = true;
    unsub();
    es?.close();
    clearTimeout(pushTimer);
    document.removeEventListener("visibilitychange", onHide);
    window.removeEventListener("pagehide", flushCache);
  };
  g.__prototypieSync = {
    stop,
    flush: async () => {
      if (live === store) await writeCache();
    },
  };
}
