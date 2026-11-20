"use client";
import { useRef, useState } from "react";
import { Check, ChevronDown, Code2, Loader2, RotateCcw, Send } from "lucide-react";
import { ClaudeMinion } from "@/components/agent-icons";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { AgentTask } from "@/lib/server/agent-tasks";
import type { Design } from "@/lib/types";
import { useApp } from "@/store";
import { openTaskPanel, reloadAgentTasks, tasksForDesign, useAgentTasks, useLinkedDesignIds } from "./use-agent-tasks";

/**
 * "Implement" on a promoted design, showing the real state of its Implement request (shared with its promoted
 * source/copies): Gesendet → In Arbeit → Erledigt. While a request is open or done, a click opens the task panel
 * instead of sending again; sending again after "Erledigt" is a separate, confirmed menu item (force).
 */

type Phase = "idle" | "sent" | "working" | "done";

const LABEL: Record<Phase, string> = { idle: "Implement", sent: "Gesendet", working: "In Arbeit", done: "Erledigt" };

function phaseOf(task: AgentTask | undefined): Phase {
  if (!task || task.status === "cancelled") return "idle";
  if (task.status === "resolved" || task.status === "delivered") return "done";
  return task.status === "working" ? "working" : "sent";
}

export function ImplementButton({ design }: { design: Design }) {
  const { data } = useAgentTasks();
  const ids = useLinkedDesignIds(design.id);
  const updateDesign = useApp((s) => s.updateDesign);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
  const busy = useRef(false);

  // Newest Implement request for this design or a linked one; an open request wins over an older resolved one.
  const requests = tasksForDesign(data, ids)
    .filter((t) => t.kind === "implement" && t.status !== "cancelled")
    .sort((a, b) => b.createdAt.localeCompare(a.createdAt));
  const task = requests.find((t) => t.status === "waiting" || t.status === "working") ?? requests[0];
  let phase = phaseOf(task);
  // Before the first poll (or without Agentation), a request this browser just made still counts as sent.
  if (phase === "idle" && !data?.agentation && design.implementRequestedAt) phase = "sent";

  const send = async (force = false) => {
    if (busy.current) return;
    busy.current = true;
    setSending(true);
    setError(null);
    try {
      const res = await fetch("/api/implement", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ designId: design.id, title: design.title, rationale: design.rationale, html: design.html, force }),
      });
      const body = (await res.json().catch(() => ({}))) as { error?: string; done?: boolean; duplicate?: boolean };
      // Already open or already done elsewhere (another tab, the promoted source): just show that state.
      if (!res.ok && !body.done) throw new Error(body.error ?? `Request failed (${res.status})`);
      if (res.ok && !body.duplicate) updateDesign(design.id, { implementRequestedAt: Date.now() });
      await reloadAgentTasks();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      busy.current = false;
      setSending(false);
    }
  };

  const onClick = () => {
    if (task && phase !== "idle") openTaskPanel(task.id);
    else void send();
  };

  const tip = error
    ? error
    : phase === "idle"
      ? "Hand this design to the coding agent to build it into the product"
      : phase === "done"
        ? "Vom Agenten erledigt · Aufgabe im Panel öffnen"
        : phase === "working"
          ? "Der Agent arbeitet daran · Aufgabe im Panel öffnen"
          : task
            ? "Wartet auf einen Agenten · Aufgabe im Panel öffnen"
            : "An den Agenten gesendet";

  return (
    <>
      <span className="inline-flex items-center">
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                size="sm"
                variant={phase === "idle" ? "brand" : "ghost"}
                className={phase === "done" ? "rounded-r-none pr-1.5" : undefined}
                disabled={sending}
                aria-busy={sending || undefined}
                onClick={onClick}
              />
            }
          >
            {sending ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : phase === "working" ? (
              <ClaudeMinion working className="h-3 w-4" />
            ) : phase === "done" ? (
              <Check className="size-3.5" />
            ) : phase === "sent" ? (
              <Send className="size-3.5" />
            ) : (
              <Code2 className="size-3.5" />
            )}
            {LABEL[phase]}
          </TooltipTrigger>
          <TooltipContent>{tip}</TooltipContent>
        </Tooltip>
        {phase === "done" && (
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button size="sm" variant="ghost" className="rounded-l-none px-1" aria-label="Weitere Aktionen zu Implement" disabled={sending} />
              }
            >
              <ChevronDown className="size-3.5" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-auto">
              <DropdownMenuItem onClick={() => setConfirm(true)}>
                <RotateCcw /> Erneut senden …
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </span>

      <Dialog open={confirm} onOpenChange={setConfirm}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Erneut an den Agenten senden?</DialogTitle>
            <DialogDescription>
              Der Agent hat „{design.title}“ schon umgesetzt. Ein neuer Auftrag lässt ihn das Design noch einmal einbauen.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <DialogClose render={<Button variant="outline" />}>Abbrechen</DialogClose>
            <Button
              disabled={sending}
              onClick={() => {
                setConfirm(false);
                void send(true);
              }}
            >
              Erneut senden
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
