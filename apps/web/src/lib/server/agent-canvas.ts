// Server-side canvas editing for the coding agent (Claude Code via the agent-queue watcher):
// create cards (designs) and explorations (nested spaces) directly in the shared state, so an agent
// can answer a prompt with several cards or a new exploration instead of exactly one card per queue item.
// Writes go through applyChanges, so every open browser picks them up live.
import { nanoid } from "nanoid";
import { z } from "zod";
import type { Design, Space } from "@/lib/types";
import { applyChanges, readState, type Change } from "./state-store";

// Mirrors src/store/core.ts and src/features/nested/slice.ts (not imported: those are client modules).
const ROOT = "root";
const NODE_W = 380;
const NODE_H = 420;
const GAP = 160;
const PORTAL_W = 300;
const PORTAL_H = 330;

type Box = { x: number; y: number; w: number; h: number };

async function snapshot() {
  const { records } = await readState();
  const live = <T>(prefix: string) =>
    Object.entries(records)
      .filter(([p, r]) => p.startsWith(prefix) && !r.deleted && r.value !== undefined)
      .map(([p, r]) => [p.slice(prefix.length), r.value as T] as const);
  return {
    spaces: Object.fromEntries(live<Space>("spaces/")),
    designs: Object.fromEntries(live<Design>("designs/")),
    portals: Object.fromEntries(live<{ x: number; y: number }>("portals/")),
  };
}
type Snap = Awaited<ReturnType<typeof snapshot>>;

function boxes(s: Snap, spaceId: string): Box[] {
  const out: Box[] = Object.values(s.designs)
    .filter((d) => d.spaceId === spaceId)
    .map((d) => ({ x: d.position.x, y: d.position.y, w: NODE_W, h: cardH(d.viewport, d.viewportHeight) }));
  for (const [id, p] of Object.entries(s.portals)) {
    if (s.spaces[id]?.parentSpaceId === spaceId) out.push({ x: p.x, y: p.y, w: PORTAL_W, h: PORTAL_H });
  }
  return out;
}

const hits = (a: Box, list: Box[]) =>
  list.some((b) => a.x < b.x + b.w + 40 && a.x + a.w + 40 > b.x && a.y < b.y + b.h + 40 && a.y + a.h + 40 > b.y);

/** Free spots for `n` items of size w×h in a row: right of `near` when given, else below everything. */
function placeRow(taken: Box[], n: number, w: number, h: number, near?: { x: number; y: number }): { x: number; y: number }[] {
  const out: { x: number; y: number }[] = [];
  const occupied = [...taken];
  let x: number, y: number;
  if (near) {
    x = near.x + NODE_W + GAP;
    y = near.y;
  } else {
    x = occupied.length ? Math.min(...occupied.map((b) => b.x)) : 0;
    y = occupied.length ? Math.max(...occupied.map((b) => b.y + b.h)) + GAP : 0;
  }
  for (let i = 0; i < n; i++) {
    let box = { x, y, w, h };
    while (hits(box, occupied)) box = { ...box, x: box.x + w + GAP };
    occupied.push(box);
    out.push({ x: box.x, y: box.y });
    x = box.x + w + GAP;
  }
  return out;
}

export class CanvasError extends Error {
  constructor(
    message: string,
    readonly status = 400,
  ) {
    super(message);
  }
}

// Everything written here lands in shared state that every browser renders, so inputs are validated strictly.
const Id = z.string().regex(/^[\w-]{1,64}$/, "must be an id (letters, digits, _ or -)");
export const CardSchema = z.object({
  title: z.string().trim().min(1).max(200),
  rationale: z.string().max(4000).optional(),
  html: z.string().trim().min(1, "html is empty"),
  /** Virtual width the design is laid out at (default 1280; phone screens ~400). */
  viewport: z.number().int().min(200).max(4000).optional(),
  /** Height of a fixed-size screen (e.g. 960 for a 640×960 phone): the card frame keeps that ratio. */
  viewportHeight: z.number().int().min(200).max(4000).optional(),
  kind: z.enum(["page", "variation", "spoke"]).optional(),
  /** Design it was derived from (draws an edge when in the same space). */
  parentId: Id.optional(),
  /** A reusable component rather than a page: listed first when the designer #-mentions components. */
  component: z.boolean().optional(),
});
export type CardInput = z.infer<typeof CardSchema>;
export const CardsBody = z.object({
  spaceId: Id.optional(),
  nearId: Id.optional(),
  cards: z.array(CardSchema).min(1).max(50),
});
export const ExplorationBody = z.object({
  name: z.string().trim().min(1).max(120),
  parentSpaceId: Id.optional(),
  seedId: Id.optional(),
  cards: z.array(CardSchema).max(50).optional(),
});

/** Box height of a card on the canvas (phone-width and portrait fixed-size designs render taller). */
const cardH = (viewport?: number, viewportHeight?: number) =>
  (viewport ?? 1280) < 600 || (viewportHeight ?? 0) > (viewport ?? 1280) ? 560 : NODE_H;

