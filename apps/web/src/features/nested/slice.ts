import type { StateCreator } from "zustand";
import type { Design, Space } from "@/lib/types";
import type { AppState } from "@/store";
import { NODE_H, NODE_W, ROOT_SPACE_ID, cancelJob } from "@/store/core";

// Owned by the "nested" feature. Everything here is persisted automatically.

/** Approximate rendered size of a portal node, used for layout. */
export const PORTAL_W = 300;
export const PORTAL_H = 330;

type Point = { x: number; y: number };

/** Provenance of a promoted copy. `spaceName` is captured so the badge survives deleting the source space. */
export interface PromotedRef {
  spaceId: string;
  designId: string;
  spaceName: string;
  at: number;
}

export interface NestedSlice {
  /** Portal position on the parent canvas, keyed by the nested space's id. */
  portals: Record<string, Point>;
  /** Promoted copy id -> where it came from. */
  promotedFrom: Record<string, PromotedRef>;
  /** Seed copy (inside a nested space) id -> the original design it was copied from. */
  seededFrom: Record<string, string>;

  /** Create a nested space under `parentSpaceId`, optionally seeded with a copy of a design. Returns the space id. */
  createExploration: (opts: { parentSpaceId: string; name?: string; seedDesignId?: string }) => {
    spaceId: string;
    seedId?: string;
  };
  /**
   * A project on the main canvas. One project per local folder: with a `localPath` that an existing project
   * already has, that project is returned instead (`created: false`).
   */
  createProject: (opts: { name: string; localPath?: string }) => { spaceId: string; created: boolean };
  movePortal: (spaceId: string, pos: Point) => void;
  /** Copy designs from their (nested) space into its parent space. Returns ids of the new copies. */
  promoteDesigns: (designIds: string[]) => string[];
  /** Remove a space, its designs and all sub-spaces. */
  deleteSpace: (spaceId: string) => void;
}

export const createNestedSlice: StateCreator<AppState, [], [], NestedSlice> = (set, get) => ({
  portals: {},
  promotedFrom: {},
  seededFrom: {},

  createExploration: ({ parentSpaceId, name, seedDesignId }) => {
    const s = get();
    const seed = seedDesignId ? s.designs[seedDesignId] : undefined;
    const siblings = Object.values(s.spaces).filter((x) => x.parentSpaceId === parentSpaceId).length;
    const spaceName = name ?? (seed ? seed.title : `Exploration ${siblings + 1}`);
    const spaceId = s.createSpace(spaceName, parentSpaceId);

    const start = seed
      ? { x: seed.position.x + NODE_W + 80, y: seed.position.y }
      : defaultSpot(s, parentSpaceId);
    const pos = findFreeSpot(occupied(get(), parentSpaceId), start, PORTAL_W, PORTAL_H, "right");

    let seedId: string | undefined;
    if (seed) {
      const { id: _id, createdAt: _c, ...rest } = seed;
      seedId = get().addDesign({
        ...rest,
        spaceId,
        parentId: undefined,
        starred: false,
        position: { x: 0, y: 0 },
      });
    }
    set((st) => ({
      portals: { ...st.portals, [spaceId]: pos },
      seededFrom: seedId && seed ? { ...st.seededFrom, [seedId]: seed.id } : st.seededFrom,
    }));
    return { spaceId, seedId };
  },

  createProject: ({ name, localPath }) => {
    const s = get();
    const key = localPath?.toLowerCase();
    const existing = key && Object.values(s.spaces).find((sp) => isProject(sp) && sp.localPath?.toLowerCase() === key);
    if (existing) return { spaceId: existing.id, created: false };
    const { spaceId } = get().createExploration({ parentSpaceId: ROOT_SPACE_ID, name });
    set((st) => ({ spaces: { ...st.spaces, [spaceId]: { ...st.spaces[spaceId], kind: "project", ...(localPath ? { localPath } : {}) } } }));
    return { spaceId, created: true };
  },

  movePortal: (spaceId, pos) => set((s) => ({ portals: { ...s.portals, [spaceId]: pos } })),

  promoteDesigns: (designIds) => {
    const created: string[] = [];
    for (const id of designIds) {
      const s = get();
      const src = s.designs[id];
      if (!src || src.status !== "ready") continue;
      const space = s.spaces[src.spaceId];
      const parentSpaceId = space?.parentSpaceId;
      if (!space || !canPromoteTo(parentSpaceId) || !s.spaces[parentSpaceId]) continue;
      if (findPromotedCopy(s, id)) continue;

      const portal = s.portals[space.id] ?? defaultSpot(s, parentSpaceId);
      const start = { x: portal.x, y: portal.y + PORTAL_H + 80 };
      const pos = findFreeSpot(occupied(s, parentSpaceId), start, NODE_W, NODE_H, "grid");

      const { id: _id, createdAt: _c, ...rest } = src;
      const copyId = s.addDesign({
        ...rest,
        spaceId: parentSpaceId,
        parentId: lineageParent(get(), src),
        starred: false,
        position: pos,
      });
      set((st) => ({
        promotedFrom: {
          ...st.promotedFrom,
          [copyId]: { spaceId: space.id, designId: id, spaceName: space.name, at: Date.now() },
        },
      }));
      created.push(copyId);
    }
    return created;
  },

  deleteSpace: (spaceId) => {
    const s = get();
    const target = s.spaces[spaceId];
    if (!target || !target.parentSpaceId) return; // never delete the root
    const doomed = new Set(descendantSpaceIds(s, spaceId));
    doomed.add(spaceId);

    for (const job of Object.values(s.jobs)) if (doomed.has(job.spaceId) && job.status === "running") cancelJob(job.id);

    set((st) => {
      const spaces = { ...st.spaces };
      const portals = { ...st.portals };
      for (const id of doomed) {
        delete spaces[id];
        delete portals[id];
      }
      const designs: Record<string, Design> = {};
      const removed = new Set<string>();
      for (const d of Object.values(st.designs)) {
        if (doomed.has(d.spaceId)) removed.add(d.id);
        else designs[d.id] = d;
      }
      const promotedFrom = Object.fromEntries(Object.entries(st.promotedFrom).filter(([k]) => !removed.has(k)));
      const seededFrom = Object.fromEntries(Object.entries(st.seededFrom).filter(([k]) => !removed.has(k)));
      return {
        spaces,
        portals,
        designs,
        promotedFrom,
        seededFrom,
        activeSpaceId: doomed.has(st.activeSpaceId) ? target.parentSpaceId! : st.activeSpaceId,
        selectedId: st.selectedId && removed.has(st.selectedId) ? null : st.selectedId,
      };
    });
  },
});

