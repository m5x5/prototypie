"use client";
import { openNotesFor } from "@/features/agentation/slice";
import { create } from "zustand";
import { toast } from "sonner";
import type { Design } from "@/lib/types";
import { useApp, type AppState } from "@/store";
import { NODE_H, NODE_W, type GenerateOptions } from "@/store/core";
import type { LaneMeta } from "./slice";

/* ------------------------------------------------------------------ transient UI state */

interface SpokesUI {
  clayOpen: boolean;
  setClayOpen: (open: boolean) => void;
  /** Hub page whose "break into spokes" dialog is open. */
  spokesFor: string | null;
  setSpokesFor: (id: string | null) => void;
  reviewOpen: boolean;
  /** Design to show first when the review opens. */
  reviewStartId: string | null;
  openReview: (startId?: string | null) => void;
  closeReview: () => void;
}

export const useSpokesUI = create<SpokesUI>()((set) => ({
  clayOpen: false,
  setClayOpen: (clayOpen) => set({ clayOpen }),
  spokesFor: null,
  setSpokesFor: (spokesFor) => set({ spokesFor }),
  reviewOpen: false,
  reviewStartId: null,
  openReview: (startId = null) => set({ reviewOpen: true, reviewStartId: startId }),
  closeReview: () => set({ reviewOpen: false, reviewStartId: null }),
}));

/* ------------------------------------------------------------------ keyboard */

/** True when a key event comes from somewhere the user is typing. */
export function isTypingTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  if (tag === "TEXTAREA" || tag === "SELECT") return true;
  if (tag === "INPUT") {
    const type = (target as HTMLInputElement).type;
    return !["checkbox", "radio", "button", "submit", "range"].includes(type);
  }
  return target.getAttribute("role") === "textbox";
}

/* ------------------------------------------------------------------ lanes */

type LaneInfo = Omit<LaneMeta, "jobId" | "startedAt" | "count">;

/**
 * Launch a core `generate` call and register it as a lane. Doesn't wait for the stream:
 * results land on the canvas (and in the review queue) as each one finishes.
 */
export function launchLane(opts: GenerateOptions, info: LaneInfo): Promise<string[]> {
  const before = new Set(Object.keys(useApp.getState().jobs));
  const promise = useApp.getState().generate(opts);
  // `generate` registers its job synchronously before its first await.
  const jobId = Object.keys(useApp.getState().jobs).find((k) => !before.has(k));
  if (jobId) {
    useApp.getState().registerLane({ ...info, jobId, count: opts.request.count, startedAt: Date.now() });
  }
  return promise.then((ids) => {
    const s = useApp.getState();
    if (info.focus && ids.length) s.setSpokeFocus(ids, info.focus);
    const job = jobId ? s.jobs[jobId] : undefined;
    if (job?.status === "error" && /abort/i.test(job.error ?? "")) {
      toast(`${info.name} cancelled`);
    } else if (job?.status === "error") {
      toast.error(`${info.name} failed`, { description: job.error ?? "Generation failed" });
    } else {
      const failed = ids.filter((id) => s.designs[id]?.status === "error").length;
      if (failed && failed === ids.length) toast.error(`${info.name}: every direction failed`);
    }
    return ids;
  });
}

export function laneProgress(state: AppState, designIds: string[], requested: number) {
  const ds = designIds.map((id) => state.designs[id]).filter(Boolean);
  return {
    total: designIds.length || requested,
    ready: ds.filter((d) => d.status === "ready").length,
    failed: ds.filter((d) => d.status === "error").length,
  };
}

/* ------------------------------------------------------------------ relationships */