/** Build ready cards for a space (no write). `taken` are the boxes already occupied there. */
function buildCards(s: Snap, spaceId: string, taken: Box[], cards: CardInput[], near?: { x: number; y: number }): Design[] {
  for (const [i, c] of cards.entries()) {
    if (c.parentId && !s.designs[c.parentId]) throw new CanvasError(`cards[${i}].parentId: unknown design`, 404);
  }
  // Row height is the tallest card in it, so phone cards don't overlap what's below.
  const h = Math.max(...cards.map((c) => cardH(c.viewport, c.viewportHeight)));
  const spots = placeRow(taken, cards.length, NODE_W, h, near);
  const now = Date.now();
  return cards.map((c, i) => ({
    id: nanoid(10),
    spaceId,
    parentId: c.parentId,
    kind: c.kind ?? "variation",
    title: c.title,
    rationale: c.rationale ?? "",
    html: c.html,
    status: "ready",
    position: spots[i],
    createdAt: now + i,
    ...(c.viewport ? { viewport: c.viewport } : {}),
    ...(c.viewportHeight ? { viewportHeight: c.viewportHeight } : {}),
    ...(c.component ? { component: true } : {}),
  }));
}

const ROOT_REJECTED =
  "The main canvas holds only projects: pass the spaceId of a project or one of its explorations (GET /api/agent/canvas lists them; kind \"project\").";

/** Create ready cards in a space, in a row right of `nearId` (or below the space's content). */
export async function createCards(input: z.infer<typeof CardsBody>) {
  const s = await snapshot();
  const near = input.nearId ? s.designs[input.nearId] : undefined;
  if (input.nearId && !near) throw new CanvasError(`Unknown design ${input.nearId}`, 404);
  const spaceId = input.spaceId ?? near?.spaceId ?? ROOT;
  if (spaceId === ROOT) throw new CanvasError(ROOT_REJECTED);
  if (!s.spaces[spaceId]) throw new CanvasError(`Unknown space ${spaceId}`, 404);
  const created = buildCards(s, spaceId, boxes(s, spaceId), input.cards, near?.spaceId === spaceId ? near.position : undefined);
  await applyChanges(
    created.map((d): Change => ({ path: `designs/${d.id}`, value: d })),
    { clientId: "agent", reason: "agent-cards" },
  );
  return { spaceId, ids: created.map((d) => d.id) };
}

/**
 * Create an exploration (nested space) with its portal card in the parent space. With `seedId`, a copy of
 * that design becomes the exploration's starting card (like the app's "Explore" button). Optional `cards`
 * are created inside the new space.
 */
export async function createExploration(input: z.infer<typeof ExplorationBody>) {
  const s = await snapshot();
  const seed = input.seedId ? s.designs[input.seedId] : undefined;
  if (input.seedId && !seed) throw new CanvasError(`Unknown design ${input.seedId}`, 404);
  const parentSpaceId = input.parentSpaceId ?? seed?.spaceId ?? ROOT;
  if (parentSpaceId === ROOT) throw new CanvasError(ROOT_REJECTED);
  if (!s.spaces[parentSpaceId]) throw new CanvasError(`Unknown space ${parentSpaceId}`, 404);

  // Everything is built first and written in one batch: a bad card can't leave a half-made exploration.
  const spaceId = nanoid(8);
  const now = Date.now();
  const [portal] = placeRow(boxes(s, parentSpaceId), 1, PORTAL_W, PORTAL_H, seed?.spaceId === parentSpaceId ? seed.position : undefined);
  const changes: Change[] = [
    { path: `spaces/${spaceId}`, value: { id: spaceId, name: input.name, parentSpaceId, createdAt: now } satisfies Space },
    { path: `portals/${spaceId}`, value: portal },
  ];
  let seedCopyId: string | undefined;
  const inside: Box[] = [];
  if (seed) {
    seedCopyId = nanoid(10);
    const { id: _id, createdAt: _c, ...rest } = seed;
    changes.push(
      { path: `designs/${seedCopyId}`, value: { ...rest, id: seedCopyId, spaceId, parentId: undefined, starred: false, position: { x: 0, y: 0 }, createdAt: now } },
      { path: `seededFrom/${seedCopyId}`, value: seed.id },
    );
    inside.push({ x: 0, y: 0, w: NODE_W, h: cardH(seed.viewport, seed.viewportHeight) });
  }
  const cards = input.cards?.length ? buildCards(s, spaceId, inside, input.cards, seed ? { x: 0, y: 0 } : undefined) : [];
  for (const d of cards) changes.push({ path: `designs/${d.id}`, value: d });
  await applyChanges(changes, { clientId: "agent", reason: "agent-exploration" });
  return { spaceId, portal, seedId: seedCopyId, cardIds: cards.map((d) => d.id) };
}

/** Spaces with their cards (ids, titles, positions), for an agent to find where to put things. */
export async function describeCanvas() {
  const s = await snapshot();
  const spaces = { [ROOT]: { id: ROOT, name: "Main canvas", createdAt: 0 } as Space, ...s.spaces };
  return Object.values(spaces).map((sp) => ({
    id: sp.id,
    name: sp.name,
    parentSpaceId: sp.parentSpaceId,
    ...(sp.kind ? { kind: sp.kind } : {}),
    ...(sp.localPath ? { localPath: sp.localPath } : {}),
    cards: Object.values(s.designs)
      .filter((d) => d.spaceId === sp.id)
      .map((d) => ({ id: d.id, title: d.title, status: d.status, kind: d.kind, position: d.position })),
  }));
}
