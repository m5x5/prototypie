"use client";
import { useEffect, useMemo, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { useApp, type AppState } from "@/store";
import type { Space } from "@/lib/types";

/**
 * Which local project folder a space belongs to: its own `localPath` link, else the nearest ancestor's, else a
 * project matched by the space's name (GET /api/projects?match=). Plus the project's record on the Solid pod.
 */

/** Name matches, fetched once per name per page. */
const nameMatches = new Map<string, Promise<string | undefined>>();
function matchName(name: string) {
  let req = nameMatches.get(name);
  if (!req) {
    req = fetch(`/api/projects?match=${encodeURIComponent(name)}`, { cache: "no-store" })
      .then((r) => (r.ok ? r.json() : {}))
      .then((d: { localPath?: string }) => d.localPath)
      .catch(() => undefined);
    nameMatches.set(name, req);
  }
  return req;
}

/** The nearest linked project folder: the space's own `localPath`, else that of the closest ancestor. */
function linkedPath(s: AppState, spaceId: string): string | undefined {
  const seen = new Set<string>();
  for (let sp: Space | undefined = s.spaces[spaceId]; sp && !seen.has(sp.id); sp = sp.parentSpaceId ? s.spaces[sp.parentSpaceId] : undefined) {
    if (sp.localPath) return sp.localPath;
    seen.add(sp.id);
  }
  return undefined;
}

/** Local project folders for several spaces, in the same order (undefined: none found). */
export function useSpaceLocalPaths(spaceIds: string[]): (string | undefined)[] {
  const linked = useApp(useShallow((s) => spaceIds.map((id) => linkedPath(s, id))));
  const names = useApp(useShallow((s) => spaceIds.map((id) => s.spaces[id]?.name ?? "")));
  const [matched, setMatched] = useState<Record<string, string | undefined>>({});
  const pending = names.filter((n, i) => n && !linked[i]);
  const key = pending.join("\n");
  useEffect(() => {
    if (!key) return;
    let live = true;
    for (const n of key.split("\n")) void matchName(n).then((p) => live && setMatched((m) => (n in m ? m : { ...m, [n]: p })));
    return () => {
      live = false;
    };
  }, [key]);
  return spaceIds.map((_, i) => linked[i] ?? (names[i] ? matched[names[i]] : undefined));
}

export function useSpaceLocalPath(spaceId: string): string | undefined {
  return useSpaceLocalPaths([spaceId])[0];
}

export interface PodRecord {
  /** The pod document holding the record (e.g. https://…/me/projects/index.ttl). */
  doc: string;
  /** The project's subject in it (urn:example:project:<id>). */
  subject: string;
}

/** Local folder (lower-cased) -> pod record, from the projects in private-projects.ttl; fetched once per page. */
let podIndex: Promise<Map<string, PodRecord>> | undefined;
function loadPodIndex() {
  podIndex ??= fetch("/api/projects", { cache: "no-store" })
    .then((r) => (r.ok ? r.json() : { projects: [] }))
    .then((d: { projects: { localPath?: string; pod?: PodRecord }[] }) => {
      const m = new Map<string, PodRecord>();
      for (const p of d.projects) if (p.localPath && p.pod) m.set(p.localPath.toLowerCase(), p.pod);
      return m;
    })
    .catch(() => new Map<string, PodRecord>());
  return podIndex;
}

/** The pod record of the project at each local folder (undefined: that folder isn't a project on the pod). */
export function usePodRecords(paths: (string | undefined)[]): (PodRecord | undefined)[] {
  const [index, setIndex] = useState<Map<string, PodRecord>>();
  const wanted = paths.some(Boolean);
  useEffect(() => {
    if (!wanted) return;
    let live = true;
    void loadPodIndex().then((m) => live && setIndex(m));
    return () => {
      live = false;
    };
  }, [wanted]);
  return useMemo(() => paths.map((p) => (p ? index?.get(p.toLowerCase()) : undefined)), [paths, index]);
}

/** /Users/<me>/x -> ~/x, for display only. */
export const tildePath = (p: string) => p.replace(/^\/(?:Users|home)\/[^/]+(?=\/|$)/, "~");
