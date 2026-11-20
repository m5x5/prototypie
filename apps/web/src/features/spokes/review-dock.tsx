"use client";
import { useState } from "react";
import { Inbox, Loader2, Star } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { DockCount, DockEntry, DockHead } from "@/components/activity-dock";
import { Button } from "@/components/ui/button";
import { useApp } from "@/store";
import { laneOf, useSpokesUI } from "./lib";
import { useQueueIds } from "./review";

const SHOWN = 5;

/** "Review" in the header's activity dock: waiting/generating at a glance, the queue's head in an anchored panel. */
export function ReviewDock() {
  const openReview = useSpokesUI((s) => s.openReview);
  const queue = useQueueIds();
  const running = useApp((s) => Object.values(s.jobs).filter((j) => j.status === "running").length);
  const [open, setOpen] = useState(false);
  const queued = queue.length;

  const waiting = queued ? `${queued} waiting` : "Nothing waiting";
  const generating = running ? `${running} generating` : null;
  const start = (id?: string) => {
    setOpen(false);
    openReview(id ?? null);
  };

  return (
    <DockEntry
      icon={<Inbox />}
      name="Review"
      status={
        <>
          {waiting}
          {generating && <span className="text-blue-600 dark:text-blue-400"> · {generating}</span>}
        </>
      }
      compact={
        <span className="flex items-center gap-1">
          {queued > 0 && <DockCount n={queued} />}
          {running > 0 && <span className="size-2 rounded-full bg-blue-500 motion-safe:animate-pulse" />}
        </span>
      }
      label={`Review: ${waiting}${generating ? `, ${generating}` : ""}`}
      highlight={queued > 0}
      open={open}
      onOpenChange={setOpen}
      panelLabel="Review queue"
    >
      <DockHead title="Review queue" count={waiting} />
      {running > 0 && (
        <p className="flex items-center gap-2 border-b px-3.5 py-2 text-xs text-blue-700 dark:text-blue-300">
          <Loader2 className="size-3.5 motion-safe:animate-spin" aria-hidden />
          {running} lane{running === 1 ? "" : "s"} generating
        </p>
      )}
      {queued > 0 ? (
        <ol className="min-h-0 overflow-y-auto p-1.5" aria-label="Next in the queue">
          {queue.slice(0, SHOWN).map((id, i) => (
            <QueueRow key={id} id={id} index={i + 1} onOpen={() => start(id)} />
          ))}
          {queued > SHOWN && <li className="px-2 py-1 text-xs md:text-[11px] text-muted-foreground">+{queued - SHOWN} more</li>}
        </ol>
      ) : (
        <p className="px-3.5 py-6 text-center text-sm text-muted-foreground">Every ready design has been reviewed.</p>
      )}
      <div className="border-t bg-muted/30 p-2.5">
        <Button size="sm" className="w-full" disabled={!queued} onClick={() => start()}>
          Start reviewing
        </Button>
      </div>
    </DockEntry>
  );
}

function QueueRow({ id, index, onOpen }: { id: string; index: number; onOpen: () => void }) {
  const { title, starred, kind, lane } = useApp(
    useShallow((s) => {
      const d = s.designs[id];
      const job = Object.values(s.jobs).find((j) => j.designIds.includes(id));
      return { title: d?.title ?? "", starred: !!d?.starred, kind: d?.kind ?? "", lane: laneOf(s, id)?.name ?? job?.label };
    }),
  );
  return (
    <li>
      <button
        type="button"
        onClick={onOpen}
        className="flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left hover:bg-muted focus-visible:bg-muted focus-visible:outline-none"
      >
        <span className="mt-0.5 w-4 shrink-0 text-right text-xs md:text-[10px] tabular-nums text-muted-foreground">{index}</span>
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-1 text-xs font-medium">
            {starred && <Star className="size-3 shrink-0 fill-amber-400 text-amber-400" aria-label="Starred" />}
            <span className="truncate">{title}</span>
          </span>
          <span className="block truncate text-xs md:text-[11px] text-muted-foreground">{lane ?? kind}</span>
        </span>
      </button>
    </li>
  );
}
