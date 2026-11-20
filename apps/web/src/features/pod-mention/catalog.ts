"use client";
import { useEffect, useState } from "react";
import type { PodAccess, PodCatalog, PodCatalogEntry, PodContextEntry, PodEntryKind } from "@/lib/types";

/** Display order of the @-mention groups. */
export const KIND_ORDER: PodEntryKind[] = ["resource", "container", "shape", "pod"];
export const KIND_LABEL: Record<PodEntryKind, string> = {
  resource: "Resources",
  container: "Containers",
  shape: "Shapes · types",
  pod: "Pods",
};
export const ACCESS_LABEL: Record<PodAccess, string> = { live: "live", "401": "401", ref: "reference", cache: "cache" };

let cached: Promise<PodCatalog> | null = null;
/** One fetch per page load; a failed fetch is retried on the next mount. */
function loadCatalog() {
  cached ??= fetch("/api/pod-catalog")
    .then((r) => (r.ok ? (r.json() as Promise<PodCatalog>) : Promise.reject(new Error(`HTTP ${r.status}`))))
    .catch((e) => {
      cached = null;
      throw e;
    });
  return cached;
}

export function usePodCatalog(enabled = true) {
  const [catalog, setCatalog] = useState<PodCatalog | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!enabled || catalog) return;
    let live = true;
    loadCatalog().then(
      (c) => live && setCatalog(c),
      (e: unknown) => live && setError(e instanceof Error ? e.message : String(e)),
    );
    return () => {
      live = false;
    };
  }, [enabled, catalog]);
  return { catalog, error };
}

/** Field key for the "flows into generation" toggles. */
export const fieldKey = (entryId: string, index: number) => `${entryId}:${index}`;

export const sourceOf = (catalog: PodCatalog, e: PodCatalogEntry) => {
  const pod = catalog.pods.find((p) => p.id === e.pod);
  const root = pod?.display ?? e.pod;
  return e.kind === "shape" ? `${root} · ${e.path}` : `${root}${e.path ? (root.endsWith("/") ? "" : "/") + e.path : ""}`;
};

/** Entries matching the text typed after "@" (label, path or group), grouped in display order. */
export function matchEntries(catalog: PodCatalog, query: string): PodCatalogEntry[] {
  const q = query.trim().toLowerCase();
  const hit = (e: PodCatalogEntry) => !q || [e.label, e.path, KIND_LABEL[e.kind]].some((s) => s.toLowerCase().includes(q));
  return KIND_ORDER.flatMap((k) => catalog.entries.filter((e) => e.kind === k && hit(e)));
}

/** The structured context sent with the generation request: mentioned entries with their selected fields. */
export function toPodContext(catalog: PodCatalog, ids: string[], off: Record<string, boolean>): PodContextEntry[] {
  return ids.flatMap((id) => {
    const e = catalog.entries.find((x) => x.id === id);
    if (!e) return [];
    return [
      {
        id: e.id,
        kind: e.kind,
        label: e.label,
        source: sourceOf(catalog, e),
        access: e.access,
        ...(e.stand ? { stand: e.stand } : {}),
        fields: e.fields.filter((_, i) => !off[fieldKey(e.id, i)]),
        ...(e.relations?.length ? { relations: e.relations } : {}),
        ...(e.note ? { note: e.note } : {}),
      },
    ];
  });
}
