import { readFile } from "node:fs/promises";
import path from "node:path";
import type { PodAccess, PodCatalog, PodCatalogEntry, PodContextEntry } from "@/lib/types";

/**
 * Honest catalog of the user's Solid pod data for data-driven explorations (@-mentions, layout finder).
 *
 * The documented entries live in data/pod-catalog.json (generated from the pod findings, provenance per entry).
 * Entries marked `live` there are probed with an unauthenticated GET (Accept: text/turtle, short timeout,
 * cached for a few minutes): a 200 makes them "live", a 401/403 makes them "401", and a network error keeps the
 * documented state. Read-only: no writes, no credentials. Values of locked resources only ever come from the file.
 */

interface PodDef {
  id: string;
  label: string;
  base: string;
  display: string;
  note?: string;
}
interface FileEntry extends Omit<PodCatalogEntry, "probe"> {
  live?: boolean;
  accept?: string;
}
interface CatalogFile {
  version: number;
  pods: PodDef[];
  entries: FileEntry[];
}

const FILE = path.join(process.cwd(), "data", "pod-catalog.json");
const TTL_MS = 5 * 60_000;
const TIMEOUT_MS = 2500;
/** Pod roots can be overridden per pod id, e.g. POD_BASE_HOME=https://…/me/ */
const baseOf = (p: PodDef) => process.env[`POD_BASE_${p.id.toUpperCase()}`] || p.base;

type Probe = NonNullable<PodCatalogEntry["probe"]>;
const g = globalThis as unknown as { __podCatalog?: { at: number; value: PodCatalog } };

async function probe(url: string, accept: string): Promise<Probe> {
  const checkedAt = new Date().toISOString();
  try {
    const res = await fetch(url, { headers: { accept }, signal: AbortSignal.timeout(TIMEOUT_MS), cache: "no-store" });
    if (!res.ok) return { status: res.status, checkedAt };
    const text = url.endsWith("/") ? await res.text() : "";
    return { status: res.status, checkedAt, ...(text ? { contains: containedNames(text) } : {}) };
  } catch {
    return { status: "error", checkedAt };
  }
}

/** Best-effort names from an LDP container listing in Turtle (ldp:contains <a>, <b> .). */
export function containedNames(turtle: string): string[] {
  const names = new Set<string>();
  for (const m of turtle.matchAll(/ldp:contains\s+((?:<[^>]*>\s*,?\s*)+)/g)) {
    for (const iri of m[1].matchAll(/<([^>]+)>/g)) {
      const parts = iri[1].split("/");
      const last = iri[1].endsWith("/") ? parts[parts.length - 2] + "/" : parts[parts.length - 1];
      if (last && last !== "/") names.add(decodeURIComponent(last));
    }
  }
  return [...names].sort();
}

function accessAfter(documented: PodAccess, p: Probe | undefined): PodAccess {
  if (!p || p.status === "error") return documented;
  if (p.status >= 200 && p.status < 300) return "live";
  if (p.status === 401 || p.status === 403) return "401";
  return documented;
}

async function build(): Promise<PodCatalog> {
  const file = JSON.parse(await readFile(FILE, "utf8")) as CatalogFile;
  const pods = new Map(file.pods.map((p) => [p.id, p]));
  const entries = await Promise.all(
    file.entries.map(async ({ live, accept, ...e }): Promise<PodCatalogEntry> => {
      const pod = pods.get(e.pod);
      if (!live || !pod) return e;
      const result = await probe(new URL(e.path, baseOf(pod)).toString(), accept ?? "text/turtle");
      return { ...e, access: accessAfter(e.access, result), probe: result };
    }),
  );
  return {
    version: file.version,
    pods: file.pods.map(({ id, label, display, note }) => ({ id, label, display, ...(note ? { note } : {}) })),
    entries,
    checkedAt: new Date().toISOString(),
  };
}

/** The catalog with fresh-enough live probes. `refresh` skips the cache. */
export async function getPodCatalog({ refresh = false } = {}): Promise<PodCatalog> {
  const hit = g.__podCatalog;
  if (!refresh && hit && Date.now() - hit.at < TTL_MS) return hit.value;
  const value = await build();
  g.__podCatalog = { at: Date.now(), value };
  return value;
}

const ACCESS_TEXT: Record<PodAccess, string> = {
  live: "publicly readable (live)",
  "401": "locked (401), values from documented captures",
  ref: "known by reference only",
  cache: "app cache",
};

/** Turn the @-mentioned pod data into a prompt section for the design agent. */
export function describePodContext(ctx: PodContextEntry[] | undefined): string | undefined {
  if (!ctx?.length) return undefined;
  const blocks = ctx.map((e) => {
    const head = `- ${e.label} [${e.kind}] · ${e.source} · ${ACCESS_TEXT[e.access]}${e.stand ? ` · ${e.stand}` : ""}`;
    const fields = e.fields.map((f) => `    ${f.p}${f.v ? `: ${f.v}` : f.type ? ` (${f.type})` : ""}`);
    const rel = (e.relations ?? []).map((r) => `    ${r.p} → ${r.target}`);
    return [head, ...fields, ...rel, ...(e.note ? [`    note: ${e.note}`] : [])].join("\n");
  });
  return (
    `Pod data mentioned by the designer (Solid pod, read-only). Use these real values and shapes; don't invent ` +
    `values for fields that aren't listed, and show gaps honestly (locked, unread):\n${blocks.join("\n")}`
  );
}
