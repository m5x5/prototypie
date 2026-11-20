import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import type { Design, Space, Variation, VariationRequest } from "@/lib/types";
import { applyChanges, readState } from "./state-store";

/**
 * Work queue for the "claude" agent: instead of calling the LLM, a job writes one item per direction to
 * data/agent-queue/ and waits until a Claude Code session watching the queue POSTs the finished design
 * (see scripts/agent-queue.md). Resolvers live on globalThis so dev hot reloads keep them.
 */

export interface QueueItem {
  jobId: string;
  index: number;
  instruction: string;
  focus?: string;
  scope?: VariationRequest["scope"];
  annotations?: VariationRequest["annotations"];
  direction: { title: string; brief: string };
  baseTitle?: string;
  baseHtml?: string;
  /** Combine these designs into one new design (multi-selection "Combine"); replaces baseHtml. */
  bases?: VariationRequest["bases"];
  /** @-mentioned Solid pod data (selected fields only, with source, access and stand). Use these values; don't invent others. */
  podContext?: VariationRequest["podContext"];
  /** #-mentioned cards: existing components to reuse exactly (markup, styles, copy, sizes), not redesign. */
  components?: VariationRequest["components"];
  /** The space's design guidelines (its marked card, or the nearest ancestor space's), as text. Follow them strictly. */
  guidelines?: { cardId: string; title: string; text: string };
  /**
   * "import": recreate `element` as it exists today in the app at `importSource` (read the source read-only:
   * code, tokens, real copy, real sizes and viewport) as a component-scope card. Don't invent or improve.
   * "edit": in-place AI edit of the card `targetDesignId` (its html is `baseHtml`): return the SAME document with
   * only the requested change; the delivery overwrites that card (the old html is kept as a version to undo to).
   */
  mode?: "import" | "edit";
  /** mode "edit": the card whose html the delivery replaces. */
  targetDesignId?: string;
  element?: string;
  importSource?: string;
  /** Absolute path of a screenshot of the element the designer attached, if any. */
  importScreenshot?: string;
  createdAt: string;
  status: "pending" | "done" | "cancelled";
  /** Agentation annotation that mirrors this item, if the agentation server was reachable. */
  annotationId?: string;
  /** Placeholder card id, remembered once known so a delivery can still find its card after the run is gone. */
  placeholderId?: string;
  /** Where on the canvas this item lands. Filled in when listed (from the run in shared state), not stored. */
  context?: QueueContext;
}

export interface QueueContext {
  /** The exploration (space) the result appears in, with its path from the main canvas. */
  spaceId: string;
  spaceName: string;
  spacePath: string[];
  /** The design being varied, if any. */
  baseDesignId?: string;
  /** The placeholder card this item's result fills in. */
  placeholderId?: string;
  /** Other cards already in that space (for orientation, or as nearId/parentId with the /api/agent skills). */
  cards: { id: string; title: string }[];
  /** The design this exploration was spawned from ("Explore"): its copy in the space, and the original it came from. */
  seedDesign?: { id: string; title: string; originalId: string };
}

const DIR = path.join(process.cwd(), "data", "agent-queue");
const AGENTATION = "http://localhost:4747";
const APP_URL = "http://localhost:3123/";

const g = globalThis as unknown as { __prototypieQueue?: Map<string, (v: Variation) => void> };
const waiting = (g.__prototypieQueue ??= new Map<string, (v: Variation) => void>());
/** Per waiting item: give up on it (its placeholder card was deleted). Separate map so hot reloads keep old entries valid. */
const gc = globalThis as unknown as { __prototypieQueueCancel?: Map<string, (reason: string) => void> };
const cancellers = (gc.__prototypieQueueCancel ??= new Map<string, (reason: string) => void>());

const key = (jobId: string, index: number) => `${jobId}-${index}`;
const fileOf = (jobId: string, index: number) => path.join(DIR, `${key(jobId, index)}.json`);

async function save(item: QueueItem) {
  await mkdir(DIR, { recursive: true });
  await writeFile(fileOf(item.jobId, item.index), JSON.stringify(item, null, 2));
}

async function load(jobId: string, index: number): Promise<QueueItem | undefined> {
  try {
    return JSON.parse(await readFile(fileOf(jobId, index), "utf8")) as QueueItem;
  } catch {
    return undefined;
  }
}

async function setStatus(jobId: string, index: number, status: QueueItem["status"]) {
  const item = await load(jobId, index);
  if (!item || item.status !== "pending") return;
  await save({ ...item, status });
  if (item.annotationId) {
    await fetch(`${AGENTATION}/annotations/${item.annotationId}`, {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ status: status === "done" ? "resolved" : "dismissed", resolvedBy: "agent" }),
    }).catch(() => {});
  }
}

