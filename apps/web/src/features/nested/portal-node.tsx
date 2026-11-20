"use client";
import { memo, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import type { Node, NodeProps } from "@xyflow/react";
import { ArrowUpFromLine, FolderGit2, FolderTree, LayoutGrid, Layers, Star, Trash2 } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { Button } from "@/components/ui/button";
import { DesignPreview } from "@/components/design-preview";
import { useApp, type AppState } from "@/store";
import { cn } from "@/lib/utils";
import type { Design } from "@/lib/types";
import { PORTAL_W, descendantSpaceIds, isProject } from "./slice";
import { enterSpace, useNestedUI } from "./nav";
import { tildePath, useSpaceLocalPath } from "./local-project";

export type PortalNodeType = Node<{ spaceId: string }, "portal">;

/** Designs inside a space and all of its sub-spaces (stable references, for shallow comparison). */
export function selectSubtreeDesigns(s: AppState, spaceId: string): Design[] {
  const ids = new Set([spaceId, ...descendantSpaceIds(s, spaceId)]);
  return Object.values(s.designs).filter((d) => ids.has(d.spaceId));
}

/* ---- when a space was last left (per browser, localStorage via useSyncExternalStore so hydration stays safe) ---- */

const leftKey = (spaceId: string) => `prototypie:space-left:${spaceId}`;
const prefListeners = new Set<() => void>();
function subscribePrefs(cb: () => void) {
  prefListeners.add(cb);
  window.addEventListener("storage", cb);
  return () => {
    prefListeners.delete(cb);
    window.removeEventListener("storage", cb);
  };
}
function readPref(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function writePref(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // private mode etc.: "new" then falls back to the last 24 hours
  }
  prefListeners.forEach((l) => l());
}
function usePref(key: string) {
  return useSyncExternalStore(subscribePrefs, () => readPref(key), () => null);
}

/** Remember when a space was last left, so its portal can mark what arrived since (installed once per page). */
let visitTracking = false;
function trackSpaceVisits() {
  if (visitTracking) return;
  visitTracking = true;
  useApp.subscribe((s, prev) => {
    if (s.activeSpaceId !== prev.activeSpaceId && prev.activeSpaceId) writePref(leftKey(prev.activeSpaceId), String(Date.now()));
  });
}

const NEW_WINDOW = 24 * 60 * 60 * 1000;

/**
 * The first ready design of `heroRefs` (else the newest ready one of `pool`) as a real preview; a shimmer
 * slot until one of the pool's designs is ready.
 */
function Hero({ pool, heroRefs, project }: { pool: Design[]; heroRefs: string[]; project: boolean }) {
  const hero = useMemo(() => {
    const byId = new Map(pool.map((d) => [d.id, d]));
    const fromPromote = heroRefs.map((id) => byId.get(id)).find((d) => d?.status === "ready");
    return fromPromote ?? [...pool].filter((d) => d.status === "ready").sort((a, b) => b.createdAt - a.createdAt)[0];
  }, [pool, heroRefs]);

  if (!pool.length) {
    return (
      <div className="mx-2.5 flex aspect-[16/10] flex-col items-center justify-center gap-1.5 rounded-[9px] border bg-muted/30 text-muted-foreground" aria-hidden>
        <FolderTree className="size-5" />
        <span className="text-xs">{project ? "Empty project" : "Empty exploration"}</span>
      </div>
    );
  }
  return (
    <div className="px-2.5">
      {hero ? (
        <div className="relative overflow-hidden rounded-[9px] border bg-muted/30">
          <DesignPreview html={hero.html} viewport={hero.viewport} className="aspect-[16/10] w-full" />
          {hero.starred && (
            <span className="absolute top-2 right-2 flex size-[18px] items-center justify-center rounded-full bg-card shadow-sm" title="Starred">
              <Star className="size-2.5 fill-amber-400 text-amber-400" />
            </span>
          )}
        </div>
      ) : (
        <div className="aspect-[16/10] w-full animate-pulse rounded-[9px] border border-dashed border-blue-500/50 bg-muted motion-reduce:animate-none" title="Designing…" />
      )}
    </div>
  );
}

function PortalNodeImpl({ data, selected }: NodeProps<PortalNodeType>) {
  const space = useApp((s) => s.spaces[data.spaceId]);
  const designs = useApp(useShallow((s) => selectSubtreeDesigns(s, data.spaceId)));
  // The space's own cards feed the hero and the strip (stable references).
  const own = useApp(useShallow((s) => Object.values(s.designs).filter((d) => d.spaceId === data.spaceId)));
  const subSpaces = useApp((s) => descendantSpaceIds(s, data.spaceId).length);
  const promotedFrom = useApp((s) => s.promotedFrom);
  const askDelete = useNestedUI((s) => s.askDelete);
  const localPath = useSpaceLocalPath(data.spaceId);
  const leftRaw = usePref(leftKey(data.spaceId));
  const lastLeft = leftRaw ? Number(leftRaw) || null : null;
  const [now] = useState(() => Date.now());
  useEffect(trackSpaceVisits, []);

  const promotedIds = useMemo(() => new Set(Object.values(promotedFrom).map((r) => r.designId)), [promotedFrom]);
  const project = isProject(space);
  /**
   * The hero's first choice, newest promotion first: for an exploration its designs promoted to the parent; for a
   * project (which mostly holds explorations) the designs promoted up into it.
   */
  const heroRefs = useMemo(
    () =>
      Object.entries(promotedFrom)
        .filter(([copyId, r]) => (project ? designs.some((d) => d.id === copyId && d.spaceId === data.spaceId) : r.spaceId === data.spaceId))
        .sort(([, a], [, b]) => b.at - a.at)
        .map(([copyId, r]) => (project ? copyId : r.designId)),
    [promotedFrom, data.spaceId, project, designs],
  );
  const newCount = useMemo(
    () => own.filter((d) => d.status !== "generating" && (lastLeft != null ? d.createdAt > lastLeft : now - d.createdAt < NEW_WINDOW)).length,
    [own, lastLeft, now],
  );
  const counts = useMemo(
    () => ({
      starred: designs.filter((d) => d.starred).length,
      promoted: designs.filter((d) => promotedIds.has(d.id)).length,
      generating: designs.filter((d) => d.status === "generating").length,
    }),
    [designs, promotedIds],
  );
  if (!space) return null;
  const enter = () => enterSpace(space.id);

  return (
    // Double-click anywhere on the card opens the project or exploration.
    <div
      style={{ width: PORTAL_W }}
      className="group/portal relative"
      role="group"
      aria-label={`${project ? "Project" : "Exploration"} ${space.name}, double-click to open`}
      onDoubleClick={(e) => (e.stopPropagation(), enter())}
    >
      <div
        className={cn(
          "relative rounded-xl border bg-card text-card-foreground shadow-sm transition-shadow",
          selected && "ring-2 ring-primary shadow-lg",
        )}
      >
        <div className="flex items-center gap-2 py-2.5 pr-2.5 pl-3">
          <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
            {project ? <FolderGit2 className="size-3.5" /> : <Layers className="size-3.5" />}
          </span>
          <span className="flex min-w-0 flex-col">
            <span className="truncate text-sm font-medium" title={space.name}>
              {space.name}
            </span>
            {localPath && (
              <span className="truncate font-mono text-[11px] leading-4 text-muted-foreground" title={localPath}>
                {tildePath(localPath)}
              </span>
            )}
          </span>
          {counts.generating > 0 && (
            <span className="relative size-1.5 shrink-0 rounded-full bg-blue-500" title="Generating" aria-label="Generating">
              <span className="absolute -inset-1 animate-ping rounded-full border border-blue-500 opacity-60 motion-reduce:animate-none" />
            </span>
          )}
          {newCount > 0 && (
            <span className="shrink-0 rounded-full bg-blue-600/10 px-1.5 py-0.5 text-[11px] font-medium text-blue-700 tabular-nums dark:bg-blue-400/15 dark:text-blue-300">
              {newCount} new
            </span>
          )}
          <Button
            size="icon-sm"
            variant="ghost"
            className="nodrag ml-auto shrink-0 text-muted-foreground opacity-60 transition-opacity group-hover/portal:opacity-100 focus-visible:opacity-100"
            aria-label={project ? "Delete project" : "Delete exploration"}
            onClick={() => askDelete(space.id)}
          >
            <Trash2 className="size-4" />
          </Button>
        </div>

        <Hero pool={project ? designs : own} heroRefs={heroRefs} project={project} />

        <div className="flex items-center justify-between gap-2 py-2.5 pr-2.5 pl-3.5">
          <div className="flex items-center gap-2.5 text-xs text-muted-foreground tabular-nums">
            <span className="flex items-center gap-1" title="Designs">
              <LayoutGrid className="size-3 opacity-80" />
              <b className="font-medium text-foreground">{designs.length}</b>
            </span>
            {counts.starred > 0 && (
              <span className="flex items-center gap-1" title="Starred">
                <Star className="size-3 opacity-80" />
                <b className="font-medium text-foreground">{counts.starred}</b>
              </span>
            )}
            {counts.promoted > 0 && (
              <span className="flex items-center gap-1" title="Promoted">
                <ArrowUpFromLine className="size-3 opacity-80" />
                <b className="font-medium text-foreground">{counts.promoted}</b>
              </span>
            )}
            {subSpaces > 0 && (
              <span className="flex items-center gap-1" title="Nested explorations">
                <Layers className="size-3 opacity-80" />
                <b className="font-medium text-foreground">{subSpaces}</b>
              </span>
            )}
            <span className="sr-only">
              {designs.length} {designs.length === 1 ? "design" : "designs"}
              {counts.starred > 0 && `, ${counts.starred} starred`}
              {counts.promoted > 0 && `, ${counts.promoted} promoted`}
              {subSpaces > 0 && `, ${subSpaces} nested`}
            </span>
          </div>
          {counts.generating > 0 && (
            <span className="rounded-full bg-muted px-2 py-0.5 text-[11px] font-medium text-blue-600 tabular-nums dark:text-blue-400">
              {counts.generating} generating
            </span>
          )}
        </div>
      </div>
    </div>
  );
}

export const PortalNode = memo(PortalNodeImpl);