/** Designs produced by the same round (core job) as `id`, in generation order. Includes `id`. */
export function siblingsOf(state: AppState, id: string): Design[] {
  const d = state.designs[id];
  if (!d) return [];
  const job = Object.values(state.jobs).find((j) => j.designIds.includes(id));
  if (job) return job.designIds.map((x) => state.designs[x]).filter(Boolean);
  // Jobs are transient: after a reload, fall back to designs created together from the same prompt.
  return Object.values(state.designs)
    .filter(
      (o) =>
        o.spaceId === d.spaceId &&
        o.parentId === d.parentId &&
        o.kind === d.kind &&
        (o.instruction ?? "") === (d.instruction ?? "") &&
        Math.abs(o.createdAt - d.createdAt) < 10_000,
    )
    .sort((a, b) => a.position.x - b.position.x || a.createdAt - b.createdAt);
}

/** Lane metadata for the round that produced `id`, if this feature launched it. */
export function laneOf(state: AppState, id: string): LaneMeta | undefined {
  const job = Object.values(state.jobs).find((j) => j.designIds.includes(id));
  return job ? state.lanes[job.id] : undefined;
}

/** Component this spoke explores (walks up through earlier rounds of the same spoke). */
export function focusOf(state: AppState, id: string): string | undefined {
  let cur: Design | undefined = state.designs[id];
  for (let guard = 0; cur && guard < 50; guard++) {
    const f = state.spokeFocus[cur.id] ?? laneOf(state, cur.id)?.focus;
    if (f) return f;
    if (cur.kind !== "spoke") return undefined;
    cur = cur.parentId ? state.designs[cur.parentId] : undefined;
  }
  return undefined;
}

/**
 * The "hub" of a design. For a spoke: the design it was broken out of, i.e. the nearest non-spoke ancestor
 * (any kind can be broken into spokes, not only pages). For anything else: the nearest page ancestor.
 */
export function hubOf(state: AppState, id: string): Design | undefined {
  const fromSpoke = state.designs[id]?.kind === "spoke";
  let cur = state.designs[id]?.parentId ? state.designs[state.designs[id].parentId!] : undefined;
  for (let guard = 0; cur && guard < 50; guard++) {
    if (cur.kind === "page" || (fromSpoke && cur.kind !== "spoke")) return cur;
    cur = cur.parentId ? state.designs[cur.parentId] : undefined;
  }
  return undefined;
}

type Annotation = { element?: string; comment: string };

/**
 * Agentation notes pinned on a design, if the agentation feature stores any. Read defensively (its slice may
 * not exist yet): accepts `annotations[designId]` as an array of `{ element?, comment }`.
 */
/** Open Agentation notes pinned on a design, for VariationRequest.annotations. */
export function annotationsFor(state: AppState, id: string): Annotation[] | undefined {
  const out = openNotesFor(state, id);
  return out.length ? out : undefined;
}

/* ------------------------------------------------------------------ layout */

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}
const GAP = 40;
const ROW_GAP = 120;

export function rowWidth(count: number) {
  return count * NODE_W + (count - 1) * GAP;
}

function occupied(state: AppState, spaceId: string): Rect[] {
  return Object.values(state.designs)
    .filter((d) => d.spaceId === spaceId)
    .map((d) => ({ x: d.position.x, y: d.position.y, w: NODE_W, h: NODE_H }));
}

function hits(r: Rect, list: Rect[]) {
  const pad = 30;
  return list.some(
    (o) => r.x < o.x + o.w + pad && r.x + r.w + pad > o.x && r.y < o.y + o.h + pad && r.y + r.h + pad > o.y,
  );
}

function firstFree(candidate: (k: number) => { x: number; y: number }, w: number, taken: Rect[]) {
  for (let k = 0; k < 400; k++) {
    const p = candidate(k);
    const r = { ...p, w, h: NODE_H };
    if (!hits(r, taken)) {
      taken.push(r);
      return p;
    }
  }
  const p = candidate(0);
  taken.push({ ...p, w, h: NODE_H });
  return p;
}

/**
 * Origins for several spoke rows around a hub: alternating right/left columns of rows, skipping anything
 * already on the canvas so rows never overlap each other or existing nodes.
 */
