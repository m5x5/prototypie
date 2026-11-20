import { nanoid } from "nanoid";
import type { StateCreator } from "zustand";
import type { AgentChoice, Design, DesignKind, Space, VariationRequest } from "@/lib/types";
import { cancelServerJob, resumeVariations, serverJobExists, streamVariations } from "@/lib/generate";
import type { VariationEvent } from "@/lib/types";
import type { AppState } from "./index";

export const ROOT_SPACE_ID = "root";
export const NODE_W = 380;
export const NODE_H = 420; // approximate rendered height of a design node, used for layout
/** Horizontal gap between sibling variations in a round. */
export const SIBLING_GAP = 160;

export interface Job {
  id: string;
  label: string;
  spaceId: string;
  status: "running" | "done" | "error";
  designIds: string[];
  startedAt: number;
  error?: string;
}

export interface GenerateOptions {
  spaceId: string;
  parentId?: string;
  kind?: DesignKind;
  request: VariationRequest;
  /** Top-left where the first new node goes. Defaults to below the parent / right of existing nodes. */
  origin?: { x: number; y: number };
  label?: string;
}

/** A generation in flight, persisted so a page reload can reattach to the server job and finish it. */
export interface Run {
  id: string; // same as the local Job id
  serverJobId?: string;
  spaceId: string;
  parentId?: string;
  kind: DesignKind;
  instruction: string;
  origin: { x: number; y: number };
  label: string;
  request: Omit<VariationRequest, "baseHtml">;
  designIds: string[]; // by direction index
}

export interface CoreSlice {
  spaces: Record<string, Space>;
  designs: Record<string, Design>;
  jobs: Record<string, Job>;
  runs: Record<string, Run>;
  activeSpaceId: string;
  selectedId: string | null;

  createSpace: (name: string, parentSpaceId?: string) => string;
  renameSpace: (id: string, name: string) => void;
  setActiveSpace: (id: string) => void;
  addDesign: (d: Omit<Design, "id" | "createdAt"> & { id?: string }) => string;
  updateDesign: (id: string, patch: Partial<Design>) => void;
  removeDesign: (id: string) => void;
  /** Make a card its space's design guidelines (unmarks any other in that space), or unmark it. */
  setGuidelines: (id: string, on: boolean) => void;
  select: (id: string | null) => void;
  /** Ask the agent for variations. Resolves with the created design ids when the stream finishes. */
  generate: (opts: GenerateOptions) => Promise<string[]>;
  /** Reattach to server jobs that were still running when the page was reloaded (only runs `only` accepts). */
  resumeRuns: (only?: (run: Run) => boolean) => void;
  /**
   * For runs this browser won't drive (`only` accepts them), ask the server whether their job still
   * exists; drop the ones it has lost and mark their placeholders "Interrupted". Never follows a job.
   */
  reapLostRuns: (only: (run: Run) => boolean) => Promise<void>;
  /** Re-run one failed/interrupted design's direction in place. */
  retryDesign: (id: string) => Promise<void>;
  /** Prompt box drafts (key: design id, or "new") and recent prompts, persisted. */
  drafts: Record<string, string>;
  promptHistory: string[];
  setDraft: (key: string, text: string) => void;
  /** Drops a draft (after sending or cancelling). */
  clearDraft: (key: string) => void;
  rememberPrompt: (text: string) => void;
  /** Which agent the inspector sends prompts to (persisted). */
  agentChoice: AgentChoice;
  setAgentChoice: (a: AgentChoice) => void;
}

const controllers = new Map<string, AbortController>();

