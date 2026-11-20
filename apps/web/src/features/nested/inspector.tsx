"use client";
import { useMemo } from "react";
import { ArrowRight, ArrowUpFromLine, Database, FolderGit2, Info, Layers, Star } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useApp } from "@/store";
import type { Design } from "@/lib/types";
import { ROOT_SPACE_ID } from "@/store/core";
import { canPromoteTo, descendantSpaceIds, findPromotedCopy } from "./slice";
import { selectSubtreeDesigns } from "./portal-node";
import { tildePath, usePodRecords, useSpaceLocalPaths, type PodRecord } from "./local-project";
import { enterSpace, promote, showPromotedCopy } from "./nav";
import { AgentProgress } from "@/features/agent-tasks/inspector-progress";

export function InspectorImpl({ design }: { design: Design | null }) {
  const space = useApp((s) => s.spaces[s.activeSpaceId]);
  const parent = useApp((s) => (space?.parentSpaceId ? s.spaces[space.parentSpaceId] : undefined));
  const designs = useApp(useShallow((s) => Object.values(s.designs).filter((d) => d.spaceId === s.activeSpaceId)));
  const promotedFrom = useApp((s) => s.promotedFrom);
  const allDesigns = useApp((s) => s.designs);
  const nestedCount = useApp((s) => descendantSpaceIds(s, s.activeSpaceId).length);

  const stats = useMemo(() => {
    const promotedIds = new Set(
      Object.entries(promotedFrom)
        .filter(([copyId]) => allDesigns[copyId]?.spaceId === parent?.id)
        .map(([, r]) => r.designId),
    );
    const starred = designs.filter((d) => d.starred);
    return {
      total: designs.length,
      starred: starred.length,
      promoted: designs.filter((d) => promotedIds.has(d.id)).length,
      toPromote: starred.filter((d) => d.status === "ready" && !promotedIds.has(d.id)).map((d) => d.id),
    };
  }, [designs, promotedFrom, allDesigns, parent?.id]);

  if (!space) return null;
  return (
    <>
      <AgentProgress design={design} />
      {/* Projects sit on the main canvas, which takes no promoted designs: these controls are for explorations. */}
      {parent && canPromoteTo(parent.id) && (
        <>
          <Separator />
          <section className="space-y-3">
            <div className="flex items-center gap-1.5">
              <Label className="flex items-center gap-1.5">
                <Layers className="size-3.5 text-primary" /> Exploration
              </Label>
              <Tooltip>
                <TooltipTrigger
                  render={
                    <button
                      type="button"
                      aria-label="About explorations"
                      className="ml-auto text-muted-foreground hover:text-foreground"
                    />
                  }
                >
                  <Info className="size-3.5" />
                </TooltipTrigger>
                <TooltipContent className="max-w-60">
                  Make a mess here. Promote the strands worth keeping up to {parent.name}.
                </TooltipContent>
              </Tooltip>
            </div>

            <div className="grid grid-cols-4 gap-1.5">
              <Stat label="Designs" value={stats.total} />
              <Stat label="Starred" value={stats.starred} />
              <Stat label="Promoted" value={stats.promoted} />
              <Stat label="Nested" value={nestedCount} />
            </div>

            {design && design.spaceId === space.id && <PromoteSelected design={design} parentName={parent.name} />}

            <Button
              variant="secondary"
              className="w-full"
              disabled={!stats.toPromote.length}
              onClick={() => promote(stats.toPromote)}
            >
              <Star className="size-3.5" />
              Promote starred
              {stats.toPromote.length > 0 && (
                <span className="ml-auto text-xs tabular-nums opacity-70">{stats.toPromote.length}</span>
              )}
            </Button>
            {!stats.starred && (
              <p className="text-xs text-muted-foreground">Star designs on the canvas to promote them in bulk.</p>
            )}
          </section>
        </>
      )}
      <ChildSpaces spaceId={space.id} />
    </>
  );
}

function PromoteSelected({ design, parentName }: { design: Design; parentName: string }) {
  const copy = useApp((s) => findPromotedCopy(s, design.id));
  if (copy) {
    return (
      <Button variant="outline" className="w-full" onClick={() => showPromotedCopy(design.id)}>
        <ArrowUpFromLine className="size-3.5 text-primary" />
        <span className="truncate">Promoted · show in {parentName}</span>
      </Button>
    );
  }
  return (
    <Button className="w-full" disabled={design.status !== "ready"} onClick={() => promote([design.id])}>
      <ArrowUpFromLine className="size-3.5" />
      <span className="truncate">Promote “{design.title}”</span>
    </Button>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-lg border bg-muted/30 px-2 py-1.5">
      <div className="text-sm font-medium tabular-nums">{value}</div>
      <div className="text-[10px] text-muted-foreground">{label}</div>
    </div>
  );
}

