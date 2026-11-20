// Server-only shared canvas state: data/state.json. Never import from client code.
//
// Records are addressed by a path "<storeKey>/<recordId>" (e.g. "designs/abc123"). Only store keys
// that KEY_POLICY (src/store/key-policy.ts) marks as shared are accepted; how they map to records is
// up to the client (src/store/sync.ts). Every write bumps a global `version`; each record
// remembers the version (`rev`) that last touched it, so clients can ask "what changed since N".
// Deletes are kept as tombstones so a browser that was offline can't resurrect a deleted record.
import { randomUUID } from "node:crypto";
import { appendFile, mkdir, readFile, rename, writeFile } from "node:fs/promises";
import path from "node:path";
import { SHARED_KEYS } from "@/store/key-policy";

export interface StoredRecord {
  rev: number;
  value?: unknown;
  deleted?: true;
}

export interface StateFile {
  /** Changes when the state file is recreated: clients holding an old cursor must resync fully. */
  epoch: string;
  version: number;
  records: Record<string, StoredRecord>;
}

export interface Change {
  path: string;
  value?: unknown;
  deleted?: boolean;
}

export interface ChangeEvent {
  epoch: string;
  version: number;
  clientId?: string;
}

const FILE = path.join(process.cwd(), "data", "state.json");
/** Audit trail: one NDJSON line per accepted change (never rewritten, only appended). */
const LOG = path.join(process.cwd(), "data", "state-log.ndjson");

/** Who sent a batch, for the audit log. */
export interface ChangeMeta {
  clientId?: string;
  userAgent?: string;
  /** Free-form tag the client attaches to the batch (why it was sent), e.g. "push" or "cache-dirty". */
  reason?: string;
}
const SHARED = new Set<string>(SHARED_KEYS);

// Kept on globalThis so dev-mode HMR and separately bundled route modules share one copy.
interface Holder {
  state?: Promise<StateFile>;
  listeners: Set<(e: ChangeEvent) => void>;
  writeTimer?: ReturnType<typeof setTimeout>;
  writing: Promise<void>;
  logging?: Promise<void>;
  /** Set once stray keys were pruned in this process (also covers a state loaded by older code). */
  pruned?: true;
}
const g = globalThis as { __prototypieState?: Holder };
const holder: Holder = (g.__prototypieState ??= { listeners: new Set(), writing: Promise.resolve() });

async function load(): Promise<StateFile> {
  holder.state ??= readFile(FILE, "utf8").then(
    (txt) => JSON.parse(txt) as StateFile,
    (err: NodeJS.ErrnoException) => {
      if (err.code !== "ENOENT") throw err;
      return { epoch: randomUUID(), version: 0, records: {} };
    },
  );
  const s = await holder.state;
  if (!holder.pruned) {
    holder.pruned = true;
    // Drop records of keys that are not (or no longer) shared, e.g. after a slice rename. Removed
    // outright, not tombstoned: clients ignore those keys anyway.
    const stray = Object.keys(s.records).filter((p) => !isValidPath(p));
    for (const p of stray) delete s.records[p];
    if (stray.length) scheduleWrite();
  }
  return s;
}

export function isValidPath(p: unknown): p is string {
  if (typeof p !== "string" || p.length >= 512) return false;
  const slash = p.indexOf("/");
  return slash > 0 && slash < p.length - 1 && SHARED.has(p.slice(0, slash));
}

/** Records changed after `since` (all of them, tombstones included, for since=0 or a foreign epoch). */
export async function readState(since = 0, epoch?: string): Promise<StateFile> {
  const s = await load();
  if (since <= 0 || (epoch && epoch !== s.epoch)) return s;
  const records: Record<string, StoredRecord> = {};
  for (const [p, r] of Object.entries(s.records)) if (r.rev > since) records[p] = r;
  return { epoch: s.epoch, version: s.version, records };
}

/** Apply a batch of changes as one version. Last write per record wins. */
export async function applyChanges(changes: Change[], meta: ChangeMeta = {}): Promise<{ epoch: string; version: number }> {
  const { clientId } = meta;
  const s = await load();
  if (!changes.length) return { epoch: s.epoch, version: s.version };
  const rev = ++s.version;
  const ts = new Date().toISOString();
  const lines: string[] = [];
  for (const c of changes) {
    const prev = s.records[c.path];
    const before = !prev ? "absent" : prev.deleted ? "deleted" : "live";
    const op = c.deleted ? "delete" : before === "live" ? "update" : "create";
    // Title of what was there (deletes) or what is written (sets), so a log line is readable on its own.
    const title = titleOf(c.deleted ? prev?.value : c.value);
    lines.push(JSON.stringify({ ts, rev, clientId, path: c.path, op, before, prevRev: prev?.rev, title, reason: meta.reason, ua: meta.userAgent }));
    s.records[c.path] = c.deleted ? { rev, deleted: true } : { rev, value: c.value };
  }
  appendLog(lines);
  scheduleWrite();
  const event: ChangeEvent = { epoch: s.epoch, version: rev, clientId };
  for (const l of holder.listeners) l(event);
  return { epoch: s.epoch, version: rev };
}

export function subscribe(fn: (e: ChangeEvent) => void): () => void {
  holder.listeners.add(fn);
  return () => holder.listeners.delete(fn);
}

/** Current cursor, for the SSE hello message. */
export async function currentVersion(): Promise<{ epoch: string; version: number }> {
  const s = await load();
  return { epoch: s.epoch, version: s.version };
}

function titleOf(v: unknown): string | undefined {
  if (!v || typeof v !== "object") return undefined;
  const o = v as { title?: unknown; name?: unknown };
  const t = o.title ?? o.name;
  return typeof t === "string" ? t.slice(0, 80) : undefined;
}

function appendLog(lines: string[]) {
  holder.logging = (holder.logging ?? Promise.resolve())
    .then(async () => {
      await mkdir(path.dirname(LOG), { recursive: true });
      await appendFile(LOG, lines.join("\n") + "\n", "utf8");
    })
    .catch((err) => console.error("[state] could not append to data/state-log.ndjson:", err));
}

// The file holds several MB of HTML: coalesce bursts of writes into one atomic rewrite.
function scheduleWrite() {
  if (holder.writeTimer) return;
  holder.writeTimer = setTimeout(() => {
    holder.writeTimer = undefined;
    holder.writing = holder.writing
      .catch(() => undefined)
      .then(async () => {
        const s = await load();
        await mkdir(path.dirname(FILE), { recursive: true });
        const tmp = `${FILE}.${process.pid}.${Date.now()}.tmp`;
        await writeFile(tmp, JSON.stringify(s), "utf8");
        await rename(tmp, FILE); // readers never see half a file
      })
      .catch((err) => console.error("[state] could not write data/state.json:", err));
  }, 300);
}