export const createCoreSlice: StateCreator<AppState, [], [], CoreSlice> = (set, get) => ({
  spaces: { [ROOT_SPACE_ID]: { id: ROOT_SPACE_ID, name: "Main canvas", createdAt: Date.now() } },
  designs: {},
  jobs: {},
  runs: {},
  drafts: {},
  promptHistory: [],
  setDraft: (key, text) => set((s) => ({ drafts: { ...s.drafts, [key]: text } })),
  clearDraft: (key) =>
    set((s) => {
      if (!(key in s.drafts)) return {};
      const drafts = { ...s.drafts };
      delete drafts[key];
      return { drafts };
    }),
  agentChoice: "claude",
  setAgentChoice: (agentChoice) => set({ agentChoice }),
  rememberPrompt: (text) =>
    set((s) => {
      const t = text.trim();
      if (!t) return {};
      return { promptHistory: [t, ...s.promptHistory.filter((p) => p !== t)].slice(0, 30) };
    }),
  activeSpaceId: ROOT_SPACE_ID,
  selectedId: null,

  createSpace: (name, parentSpaceId) => {
    const id = nanoid(8);
    set((s) => ({ spaces: { ...s.spaces, [id]: { id, name, parentSpaceId, createdAt: Date.now() } } }));
    return id;
  },
  renameSpace: (id, name) => set((s) => ({ spaces: { ...s.spaces, [id]: { ...s.spaces[id], name } } })),
  setActiveSpace: (id) => set({ activeSpaceId: id, selectedId: null }),

  addDesign: (d) => {
    const id = d.id ?? nanoid(10);
    set((s) => ({ designs: { ...s.designs, [id]: { ...d, id, createdAt: Date.now() } } }));
    return id;
  },
  updateDesign: (id, patch) =>
    set((s) => (s.designs[id] ? { designs: { ...s.designs, [id]: { ...s.designs[id], ...patch } } } : {})),
  removeDesign: (id) =>
    set((s) => {
      const designs = { ...s.designs };
      delete designs[id];
      return { designs, selectedId: s.selectedId === id ? null : s.selectedId };
    }),
  select: (id) => set({ selectedId: id }),
  setGuidelines: (id, on) =>
    set((s) => {
      const d = s.designs[id];
      if (!d) return {};
      const designs = { ...s.designs };
      for (const o of Object.values(s.designs)) {
        if (o.spaceId === d.spaceId && o.guidelinesAt && o.id !== id) designs[o.id] = { ...o, guidelinesAt: undefined };
      }
      designs[id] = { ...d, guidelinesAt: on ? Date.now() : undefined };
      return { designs };
    }),

  generate: async ({ spaceId, parentId, kind = "variation", request, origin, label }) => {
    const jobId = nanoid(8);
    const parent = parentId ? get().designs[parentId] : undefined;
    const run: Run = {
      id: jobId,
      spaceId,
      parentId,
      kind,
      instruction: request.instruction,
      origin: origin ?? defaultOrigin(get().designs, spaceId, parent, request.count),
      label: label ?? (request.instruction || "Variations").slice(0, 60),
      request: withoutBase(request),
      designIds: [],
    };
    get().rememberPrompt(request.instruction);
    set((s) => ({ runs: { ...s.runs, [jobId]: run } }));
    return follow(set, get, jobId, (onEvent, signal) => streamVariations({ ...request, spaceId }, onEvent, signal));
  },

  resumeRuns: (only) => {
    for (const run of Object.values(get().runs)) {
      if (controllers.has(run.id) || (only && !only(run))) continue;
      if (!run.serverJobId) {
        // Reloaded before the server even answered: nothing to reattach to.
        for (const id of run.designIds) get().updateDesign(id, { status: "error", error: "Interrupted before it started" });
        set((s) => ({ runs: omit(s.runs, run.id) }));
        continue;
      }
      const serverJobId = run.serverJobId;
      void follow(set, get, run.id, (onEvent, signal) => resumeVariations(serverJobId, onEvent, signal));
    }
  },

  reapLostRuns: async (only) => {
    const candidates = Object.values(get().runs).filter((r) => !!r.serverJobId && !controllers.has(r.id) && only(r));
    await Promise.all(
      candidates.map(async (r) => {
        if ((await serverJobExists(r.serverJobId!)) !== false) return; // alive (its owner drives it) or unknown
        const run = get().runs[r.id];
        if (!run || controllers.has(run.id) || run.serverJobId !== r.serverJobId) return; // changed meanwhile
        for (const id of run.designIds) {
          if (get().designs[id]?.status === "generating") get().updateDesign(id, { status: "error", error: "Interrupted" });
        }
        set((s) => ({ runs: omit(s.runs, run.id) }));
      }),
    );
  },

  retryDesign: async (id) => {
    const d = get().designs[id];
    if (!d) return;
    const parent = d.parentId ? get().designs[d.parentId] : undefined;
    // Older designs predate stored requests: rebuild one from what the card knows.
    const req: VariationRequest = {
      instruction: d.instruction ?? "",
      ...d.request,
      count: 1,
      direction: d.request?.direction ?? { title: d.title, brief: d.rationale },
      bases: d.request?.bases?.map((b) => ({ ...b, html: get().designs[b.id]?.html ?? "" })),
      // #-mentioned components are refilled from their cards' current html (a deleted card drops out).
      components: d.request?.components
        ?.map((c) => ({ ...c, html: get().designs[c.id]?.html ?? "" }))
        .filter((c) => c.html),
      baseHtml: d.request?.bases?.length ? undefined : parent?.status === "ready" ? parent.html : undefined,
      baseTitle: parent?.title,
    };
    const jobId = nanoid(8);
    get().updateDesign(id, { status: "generating", error: undefined, request: withoutBase(req) });
    set((s) => ({
      runs: {
        ...s.runs,
        [jobId]: {
          id: jobId,
          spaceId: d.spaceId,
          parentId: d.parentId,
          kind: d.kind,
          instruction: req.instruction,
          origin: d.position,
          label: `Retry: ${d.title}`,
          request: withoutBase(req),
          designIds: [id], // the plan's single direction lands back in this card
        },
      },
    }));
    await follow(set, get, jobId, (onEvent, signal) =>
      streamVariations({ ...req, spaceId: d.spaceId }, onEvent, signal),
    );
  },
});

