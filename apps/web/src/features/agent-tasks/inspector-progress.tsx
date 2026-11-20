"use client";
import { useId, useState } from "react";
import { Check, PanelTopOpen } from "lucide-react";
import { ClaudeMinion } from "@/components/agent-icons";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";
import type { AgentTask, TaskStatus } from "@/lib/server/agent-tasks";
import type { Design } from "@/lib/types";
import { openTaskPanel, tasksForDesign, useAgentTasks, useLinkedDesignIds } from "./use-agent-tasks";

/**
 * How far the agent is with the selected card: shown in the Inspector when the card was sent to an agent
 * (Implement → "[prototypie implement <id>]" note) or is the placeholder of a queue item; a promoted copy shares
 * its source's tasks (and vice versa). Same data and poll as the
 * header panel (useAgentTasks); nothing renders for a card without a task.
 */

const STATUS: Record<TaskStatus, string> = {
  waiting: "Wartet auf einen Agenten",
  working: "In Arbeit",
  delivered: "Geliefert",
  resolved: "Abgeschlossen",
  cancelled: "Abgebrochen",
};

function when(at: string | null) {
  if (!at) return "";
  const d = new Date(at);
  if (Number.isNaN(d.getTime())) return "";
  const time = d.toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
  return d.toDateString() === new Date().toDateString() ? time : `${d.toLocaleDateString("de-DE", { day: "2-digit", month: "2-digit" })} ${time}`;
}

export function AgentProgress({ design }: { design: Design | null }) {
  const { data } = useAgentTasks();
  const tasks = tasksForDesign(data, useLinkedDesignIds(design?.id));
  if (!design || !tasks.length) return null;
  const working = tasks.some((t) => t.status === "working");
  return (
    <>
      <Separator />
      <section className="space-y-2" aria-label="Fortschritt beim Agenten">
        <Label className="flex items-center gap-1.5">
          <ClaudeMinion working={working} className="h-3 w-4" />
          Beim Agenten
          {tasks.length > 1 && <span className="ml-auto text-xs font-normal text-muted-foreground tabular-nums">{tasks.length} Aufgaben</span>}
        </Label>
        <ul className="space-y-2">
          {tasks.slice(0, 4).map((t) => (
            <TaskProgress key={t.id} task={t} />
          ))}
        </ul>
      </section>
    </>
  );
}

function TaskProgress({ task }: { task: AgentTask }) {
  const [more, setMore] = useState(false);
  const summaryId = useId();
  const steps = task.steps.filter((s) => s.key !== "announced");
  const current = [...steps].reverse().find((s) => s.done);
  const kind = task.kind === "implement" ? "Implement" : task.jobId ? `Job ${task.jobId}#${task.index}` : "Notiz";
  const summary = task.summary?.replace(/^Resolved:\s*/, "");
  const long = !!summary && summary.length > 160;
  return (
    <li className="rounded-lg border bg-muted/30 p-2.5 text-xs">
      <div className="flex items-start gap-2">
        <span className="min-w-0 flex-1">
          <span className="block font-medium">{STATUS[task.status]}</span>
          <span className="block truncate text-[11px] text-muted-foreground" title={task.title}>
            {kind}
            {current?.at ? ` · ${current.label} ${when(current.at)}` : ""}
          </span>
        </span>
        {task.status === "working" && <ClaudeMinion working className="mt-0.5 h-3 w-4 shrink-0" />}
      </div>

      <ol className="mt-2 flex items-center" aria-label="Schritte">
        {steps.map((s, i) => (
          <li key={s.key} className={cn("flex min-w-0 items-center", i > 0 && "flex-1")}>
            {i > 0 && <span className={cn("mx-1 h-px flex-1", s.done ? "bg-foreground/50" : "bg-border")} aria-hidden />}
            <span
              className={cn(
                "grid size-4 shrink-0 place-items-center rounded-full border",
                s.done ? "border-foreground bg-foreground text-background" : "border-dashed border-muted-foreground/60",
                s.key === "cancelled" && "border-destructive bg-destructive",
                s === current && task.status === "working" && "border-[#D97757] bg-[#D97757]",
              )}
              title={`${s.label}${s.at ? ` · ${new Date(s.at).toLocaleString("de-DE")}` : s.done ? " · Zeit nicht erfasst" : " · offen"}`}
            >
              {s.done && <Check className="size-2.5" aria-hidden />}
            </span>
            <span className="sr-only">
              {s.label}: {s.done ? `erledigt${s.at ? ` ${when(s.at)}` : ""}` : "offen"}
            </span>
          </li>
        ))}
      </ol>
      <div className="mt-1 flex justify-between text-[10px] text-muted-foreground" aria-hidden>
        {steps.map((s) => (
          <span key={s.key} className={cn("truncate", s.done && "text-foreground")}>
            {s.label}
          </span>
        ))}
      </div>

      {summary && (
        <div className="mt-2">
          <p id={summaryId} className={cn("whitespace-pre-wrap break-words text-muted-foreground", !more && "line-clamp-3")}>
            {summary}
          </p>
          {long && (
            <button
              type="button"
              className="mt-0.5 text-[11px] font-medium text-foreground underline-offset-2 hover:underline"
              aria-expanded={more}
              aria-controls={summaryId}
              onClick={() => setMore((m) => !m)}
            >
              {more ? "Weniger" : "Mehr"}
            </button>
          )}
        </div>
      )}

      <Button size="xs" variant="ghost" className="mt-1.5 -ml-2" onClick={() => openTaskPanel(task.id)}>
        <PanelTopOpen /> Im Aufgabenpanel öffnen
      </Button>
    </li>
  );
}
