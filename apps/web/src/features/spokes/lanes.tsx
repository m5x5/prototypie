"use client";
import { useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { useReactFlow } from "@xyflow/react";
import { Boxes, Check, CircleSlash, Inbox, Loader2, RefreshCw, Shapes, Square, Wand2, Merge } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { useApp } from "@/store";
import { cancelJob, type Job } from "@/store/core";
import type { LaneMeta } from "./slice";
import { laneProgress, useSpokesUI } from "./lib";
import { framePadding } from "@/lib/canvas-insets";

const TYPE_ICON: Record<LaneMeta["type"], typeof Boxes> = {
  clay: Wand2,
  spoke: Boxes,
  round: RefreshCw,
  merge: Merge,
};

/** Every running and finished exploration lane (core jobs), with progress and cancel. */
export function LanesPanel() {
  const jobs = useApp(
    useShallow((s) =>
      Object.values(s.jobs)
        .sort((a, b) => Number(b.status === "running") - Number(a.status === "running") || b.startedAt - a.startedAt)
        .slice(0, 30),
    ),
  );
  const [showArchived, setShowArchived] = useState(false);
  if (!jobs.length) return null;
  const running = jobs.filter((j) => j.status === "running").length;
  // Only the 3 most recent lanes (plus anything still running) stay visible; older ones are archived.
  const recent = jobs.filter((j, i) => i < 3 || j.status === "running");
  const archived = jobs.filter((j) => !recent.includes(j));
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <Label>Exploration lanes</Label>
        <span className="text-xs md:text-[11px] tabular-nums text-muted-foreground">
          {running ? `${running} running` : "all settled"}
        </span>
      </div>
      <div className="space-y-1.5">
        {(showArchived ? jobs : recent).map((j) => (
          <LaneRow key={j.id} job={j} />
        ))}
      </div>
      {archived.length > 0 && (
        <button
          type="button"
          className="text-xs md:text-[11px] font-medium text-muted-foreground hover:text-foreground"
          onClick={() => setShowArchived((v) => !v)}
        >
          {showArchived ? "Hide archived" : `Show ${archived.length} archived`}
        </button>
      )}
    </div>
  );
}

function LaneRow({ job }: { job: Job }) {
  const lane = useApp((s) => s.lanes[job.id]) as LaneMeta | undefined;
  const progress = useApp(useShallow((s) => laneProgress(s, job.designIds, lane?.count ?? 0)));
  const select = useApp((s) => s.select);
  const activeSpaceId = useApp((s) => s.activeSpaceId);
  const setActiveSpace = useApp((s) => s.setActiveSpace);
  const openReview = useSpokesUI((s) => s.openReview);
  const firstReady = useApp((s) => job.designIds.find((id) => s.designs[id]?.status === "ready"));
  const flow = useReactFlow();
  const Icon = lane ? TYPE_ICON[lane.type] : Shapes;
  const total = progress.total;
  const pct = total ? ((progress.ready + progress.failed) / total) * 100 : 0;

  const reveal = () => {
    const ids = job.designIds.filter((id) => useApp.getState().designs[id]);
    if (!ids.length) return;
    if (job.spaceId !== activeSpaceId) setActiveSpace(job.spaceId);
    select(ids[0]);
    setTimeout(() => void flow.fitView({ nodes: ids.map((id) => ({ id })), duration: 400, padding: framePadding(0.25), maxZoom: 1 }), 40);
  };

  return (
    <div className="group rounded-lg border bg-card px-2.5 py-2">
      <div className="flex items-center gap-2">
        <Icon className="size-3.5 shrink-0 text-muted-foreground" />
        <button
          type="button"
          onClick={reveal}
          className="min-w-0 flex-1 truncate text-left text-xs font-medium hover:underline"
          title={lane?.focus ?? job.label}
        >
          {lane?.name ?? (job.label || "Variations")}
        </button>
        <StatusChip job={job} ready={progress.ready} total={total} failed={progress.failed} />
        {job.status === "running" ? (
          <Button
            size="icon-xs"
            variant="ghost"
            aria-label="Cancel lane"
            title="Cancel"
            onClick={() => cancelJob(job.id)}
          >
            <Square className="size-2.5 fill-current" />
          </Button>
        ) : firstReady ? (
          <Button
            size="icon-xs"
            variant="ghost"
            aria-label="Review this lane"
            title="Review"
            onClick={() => openReview(firstReady)}
          >
            <Inbox />
          </Button>
        ) : null}
      </div>
      <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-muted">
        <div
          className={cn(
            "h-full rounded-full transition-all duration-500",
            job.status === "error" ? "bg-destructive" : job.status === "done" ? "bg-emerald-500" : "bg-blue-500",
            job.status === "running" && !progress.ready && "animate-pulse",
          )}
          style={{ width: `${job.status === "running" ? Math.max(pct, 6) : 100}%` }}
        />
      </div>
      {job.status === "error" && job.error && (
        <p className="mt-1.5 line-clamp-2 text-xs md:text-[11px] text-destructive" title={job.error}>
          {job.error}
        </p>
      )}
    </div>
  );
}

function StatusChip({ job, ready, total, failed }: { job: Job; ready: number; total: number; failed: number }) {
  const text = total ? `${ready}/${total}` : "planning";
  if (job.status === "running") {
    return (
      <span className="flex items-center gap-1 text-xs md:text-[11px] tabular-nums text-muted-foreground">
        <Loader2 className="size-3 animate-spin" /> {text}
      </span>
    );
  }
  if (job.status === "error" && !ready) {
    return (
      <span className="flex items-center gap-1 text-xs md:text-[11px] text-destructive">
        <CircleSlash className="size-3" /> {job.error?.toLowerCase().includes("abort") ? "cancelled" : "failed"}
      </span>
    );
  }
  return (
    <span className={cn("flex items-center gap-1 text-xs md:text-[11px] tabular-nums", failed ? "text-amber-600" : "text-emerald-600")}>
      <Check className="size-3" /> {text}
    </span>
  );
}