type Source = (onEvent: (e: VariationEvent) => void, signal: AbortSignal) => Promise<void>;
type Set = Parameters<StateCreator<AppState, [], [], CoreSlice>>[0];
type Get = () => AppState;

/** Drive one run from an event source (fresh request or replayed server job) into the store. */
async function follow(set: Set, get: Get, jobId: string, source: Source): Promise<string[]> {
  const ctrl = new AbortController();
  controllers.set(jobId, ctrl);
  const run = () => get().runs[jobId];
  const patchRun = (patch: Partial<Run>) =>
    set((s) => (s.runs[jobId] ? { runs: { ...s.runs, [jobId]: { ...s.runs[jobId], ...patch } } } : {}));
  const patchJob = (patch: Partial<Job>) =>
    set((s) => ({ jobs: { ...s.jobs, [jobId]: { ...s.jobs[jobId], ...patch } } }));

  const r0 = run()!;
  set((s) => ({
    jobs: {
      ...s.jobs,
      [jobId]: s.jobs[jobId] ?? {
        id: jobId,
        label: r0.label,
        spaceId: r0.spaceId,
        status: "running",
        designIds: r0.designIds,
        startedAt: Date.now(),
      },
    },
  }));

  let failed: string | undefined;
  try {
    await source((e) => {
      const { addDesign, updateDesign } = get();
      const r = run();
      if (!r) return;
      if (e.type === "job") {
        patchRun({ serverJobId: e.id });
      } else if (e.type === "plan") {
        // On a replay the placeholders already exist; only create the missing ones.
        const ids = [...r.designIds];
        e.directions.forEach((dir, i) => {
          if (ids[i] && get().designs[ids[i]]) return;
          ids[i] = addDesign({
            spaceId: r.spaceId,
            parentId: r.parentId,
            kind: r.kind,
            title: dir.title,
            rationale: dir.brief,
            html: "",
            status: "generating",
            instruction: r.instruction,
            request: { ...r.request, count: 1, direction: { title: dir.title, brief: dir.brief } },
            // Component-scope results (spokes, "Import from app…") are components for # mentions.
            ...(r.request.scope === "component" ? { component: true } : {}),
            // A variation keeps its base's frame: phone designs stay phone-sized instead of falling back to 1280.
            ...(r.parentId && get().designs[r.parentId]?.viewport ? { viewport: get().designs[r.parentId].viewport } : {}),
            ...(r.parentId && get().designs[r.parentId]?.viewportHeight
              ? { viewportHeight: get().designs[r.parentId].viewportHeight }
              : {}),
            position: { x: r.origin.x + i * (NODE_W + SIBLING_GAP), y: r.origin.y },
          });
        });
        patchRun({ designIds: ids });
        patchJob({ designIds: ids });
      } else if (e.type === "variation") {
        const id = r.designIds[e.index];
        if (id && get().designs[id]) updateDesign(id, { ...e.variation, status: "ready" });
      } else if (e.type === "error") {
        if (e.index !== null && r.designIds[e.index]) updateDesign(r.designIds[e.index], { status: "error", error: e.message });
        else failed = e.message;
      }
    }, ctrl.signal);
  } catch (err) {
    // A reload aborts the fetch, not the server job: leave the run in place so it resumes.
    if (ctrl.signal.aborted && !cancelled.has(jobId)) return run()?.designIds ?? [];
    failed = err instanceof Error ? err.message : String(err);
  } finally {
    controllers.delete(jobId);
  }
  if (cancelled.delete(jobId)) failed ??= "Cancelled";
  const ids = run()?.designIds ?? [];
  // Anything still generating at the end never arrived.
  for (const id of ids) {
    if (get().designs[id]?.status === "generating") get().updateDesign(id, { status: "error", error: failed ?? "No result" });
  }
  patchJob({ status: failed ? "error" : "done", error: failed });
  set((s) => ({ runs: omit(s.runs, jobId) }));
  return ids.filter(Boolean);
}