// ---------------------------------------------------------------------------------------------
// Pure helpers (also used by the UI)

export function isProject(sp: Space | undefined): boolean {
  return sp?.kind === "project";
}

/** Designs can be promoted into any space but the main canvas, which holds only projects. */
export function canPromoteTo(parentSpaceId: string | undefined): parentSpaceId is string {
  return !!parentSpaceId && parentSpaceId !== ROOT_SPACE_ID;
}

export function descendantSpaceIds(s: Pick<AppState, "spaces">, spaceId: string): string[] {
  const out: string[] = [];
  const stack = [spaceId];
  while (stack.length) {
    const cur = stack.pop()!;
    for (const sp of Object.values(s.spaces)) {
      if (sp.parentSpaceId === cur && !out.includes(sp.id)) {
        out.push(sp.id);
        stack.push(sp.id);
      }
    }
  }
  return out;
}

/** Root-first list of spaces from the root down to `spaceId`. */
export function spacePath(s: Pick<AppState, "spaces">, spaceId: string) {
  const path: Space[] = [];
  const seen = new Set<string>();
  let cur: Space | undefined = s.spaces[spaceId];
  while (cur && !seen.has(cur.id)) {
    seen.add(cur.id);
    path.unshift(cur);
    cur = cur.parentSpaceId ? s.spaces[cur.parentSpaceId] : undefined;
  }
  return path;
}

/** The live copy of `designId` in its parent space, if it was already promoted. */
export function findPromotedCopy(s: Pick<AppState, "promotedFrom" | "designs" | "spaces">, designId: string) {
  const src = s.designs[designId];
  const parent = src ? s.spaces[src.spaceId]?.parentSpaceId : undefined;
  if (!parent) return undefined;
  for (const [copyId, ref] of Object.entries(s.promotedFrom)) {
    if (ref.designId === designId && s.designs[copyId]?.spaceId === parent) return copyId;
  }
  return undefined;
}

/**
 * For a design being promoted, find the closest relative already on the parent canvas so core draws an
 * edge: walk up its ancestry inside the nested space and return the first ancestor's promoted copy or
 * the original a seed was copied from.
 */
function lineageParent(s: AppState, src: Design): string | undefined {
  const parentSpaceId = s.spaces[src.spaceId]?.parentSpaceId;
  const seen = new Set<string>();
  let cur: Design | undefined = src.parentId ? s.designs[src.parentId] : undefined;
  // The design itself may be the seed.
  const own = s.seededFrom[src.id];
  if (own && s.designs[own]?.spaceId === parentSpaceId) return own;
  while (cur && cur.spaceId === src.spaceId && !seen.has(cur.id)) {
    seen.add(cur.id);
    const copy = findPromotedCopy(s, cur.id);
    if (copy) return copy;
    const orig = s.seededFrom[cur.id];
    if (orig && s.designs[orig]?.spaceId === parentSpaceId) return orig;
    cur = cur.parentId ? s.designs[cur.parentId] : undefined;
  }
  return undefined;
}

type Rect = { x: number; y: number; w: number; h: number };

function occupied(s: AppState, spaceId: string): Rect[] {
  const rects: Rect[] = Object.values(s.designs)
    .filter((d) => d.spaceId === spaceId)
    .map((d) => ({ ...d.position, w: NODE_W, h: NODE_H }));
  for (const sp of Object.values(s.spaces)) {
    const p = sp.parentSpaceId === spaceId ? s.portals[sp.id] : undefined;
    if (p) rects.push({ ...p, w: PORTAL_W, h: PORTAL_H });
  }
  return rects;
}

/** Right of everything on the canvas, aligned with the top-most node. */
function defaultSpot(s: AppState, spaceId: string): Point {
  const rects = occupied(s, spaceId);
  if (!rects.length) return { x: 0, y: 0 };
  return { x: Math.max(...rects.map((r) => r.x + r.w)) + 120, y: Math.min(...rects.map((r) => r.y)) };
}

/** First spot near `start` where a w*h box doesn't overlap anything (with a gap). */
function findFreeSpot(rects: Rect[], start: Point, w: number, h: number, mode: "right" | "grid"): Point {
  const gap = 40;
  const hits = (p: Point) =>
    rects.some((r) => p.x < r.x + r.w + gap && p.x + w + gap > r.x && p.y < r.y + r.h + gap && p.y + h + gap > r.y);
  if (mode === "right") {
    for (let i = 0; i < 40; i++) {
      const p = { x: start.x + i * (w + gap), y: start.y };
      if (!hits(p)) return p;
    }
  } else {
    for (let row = 0; row < 20; row++) {
      for (let col = 0; col < 4; col++) {
        const p = { x: start.x + col * (w + gap), y: start.y + row * (h + gap) };
        if (!hits(p)) return p;
      }
    }
  }
  return start;
}