/**
 * Spaces nested directly inside the current one, for quick navigation. On the main canvas these are the projects
 * (each row with its folder and pod link); elsewhere explorations, grouped by project only when they span several.
 */
function ChildSpaces({ spaceId }: { spaceId: string }) {
  const children = useApp(
    useShallow((s) =>
      Object.values(s.spaces)
        .filter((sp) => sp.parentSpaceId === spaceId)
        .sort((a, b) => a.createdAt - b.createdAt),
    ),
  );
  const counts = useApp(
    useShallow((s) => children.map((c) => selectSubtreeDesigns(s, c.id).length)),
  );
  const paths = useSpaceLocalPaths(children.map((c) => c.id));
  const atRoot = spaceId === ROOT_SPACE_ID;
  // One group per project folder (in order of first appearance), then the spaces without one.
  const groups = useMemo(() => {
    const byKey = new Map<string, { path: string; rows: number[] }>();
    const rest: number[] = [];
    paths.forEach((p, i) => {
      if (!p) return void rest.push(i);
      const k = p.toLowerCase();
      const g = byKey.get(k) ?? { path: p, rows: [] };
      g.rows.push(i);
      byKey.set(k, g);
    });
    return { projects: [...byKey.values()], rest };
  }, [paths]);
  const grouped = !atRoot && groups.projects.length > 1;
  const pods = usePodRecords(atRoot ? paths : groups.projects.map((g) => g.path));
  if (!children.length) return null;
  const row = (i: number) => (
    <ChildSpaceRow
      key={children[i].id}
      id={children[i].id}
      name={children[i].name}
      count={counts[i]}
      project={atRoot ? { path: paths[i], pod: pods[i] } : undefined}
    />
  );
  return (
    <>
      <Separator />
      <section className="space-y-1.5">
        <Label>{atRoot ? "Projects" : "Explorations here"}</Label>
        {grouped ? (
          <>
            {groups.projects.map((g, gi) => {
              const name = folderName(g.path);
              return (
                <div key={g.path} role="group" aria-label={`Project ${name}`} className="space-y-0.5">
                  <div className="flex min-w-0 items-center gap-1.5 px-2 pt-1 text-[11px] text-muted-foreground">
                    <FolderGit2 className="size-3 shrink-0" />
                    <span className="shrink-0 font-medium">{name}</span>
                    <span className="truncate font-mono" title={g.path}>
                      {tildePath(g.path)}
                    </span>
                    <PodLink pod={pods[gi]} name={name} className="ml-auto" />
                  </div>
                  {g.rows.map(row)}
                </div>
              );
            })}
            {groups.rest.length > 0 && (
              <div role="group" aria-label="Other explorations" className="space-y-0.5">
                <div className="px-2 pt-1 text-[11px] font-medium text-muted-foreground">Other</div>
                {groups.rest.map(row)}
              </div>
            )}
          </>
        ) : (
          children.map((_, i) => row(i))
        )}
      </section>
    </>
  );
}

const folderName = (p: string) => p.split("/").filter(Boolean).pop() ?? p;

function PodLink({ pod, name, className }: { pod?: PodRecord; name: string; className?: string }) {
  if (!pod) return null;
  return (
    <a
      href={pod.doc}
      target="_blank"
      rel="noreferrer"
      className={`shrink-0 rounded p-0.5 text-muted-foreground hover:bg-muted hover:text-foreground ${className ?? ""}`}
      title={`${pod.subject} in ${pod.doc}`}
      aria-label={`Open ${name} on the Solid pod`}
    >
      <Database className="size-3" />
    </a>
  );
}

function ChildSpaceRow({
  id,
  name,
  count,
  project,
}: {
  id: string;
  name: string;
  count: number;
  /** On the main canvas: the project's folder and pod record, shown on the row itself. */
  project?: { path?: string; pod?: PodRecord };
}) {
  const path = project?.path;
  return (
    <div className="group flex items-center rounded-md hover:bg-muted">
      <button type="button" onClick={() => enterSpace(id)} className="flex min-w-0 flex-1 items-center gap-2 px-2 py-1.5 text-left text-xs">
        {project ? (
          <FolderGit2 className="size-3.5 shrink-0 text-muted-foreground" />
        ) : (
          <Layers className="size-3.5 shrink-0 text-muted-foreground" />
        )}
        <span className="flex min-w-0 flex-col">
          <span className="truncate">{name}</span>
          {path && (
            <span className="truncate font-mono text-[11px] leading-4 text-muted-foreground" title={path}>
              {tildePath(path)}
            </span>
          )}
        </span>
        <span className="ml-auto shrink-0 tabular-nums text-muted-foreground">{count}</span>
        <ArrowRight className="size-3 shrink-0 opacity-0 transition-opacity group-hover:opacity-100" />
      </button>
      {project?.pod && <PodLink pod={project.pod} name={name} className="mr-1.5" />}
    </div>
  );
}