/**
 * Post a note for the coding agent to the agentation server (best effort), so a Claude session watching it
 * picks the work up. Returns the annotation id, or undefined when the server isn't reachable.
 */
export async function notifyAgent(note: { comment: string; element: string; elementPath: string }): Promise<string | undefined> {
  // A wedged agentation server must not hang the caller (the Implement button, or enqueueing a job).
  const signal = () => AbortSignal.timeout(3000);
  try {
    const list = await fetch(`${AGENTATION}/sessions`, { signal: signal() });
    if (!list.ok) return undefined;
    const sessions = (await list.json()) as { id: string; url: string; status: string }[];
    let session = sessions.find((s) => s.url === APP_URL && s.status === "active");
    if (!session) {
      const created = await fetch(`${AGENTATION}/sessions`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ url: APP_URL }),
        signal: signal(),
      });
      if (!created.ok) return undefined;
      session = (await created.json()) as { id: string; url: string; status: string };
    }
    const res = await fetch(`${AGENTATION}/sessions/${session.id}/annotations`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ ...note, x: 0, y: 0, timestamp: Date.now(), url: APP_URL, kind: "feedback" }),
      signal: signal(),
    });
    if (!res.ok) return undefined;
    return ((await res.json()) as { id?: string }).id;
  } catch {
    return undefined;
  }
}

/** Mirror the item as an Agentation annotation so a watcher on the agentation server sees it. */
function announce(item: QueueItem) {
  return notifyAgent({
    comment:
      `[prototypie job ${item.jobId}#${item.index}] ${item.instruction || item.direction.brief}` +
      (item.bases?.length ? ` · COMBINE ${item.bases.length} designs (item.bases): ${item.bases.map((b) => b.title).join(" + ")}` : "") +
      (item.podContext?.length ? ` · POD DATA (item.podContext): ${item.podContext.map((e) => e.label).join(", ")}` : "") +
      (item.components?.length ? ` · REUSE COMPONENTS AS-IS (item.components): ${item.components.map((c) => c.title).join(", ")}` : "") +
      (item.guidelines ? ` · FOLLOW DESIGN GUIDELINES (item.guidelines): ${item.guidelines.title}` : "") +
      (item.mode === "import"
        ? ` · IMPORT (item.mode): recreate "${item.element}" faithfully from ${item.importSource}${item.importScreenshot ? ` + screenshot ${item.importScreenshot}` : ""}; read the source read-only, invent nothing`
        : "") +
      (item.mode === "edit"
        ? ` · EDIT IN PLACE (item.mode) card ${item.targetDesignId}: return item.baseHtml with ONLY this change, everything else byte-identical; the POST overwrites that card`
        : "") +
      ` · context + skills: GET /api/agent-queue (item.context), POST /api/agent/cards, /api/agent/explorations (scripts/agent-queue.md)` +
      ` · rules: only the discussed component, one variant, no explanatory text, short space-scoped title (no project name, no "Komponente")`,
    element: `agent-queue item ${key(item.jobId, item.index)}`,
    elementPath: `data/agent-queue/${key(item.jobId, item.index)}.json`,
  });
}

/**
 * Queue one direction for the Claude watcher and wait for its result.
 * Rejects when `signal` aborts (job cancelled); the item is then marked cancelled.
 */
export async function enqueueAndWait(
  item: Omit<QueueItem, "createdAt" | "status" | "annotationId">,
  signal: AbortSignal,
): Promise<Variation> {
  const full: QueueItem = { ...item, createdAt: new Date().toISOString(), status: "pending" };
  await save(full);
  const result = new Promise<Variation>((resolve, reject) => {
    const k = key(item.jobId, item.index);
    const stop = (reason: string) => {
      signal.removeEventListener("abort", onAbort);
      waiting.delete(k);
      cancellers.delete(k);
      void setStatus(item.jobId, item.index, "cancelled");
      reject(new Error(reason));
    };
    const onAbort = () => stop("Cancelled");
    if (signal.aborted) return onAbort();
    signal.addEventListener("abort", onAbort, { once: true });
    cancellers.set(k, stop);
    waiting.set(k, (v) => {
      signal.removeEventListener("abort", onAbort);
      waiting.delete(k);
      cancellers.delete(k);
      resolve(v);
    });
  });
  const annotationId = await announce(full);
  if (annotationId) {
    const now = await load(item.jobId, item.index);
    if (now) await save({ ...now, annotationId });
  }
  return result;
}

const SHOT = /^data:image\/(png|jpe?g|webp|gif);base64,([A-Za-z0-9+/=]+)$/;
/** Largest screenshot accepted, in bytes after decoding. */
export const MAX_SCREENSHOT = 8 * 1024 * 1024;