const cancelled = new Set<string>();

function withoutBase(r: VariationRequest): Omit<VariationRequest, "baseHtml"> {
  const { baseHtml: _drop, ...rest } = r;
  // A screenshot is a large data: URL; it only travels with the request, never into shared state.
  if (rest.importFrom?.screenshot) rest.importFrom = { element: rest.importFrom.element, source: rest.importFrom.source };
  // Combined sources keep their id/title only; retryDesign refills the html from the current designs.
  if (rest.components) rest.components = rest.components.map((c) => ({ ...c, html: "" }));
  return rest.bases ? { ...rest, bases: rest.bases.map((b) => ({ ...b, html: "" })) } : rest;
}

function omit<T>(rec: Record<string, T>, key: string) {
  const next = { ...rec };
  delete next[key];
  return next;
}

export function cancelJob(jobId: string) {
  cancelled.add(jobId);
  const serverJobId = (globalThis as { __prototypieStore?: () => AppState }).__prototypieStore?.().runs[jobId]?.serverJobId;
  if (serverJobId) cancelServerJob(serverJobId);
  controllers.get(jobId)?.abort();
}

function defaultOrigin(designs: Record<string, Design>, spaceId: string, parent: Design | undefined, count: number) {
  const inSpace = Object.values(designs).filter((d) => d.spaceId === spaceId);
  if (parent) {
    const width = count * NODE_W + (count - 1) * SIBLING_GAP;
    const y = parent.position.y + NODE_H + 220;
    let x = parent.position.x + NODE_W / 2 - width / 2;
    // A parent varied twice would stack the new row on its earlier children: slide right past anything in the way.
    for (let guard = 0; guard < 50; guard++) {
      const hit = inSpace.filter(
        (d) =>
          d.position.x < x + width + SIBLING_GAP &&
          d.position.x + NODE_W + SIBLING_GAP > x &&
          d.position.y < y + NODE_H &&
          d.position.y + NODE_H > y,
      );
      if (!hit.length) break;
      x = Math.max(...hit.map((d) => d.position.x)) + NODE_W + SIBLING_GAP;
    }
    return { x, y };
  }
  if (!inSpace.length) return { x: 0, y: 0 };
  const maxX = Math.max(...inSpace.map((d) => d.position.x));
  const minY = Math.min(...inSpace.map((d) => d.position.y));
  return { x: maxX + NODE_W + 160, y: minY };
}
