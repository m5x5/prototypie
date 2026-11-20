import type { PodAccess, PodCatalog, PodCatalogEntry } from "@/lib/types";
import type { LfItem, LfRelation, LfShape } from "./scene";

/**
 * Turns GET /api/pod-catalog into what the Layout-Finder compares: groups of resources (one per catalog shape,
 * plus resources no shape claims) and the relations the catalog documents between them. Only catalog values
 * are used; edges listed in reverse ("← …") are the same edge seen from the target and are skipped.
 */

export interface LayoutModel {
  shapes: LfShape[];
  relations: LfRelation[];
  /** When the catalog's live probes ran. */
  generatedAt: string;
}

const COLORS = ["#d97757", "#2563eb", "#7c3aed", "#0d9488", "#ca8a04", "#db2777", "#525252"];
/** Plain names for the groups by container, so chips read "Aufgaben" instead of a type IRI. */
const NAME_BY_CONTAINER: Record<string, string> = {
  "tasks/": "Aufgaben",
  "events/": "Termine",
  "courses/": "Kurse",
  "profile/": "Profile",
};

/** "2026-10-21T12:15 (Europe/Berlin)" (wall time) or an ISO instant → epoch ms. */
export function parseTime(v: string | undefined): number | undefined {
  if (!v) return undefined;
  const wall = v.match(/^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?\s*\(([^)]+)\)$/);
  if (wall) {
    const [, y, mo, d, h, mi, s, tz] = wall;
    const guess = Date.UTC(+y, +mo - 1, +d, +h, +mi, +(s ?? 0));
    // Offset of that zone at that moment: format the guess in the zone and compare.
    const parts = Object.fromEntries(
      new Intl.DateTimeFormat("en-US", { timeZone: tz, hourCycle: "h23", year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" })
        .formatToParts(guess)
        .map((p) => [p.type, p.value]),
    );
    const asZone = Date.UTC(+parts.year, +parts.month - 1, +parts.day, +parts.hour, +parts.minute, +parts.second);
    return guess - (asZone - guess);
  }
  const t = Date.parse(v);
  return Number.isNaN(t) ? undefined : t;
}

const field = (e: PodCatalogEntry, ...ps: string[]) => e.fields.find((f) => ps.includes(f.p))?.v;

function item(e: PodCatalogEntry): LfItem {
  return {
    id: e.id,
    label: e.label,
    start: parseTime(field(e, "ical:due", "start", "dtstart", "schema:startDate")),
    end: parseTime(field(e, "end", "dtend", "schema:endDate")),
  };
}

function groupName(members: PodCatalogEntry[], fallback: string): string {
  const containers = new Set(members.map((m) => (m.path.match(/^[^/]+\//)?.[0] ?? "")));
  if (containers.size === 1) {
    const name = NAME_BY_CONTAINER[[...containers][0]];
    if (name) return name;
  }
  return fallback;
}

/** Weakest documented access wins for a group/edge: 401 < ref < cache < live (what you can rely on). */
const RANK: Record<PodAccess, number> = { "401": 0, ref: 1, cache: 2, live: 3 };
const weakest = (xs: PodAccess[]): PodAccess => xs.reduce((m, x) => (RANK[x] < RANK[m] ? x : m), "live" as PodAccess);

export function buildLayoutModel(cat: PodCatalog): LayoutModel {
  const byId = new Map(cat.entries.map((e) => [e.id, e]));
  const podName = (id: string) => cat.pods.find((p) => p.id === id)?.display ?? id;
  const resources = cat.entries.filter((e) => e.kind === "resource");

  const shapes: LfShape[] = [];
  const groupOf = new Map<string, string>();
  const used = new Set<string>();
  for (const s of cat.entries.filter((e) => e.kind === "shape")) {
    const members = (s.children ?? []).map((id) => byId.get(id)).filter((m): m is PodCatalogEntry => m?.kind === "resource");
    members.forEach((m) => groupOf.set(m.id, s.id));
    members.forEach((m) => used.add(m.id));
    // Same label twice in one group (both pods' profile/card#me): tell them apart by pod.
    const items = members.map((m) => {
      const it = item(m);
      if (members.some((o) => o !== m && o.label === m.label)) it.label = `${m.label} · ${cat.pods.find((p) => p.id === m.pod)?.label ?? m.pod}`;
      return it;
    });
    shapes.push({
      id: s.id,
      name: groupName(members, s.label),
      type: s.label,
      access: s.access,
      color: "",
      items,
      source: `${podName(s.pod)} · ${s.path}`,
    });
  }
  for (const r of resources.filter((e) => !used.has(e.id))) {
    groupOf.set(r.id, r.id);
    shapes.push({
      id: r.id,
      name: groupName([r], r.label),
      type: r.types?.join(" · ") ?? r.kind,
      access: r.access,
      color: "",
      items: [item(r)],
      source: `${podName(r.pod)} · ${r.path}`,
    });
  }
  // Groups with entries first, then the ones that only document a type.
  shapes.sort((a, b) => Number(!a.items.length) - Number(!b.items.length));
  shapes.forEach((s, i) => (s.color = COLORS[i % COLORS.length]));

  // Direct edges between groups, aggregated per (from group, predicate, to group).
  const direct = new Map<string, LfRelation>();
  for (const r of resources) {
    for (const rel of r.relations ?? []) {
      if (rel.p.startsWith("←")) continue;
      const target = byId.get(rel.target);
      const ga = groupOf.get(r.id);
      const gb = target && groupOf.get(target.id);
      if (!ga || !gb || ga === gb) continue;
      const key = `${ga}|${rel.p}|${gb}`;
      const cur = direct.get(key) ?? { a: ga, b: gb, predicate: rel.p, access: r.access, pairs: [], evidence: r.provenance };
      cur.pairs.push([r.id, target.id]);
      cur.access = weakest([cur.access, r.access]);
      direct.set(key, cur);
    }
  }
  const relations = [...direct.values()];

  // Two groups pointing at the same target item are related through it (documented on both sides).
  const edges = [...direct.values()];
  for (let i = 0; i < edges.length; i++) {
    for (let j = i + 1; j < edges.length; j++) {
      const x = edges[i];
      const y = edges[j];
      if (x.b !== y.b || x.a === y.a) continue;
      const pairs: [string, string][] = [];
      for (const [xa, xt] of x.pairs) for (const [ya, yt] of y.pairs) if (xt === yt) pairs.push([xa, ya]);
      if (!pairs.length) continue;
      const via = shapes.find((s) => s.id === x.b)!;
      relations.push({
        a: x.a,
        b: y.a,
        predicate: `${x.predicate} ↔ ${y.predicate}`,
        via: via.items.length === 1 ? via.items[0].label : via.name,
        access: weakest([x.access, y.access]),
        pairs,
        evidence: [x.evidence, y.evidence].filter(Boolean).join(" · "),
      });
    }
  }

  return { shapes, relations, generatedAt: cat.checkedAt };
}

export async function loadLayoutModel(): Promise<LayoutModel> {
  const res = await fetch("/api/pod-catalog", { cache: "no-store" });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  return buildLayoutModel((await res.json()) as PodCatalog);
}