export function spokeRowOrigins(state: AppState, hub: Design, rows: number, count: number) {
  const taken = occupied(state, hub.spaceId);
  const w = rowWidth(count);
  return Array.from({ length: rows }, () =>
    firstFree(
      (k) => {
        const right = k % 2 === 0;
        const row = Math.floor(k / 2);
        return {
          x: right ? hub.position.x + NODE_W + 220 : hub.position.x - 220 - w,
          y: hub.position.y + row * (NODE_H + ROW_GAP),
        };
      },
      w,
      taken,
    ),
  );
}

/** Origins for follow-up rounds: below each parent, shifted sideways when that spot is taken. */
export function belowOrigins(state: AppState, parents: Design[], count: number) {
  const w = rowWidth(count);
  const bySpace = new Map<string, Rect[]>();
  return parents.map((p) => {
    if (!bySpace.has(p.spaceId)) bySpace.set(p.spaceId, occupied(state, p.spaceId));
    const taken = bySpace.get(p.spaceId)!;
    return firstFree(
      (k) => {
        const row = Math.floor(k / 5);
        const shift = [0, 1, -1, 2, -2][k % 5] * (w + 80);
        return {
          x: p.position.x + NODE_W / 2 - w / 2 + shift,
          y: p.position.y + NODE_H + ROW_GAP + row * (NODE_H + ROW_GAP),
        };
      },
      w,
      taken,
    );
  });
}

/* ------------------------------------------------------------------ voice-friendly selection */

const NUMBER_WORDS: Record<string, number> = {
  one: 1, first: 1,
  two: 2, second: 2,
  three: 3, third: 3,
  four: 4, fourth: 4,
  five: 5, fifth: 5,
  six: 6, sixth: 6,
  seven: 7, seventh: 7,
  eight: 8, eighth: 8,
  nine: 9, ninth: 9,
  ten: 10, tenth: 10,
};
const NUM = `(?:\\d{1,2}|${Object.keys(NUMBER_WORDS).join("|")})`;
const SEP = `(?:\\s*(?:,|&|\\+|and|plus|or)\\s*|\\s+)`;
// A leading selection clause like "okay, three and six," or "#2, #5 -".
const LEAD_RE = new RegExp(
  `^\\s*(?:(?:ok(?:ay)?|so|alright|right|um|uh)[\\s,]+)*(?:(?:numbers?|nos?\\.?|#)\\s*)?${NUM}(?:${SEP}(?:(?:numbers?|#)\\s*)?${NUM})*\\b[\\s,.:;-]*`,
  "i",
);
const ROUND_RE = /^(?:(?:let'?s|please|and)\s+)*(?:do|try|run|give me)?\s*(?:another|one more|a new|new|a fresh)\s+round(?:\s+(?:on|of|for)\s+(?:these|those|them))?(?:\s+(?:with|where|that)\b)?[\s,.:;-]*/i;

/**
 * Picks sibling numbers out of feedback, voice style: "3 and 6, more playful" / "okay three and six, do another
 * round". Only the leading clause counts, so "make it 2 columns" doesn't select #2. Returns 1-based numbers <= max.
 */
export function parseSelection(text: string, max: number): number[] {
  const m = text.match(LEAD_RE);
  if (!m) return [];
  const out: number[] = [];
  for (const tok of m[0].toLowerCase().match(new RegExp(`\\b${NUM}\\b`, "gi")) ?? []) {
    const n = /^\d+$/.test(tok) ? Number(tok) : NUMBER_WORDS[tok];
    if (n >= 1 && n <= max && !out.includes(n)) out.push(n);
  }
  return out;
}

/** Feedback with the leading selection clause and "do another round" filler removed. */
export function stripSelection(text: string) {
  let rest = text.replace(LEAD_RE, "");
  if (rest === text && !LEAD_RE.test(text)) rest = text;
  rest = rest.replace(ROUND_RE, "").trim();
  return rest.replace(/^[,.:;-]+\s*/, "");
}
