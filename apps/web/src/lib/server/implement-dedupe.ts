/**
 * One Implement request per design (and its promoted source/copies): POST /api/implement asks here before it posts
 * an Agentation note. No imports, so the logic runs (and can be tested) in plain Node with mocked notes.
 */

export interface ImplementNote {
  id: string;
  comment?: string;
  status?: string;
  createdAt?: string;
}

export type ImplementOutcome =
  /** A note for these designs is still open (pending/acknowledged): nothing new was posted. */
  | { kind: "duplicate"; annotationId: string }
  /** The agent already resolved one; a new note needs `force`. */
  | { kind: "done"; annotationId: string }
  | { kind: "created"; annotationId: string }
  /** No agent reachable (notify returned nothing). */
  | { kind: "unreachable" };

const TAG = /^\[prototypie implement ([\w-]+)\]/;
const OPEN = new Set(["pending", "acknowledged"]);

/** The newest open and the newest resolved Implement note for any of `ids` (dismissed notes don't count). */
export function existingImplement(notes: ImplementNote[], ids: string[]) {
  const wanted = new Set(ids);
  const mine = notes
    .filter((n) => wanted.has((n.comment ?? "").match(TAG)?.[1] ?? ""))
    .sort((a, b) => (b.createdAt ?? "").localeCompare(a.createdAt ?? ""));
  return {
    open: mine.find((n) => OPEN.has(n.status ?? "pending")),
    resolved: mine.find((n) => n.status === "resolved"),
  };
}

type Recent = { ids: string[]; annotationId: string; at: number };
const g = globalThis as unknown as { __implementLock?: Promise<unknown>; __implementRecent?: Recent[] };
/** Notes this server just posted, in case the next scan doesn't list them yet (or Agentation's list fails). */
const RECENT_MS = 2 * 60_000;

/**
 * Decide and (if needed) post, one request at a time: the check and the post must not interleave, or four quick
 * clicks post four notes. `loadNotes` must read fresh (all states); undefined means "couldn't read".
 */
export function dedupedImplement(opts: {
  ids: string[];
  force?: boolean;
  loadNotes: () => Promise<ImplementNote[] | undefined>;
  notify: () => Promise<string | undefined>;
  now?: () => number;
}): Promise<ImplementOutcome> {
  const run = async (): Promise<ImplementOutcome> => {
    const now = opts.now?.() ?? Date.now();
    const notes = (await opts.loadNotes()) ?? [];
    const { open, resolved } = existingImplement(notes, opts.ids);
    if (open) return { kind: "duplicate", annotationId: open.id };
    const listed = new Set(notes.map((n) => n.id));
    const recent = (g.__implementRecent ?? []).filter((r) => now - r.at < RECENT_MS);
    g.__implementRecent = recent;
    const unlisted = recent.find((r) => !listed.has(r.annotationId) && r.ids.some((id) => opts.ids.includes(id)));
    if (unlisted) return { kind: "duplicate", annotationId: unlisted.annotationId };
    if (resolved && !opts.force) return { kind: "done", annotationId: resolved.id };
    const annotationId = await opts.notify();
    if (!annotationId) return { kind: "unreachable" };
    recent.push({ ids: opts.ids, annotationId, at: now });
    return { kind: "created", annotationId };
  };
  const next = (g.__implementLock ?? Promise.resolve()).then(run, run);
  g.__implementLock = next.catch(() => undefined);
  return next;
}