/** Save an import's screenshot (data: URL) next to the queue items; returns its absolute path, or undefined if it isn't a valid image. */
export async function saveScreenshot(jobId: string, dataUrl: string): Promise<string | undefined> {
  const m = SHOT.exec(dataUrl);
  if (!m) return undefined;
  const bytes = Buffer.from(m[2], "base64");
  if (!bytes.length || bytes.length > MAX_SCREENSHOT) return undefined;
  await mkdir(DIR, { recursive: true });
  const file = path.join(DIR, `${jobId}-screenshot.${m[1] === "jpeg" ? "jpg" : m[1]}`);
  await writeFile(file, bytes);
  return file;
}

const viewportMeta = (html: string) => html.match(/<meta\b[^>]*\bname\s*=\s*["']?viewport["']?[^>]*>/i)?.[0];

/** A fixed layout width declared in the html (<meta name="viewport" content="width=1240">), if it's in range. */
export function inferViewport(html: string): number | undefined {
  const n = Number(viewportMeta(html)?.match(/\bwidth\s*=\s*(\d+)/i)?.[1]);
  return n >= 200 && n <= 4000 ? n : undefined;
}

/** A fixed screen height declared in the html (<meta name="viewport" content="width=640,height=960">), if it's in range. */
export function inferViewportHeight(html: string): number | undefined {
  const n = Number(viewportMeta(html)?.match(/\bheight\s*=\s*(\d+)/i)?.[1]);
  return n >= 200 && n <= 4000 ? n : undefined;
}

/**
 * Deliver a finished design. Returns false if nobody is waiting for it (unknown, done, cancelled or expired),
 * and "gone" if its placeholder card was deleted meanwhile: the item is then cancelled and nothing is placed.
 */
export async function submitResult(jobId: string, index: number, v: Variation): Promise<boolean | "gone"> {
  const k = key(jobId, index);
  const resolve = waiting.get(k);
  if (!resolve) return false;
  const { records } = await readState().catch(() => ({ records: {} as Records }));
  const id = await placeholderOf(jobId, index, records);
  if (id && records[`designs/${id}`]?.deleted) {
    cancellers.get(k)?.("Card deleted");
    return "gone";
  }
  resolve(v);
  await setStatus(jobId, index, "done");
  await fillPlaceholder(jobId, index, v);
  return true;
}

/**
 * Write a delivered result straight into its placeholder card. Normally the browser tab that follows the job
 * stream does this, but a backgrounded or closed tab can leave the card on "generating" for minutes. Best
 * effort and idempotent: only touches a placeholder that is still generating, and the follower's later update
 * writes the same fields.
 */
async function fillPlaceholder(jobId: string, index: number, v: Variation) {
  try {
    const { records } = await readState();
    const id = await placeholderOf(jobId, index, records);
    const rec = id ? records[`designs/${id}`] : undefined;
    const current = rec && !rec.deleted ? (rec.value as Design | undefined) : undefined;
    // Fill a card that is still generating, or one the follower marked as failed (e.g. "network error" after
    // its stream broke) while the server job kept running. Never overwrite a card that already has content.
    const fillable = current && (current.status === "generating" || (current.status === "error" && !current.html));
    if (!id || !current || !fillable) return;
    await applyChanges(
      [
        {
          path: `designs/${id}`,
          value: {
            ...current,
            title: v.title,
            rationale: v.rationale,
            html: v.html,
            ...(v.viewport ? { viewport: v.viewport } : {}),
            ...(v.viewportHeight ? { viewportHeight: v.viewportHeight } : {}),
            status: "ready",
            error: undefined,
          },
        },
      ],
      { clientId: "server:agent-queue", reason: `queue delivery ${key(jobId, index)}` },
    );
  } catch {
    // The follower tab still applies the result; this is only a shortcut.
  }
}

type Records = Awaited<ReturnType<typeof readState>>["records"];

/** The placeholder card an item fills: from the shared run that drives the job, else the id remembered on disk. */
async function placeholderOf(jobId: string, index: number, records: Records): Promise<string | undefined> {
  const run = Object.entries(records)
    .filter(([p, r]) => p.startsWith("runs/") && !r.deleted && r.value !== undefined)
    .map(([, r]) => r.value as RunRecord)
    .find((r) => r.serverJobId === jobId);
  return run?.designIds[index] ?? (await load(jobId, index))?.placeholderId;
}

/**
 * Cancel the waiting items whose placeholder card is among `designIds` (deleted in the app), so no agent
 * keeps working on a result that has nowhere to land. The job gets an error for that direction.
 */
export async function cancelForDeletedDesigns(designIds: string[]) {
  if (!cancellers.size) return;
  const gone = new Set(designIds);
  const { records } = await readState();
  for (const [k, stop] of [...cancellers]) {
    const cut = k.lastIndexOf("-");
    const id = await placeholderOf(k.slice(0, cut), Number(k.slice(cut + 1)), records);
    if (id && gone.has(id)) stop("Card deleted");
  }
}

/** Pending items, oldest first. Items whose job no longer waits (e.g. dev server restarted) are left out. */
export async function listPending(): Promise<QueueItem[]> {
  let names: string[] = [];
  try {
    names = (await readdir(DIR)).filter((n) => n.endsWith(".json"));
  } catch {
    return [];
  }
  const items = await Promise.all(
    names.map(async (n) => {
      try {
        return JSON.parse(await readFile(path.join(DIR, n), "utf8")) as QueueItem;
      } catch {
        return undefined;
      }
    }),
  );
  const pending = items
    .filter((i): i is QueueItem => !!i && i.status === "pending" && waiting.has(key(i.jobId, i.index)))
    .sort((a, b) => a.createdAt.localeCompare(b.createdAt));
  const withCtx = await withContext(pending);
  // Remember each placeholder id on disk: if the follower tab later drops the run (e.g. its stream broke on a
  // dev reload), a delivery can still fill the right card.
  await Promise.all(
    withCtx.map(async (it) => {
      const id = it.context?.placeholderId;
      if (!id || it.placeholderId === id) return;
      const onDisk = await load(it.jobId, it.index);
      if (onDisk && !onDisk.placeholderId) await save({ ...onDisk, placeholderId: id }).catch(() => {});
    }),
  );
  return withCtx;
}

type RunRecord = { serverJobId?: string; spaceId: string; parentId?: string; designIds: string[] };

/** Attach each item's canvas context, found via the shared run whose serverJobId is the item's job. */
export async function withContext(items: QueueItem[]): Promise<QueueItem[]> {
  if (!items.length) return items;
  let records: Awaited<ReturnType<typeof readState>>["records"];
  try {
    records = (await readState()).records;
  } catch {
    return items;
  }
  const live = <T>(prefix: string) =>
    Object.entries(records)
      .filter(([p, r]) => p.startsWith(prefix) && !r.deleted && r.value !== undefined)
      .map(([, r]) => r.value as T);
  const runs = new Map(live<RunRecord>("runs/").filter((r) => r.serverJobId).map((r) => [r.serverJobId!, r]));
  const spaces = new Map(live<Space>("spaces/").map((sp) => [sp.id, sp]));
  const designs = live<Design>("designs/");
  // seededFrom/<copy id> = original design id (the exploration's starting card).
  const seededFrom = new Map(
    Object.entries(records)
      .filter(([p, r]) => p.startsWith("seededFrom/") && !r.deleted && typeof r.value === "string")
      .map(([p, r]) => [p.slice("seededFrom/".length), r.value as string]),
  );
  const nameOf = (id: string) => (id === "root" ? "Main canvas" : (spaces.get(id)?.name ?? id));
  const pathOf = (id: string) => {
    const out: string[] = [];
    for (let cur: string | undefined = id, n = 0; cur && n < 20; cur = spaces.get(cur)?.parentSpaceId, n++) out.unshift(nameOf(cur));
    if (out[0] !== "Main canvas") out.unshift("Main canvas");
    return out;
  };
  return items.map((item) => {
    const run = runs.get(item.jobId);
    // An in-place edit has no run: its context is the card it overwrites.
    const target = !run && item.targetDesignId ? designs.find((d) => d.id === item.targetDesignId) : undefined;
    if (target) {
      return {
        ...item,
        context: {
          spaceId: target.spaceId,
          spaceName: nameOf(target.spaceId),
          spacePath: pathOf(target.spaceId),
          baseDesignId: target.id,
          placeholderId: target.id,
          cards: designs.filter((d) => d.spaceId === target.spaceId).map((d) => ({ id: d.id, title: d.title })),
        },
      };
    }
    if (!run) return item;
    const inSpace = designs.filter((d) => d.spaceId === run.spaceId);
    const seed = inSpace.find((d) => seededFrom.has(d.id));
    return {
      ...item,
      context: {
        spaceId: run.spaceId,
        spaceName: nameOf(run.spaceId),
        spacePath: pathOf(run.spaceId),
        baseDesignId: run.parentId,
        placeholderId: run.designIds[item.index],
        cards: inSpace.map((d) => ({ id: d.id, title: d.title })),
        ...(seed ? { seedDesign: { id: seed.id, title: seed.title, originalId: seededFrom.get(seed.id)! } } : {}),
      },
    };
  });
}
