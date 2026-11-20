"use client";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import {
  ArrowUpRight,
  Check,
  Circle,
  Loader2,
  MessageSquarePlus,
  X,
} from "lucide-react";
import { toast } from "sonner";
import { ClaudeMinion } from "@/components/agent-icons";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { enterSpace } from "@/features/nested/nav";
import { useDraft } from "@/store/use-draft";
import { cn } from "@/lib/utils";
import type { AgentTask, TaskStatus } from "@/lib/server/agent-tasks";
import {
  reloadAgentTasks,
  setTaskPanel,
  useAgentTasks,
  type AgentTasksData,
} from "./use-agent-tasks";

/**
 * Header element in the style of an agent pill: Claude's minion (scuttling only while an agent really
 * works) with one slot per agent session in this repo, and a panel with the agents' tasks, their steps and the
 * Agentation thread, where a note can be added. Data: GET /api/agent-tasks (Rex + agent queue + Agentation).
 */

type Data = AgentTasksData;

const STATUS: Record<TaskStatus, { label: string; dot: string }> = {
  working: { label: "In Arbeit", dot: "bg-amber-500" },
  waiting: { label: "Wartet", dot: "bg-sky-500" },
  delivered: { label: "Geliefert", dot: "bg-emerald-500" },
  resolved: { label: "Abgeschlossen", dot: "bg-muted-foreground/50" },
  cancelled: { label: "Abgebrochen", dot: "bg-destructive/70" },
};
const ORDER: TaskStatus[] = [
  "working",
  "waiting",
  "delivered",
  "resolved",
  "cancelled",
];
const SHOWN_PER_GROUP: Partial<Record<TaskStatus, number>> = {
  resolved: 8,
  cancelled: 3,
};

/** "14:05" today, "31.10. 14:05" before. */
function when(at: string | null) {
  if (!at) return "";
  const d = new Date(at);
  if (Number.isNaN(d.getTime())) return "";
  const time = d.toLocaleTimeString("de-DE", {
    hour: "2-digit",
    minute: "2-digit",
  });
  return d.toDateString() === new Date().toDateString()
    ? time
    : `${d.toLocaleDateString("de-DE", {
        day: "2-digit",
        month: "2-digit",
      })} ${time}`;
}

export function AgentCapacityButton() {
  const { data, failed, panel } = useAgentTasks();
  const open = panel.open;
  const buttonRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const panelId = useId();

  const cap = data?.capacity;
  const known = cap?.total != null && cap.working != null;
  const working = known ? cap!.working! : 0;
  const total = known ? cap!.total! : 0;
  const active = (data?.tasks ?? []).filter(
    (t) => t.status === "working" || t.status === "waiting"
  ).length;
  const label = known
    ? `Agenten: ${working} von ${total} arbeiten · ${active} offene Aufgabe${
        active === 1 ? "" : "n"
      }`
    : failed
    ? "Agenten-Status nicht erreichbar"
    : "Agenten-Status wird geladen";

  const close = useCallback((focusBack = true) => {
    setTaskPanel(false);
    if (focusBack) buttonRef.current?.focus();
  }, []);

  // Escape and a click outside close the panel; focus goes back to the button.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      }
    };
    const onDown = (e: PointerEvent) => {
      const t = e.target as Node;
      if (panelRef.current?.contains(t) || buttonRef.current?.contains(t))
        return;
      close(false);
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("pointerdown", onDown, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("pointerdown", onDown, true);
    };
  }, [open, close]);

  const slots = known ? Math.min(total, 12) : 0;
  return (
    <div className="relative">
      <button
        ref={buttonRef}
        type="button"
        className={cn(
          "flex h-8 max-md:h-10 items-center gap-2 max-md:gap-1.5 rounded-full border px-2.5 text-xs font-medium transition-colors hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          open && "bg-accent"
        )}
        aria-label={label}
        title={label}
        aria-expanded={open}
        aria-controls={panelId}
        aria-haspopup="dialog"
        onClick={() => setTaskPanel(!open)}
      >
        <ClaudeMinion working={working > 0} className="h-3 w-4" />
        {known ? (
          <>
            <span className="tabular-nums xl:hidden" aria-hidden>
              {working}/{total}
            </span>
            <span className="hidden items-center gap-[3px] xl:flex" aria-hidden>
              {Array.from({ length: slots }, (_, i) => (
                <i
                  key={i}
                  className={cn(
                    "block size-1.5 rounded-[2px] border border-foreground/60",
                    i < working && "border-[#D97757] bg-[#D97757]"
                  )}
                />
              ))}
              {total > slots && (
                <span className="ml-0.5 text-muted-foreground">
                  +{total - slots}
                </span>
              )}
            </span>
          </>
        ) : (
          <span className="text-muted-foreground" aria-hidden>
            {failed ? "–" : "…"}
          </span>
        )}
        {active > 0 && (
          <span
            className="grid h-4 min-w-4 place-items-center rounded-full bg-foreground px-1 text-[10px] leading-none font-semibold text-background"
            aria-hidden
          >
            {active}
          </span>
        )}
      </button>
      {open && (
        <div
          ref={panelRef}
          id={panelId}
          role="dialog"
          aria-label="Agenten und Aufgaben"
          className="absolute top-[calc(100%+8px)] right-0 max-md:fixed max-md:inset-x-2 max-md:top-14 max-md:right-2 max-md:!w-auto max-md:max-h-[calc(100dvh-4.5rem)] z-50 flex max-h-[min(78vh,720px)] w-[min(440px,calc(100vw-24px))] flex-col overflow-hidden rounded-xl border bg-popover text-popover-foreground shadow-xl"
        >
          <PanelHead data={data} failed={failed} onClose={() => close()} />
          <TaskList
            data={data}
            focusId={panel.taskId}
            focusTick={panel.tick}
            onJump={() => close(false)}
            onThread={() => void reloadAgentTasks()}
          />
        </div>
      )}
    </div>
  );
}

function PanelHead({
  data,
  failed,
  onClose,
}: {
  data: Data | null;
  failed: boolean;
  onClose: () => void;
}) {
  const cap = data?.capacity;
  const known = cap?.total != null && cap.working != null;
  return (
    <div className="border-b px-4 pt-3 pb-3">
      <div className="flex items-center gap-2">
        <ClaudeMinion working={!!cap?.working} className="h-3.5 w-[18px]" />
        <p className="flex-1 text-sm font-semibold">
          {known
            ? `${cap!.working} von ${cap!.total} Agenten arbeiten`
            : failed
            ? "Status nicht erreichbar"
            : "Lädt …"}
        </p>
        <Button
          size="icon-sm"
          variant="ghost"
          aria-label="Schließen"
          onClick={onClose}
        >
          <X />
        </Button>
      </div>
      {known && cap!.agents.length > 0 && (
        <ul className="mt-2 flex flex-wrap gap-1" aria-label="Agenten">
          {cap!.agents.map((a, i) => {
            const busy = a.status === "working" || a.status === "blocked";
            return (
              <li
                key={`${a.name}-${i}`}
                className={cn(
                  "flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px]",
                  busy
                    ? "border-[#D97757]/50 bg-[#D97757]/10"
                    : "text-muted-foreground"
                )}
                title={`${a.name}: ${a.status}`}
              >
                <i
                  className={cn(
                    "size-1.5 rounded-full",
                    busy ? "bg-[#D97757]" : "bg-muted-foreground/40"
                  )}
                  aria-hidden
                />
                {a.name}
              </li>
            );
          })}
        </ul>
      )}
      {data && !data.agentation && (
        <p className="mt-2 text-xs text-muted-foreground">
          Agentation ist nicht erreichbar: Status und Verlauf der Aufgaben
          fehlen.
        </p>
      )}
    </div>
  );
}

function TaskList({
  data,
  focusId,
  focusTick,
  onJump,
  onThread,
}: {
  data: Data | null;
  focusId: string | null;
  focusTick: number;
  onJump: () => void;
  onThread: (taskId: string, thread: AgentTask["thread"]) => void;
}) {
  const [expanded, setExpanded] = useState<string | null>(focusId);
  const [seenTick, setSeenTick] = useState(focusTick);
  const listRef = useRef<HTMLDivElement>(null);
  // Opened from elsewhere on one task (Inspector): expand it (adjusted while rendering, no effect needed) ...
  if (focusTick !== seenTick) {
    setSeenTick(focusTick);
    if (focusId) setExpanded(focusId);
  }
  // ... and bring it into view once it is rendered.
  useEffect(() => {
    if (!focusId) return;
    const frame = requestAnimationFrame(() =>
      listRef.current
        ?.querySelector(`[data-task="${CSS.escape(focusId)}"]`)
        ?.scrollIntoView({ block: "nearest" })
    );
    return () => cancelAnimationFrame(frame);
  }, [focusId, focusTick]);
  if (!data) return <p className="p-4 text-sm text-muted-foreground">Lädt …</p>;
  if (!data.tasks.length)
    return <p className="p-4 text-sm text-muted-foreground">Keine Aufgaben.</p>;
  return (
    <div
      ref={listRef}
      className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-2"
    >
      {ORDER.map((status) => {
        const all = data.tasks.filter((t) => t.status === status);
        if (!all.length) return null;
        const limit = SHOWN_PER_GROUP[status] ?? all.length;
        const shown = all.filter((t, i) => i < limit || t.id === focusId);
        return (
          <section
            key={status}
            className="mb-2"
            aria-label={STATUS[status].label}
          >
            <h3 className="flex items-center gap-2 px-2 pt-2 pb-1 text-[11px] font-semibold tracking-wide text-muted-foreground uppercase">
              <i
                className={cn("size-2 rounded-full", STATUS[status].dot)}
                aria-hidden
              />
              {STATUS[status].label}
              <span className="font-normal">{all.length}</span>
            </h3>
            <ul className="space-y-1">
              {shown.map((t) => (
                <TaskRow
                  key={t.id}
                  task={t}
                  open={expanded === t.id}
                  onToggle={() =>
                    setExpanded((e) => (e === t.id ? null : t.id))
                  }
                  onJump={onJump}
                  onThread={onThread}
                />
              ))}
            </ul>
          </section>
        );
      })}
    </div>
  );
}

function TaskRow({
  task,
  open,
  onToggle,
  onJump,
  onThread,
}: {
  task: AgentTask;
  open: boolean;
  onToggle: () => void;
  onJump: () => void;
  onThread: (taskId: string, thread: AgentTask["thread"]) => void;
}) {
  const detailId = useId();
  const ref =
    task.jobId !== undefined
      ? `${task.jobId}#${task.index}`
      : task.kind === "implement"
      ? "Implement"
      : "Notiz";
  return (
    <li
      data-task={task.id}
      className={cn(
        "rounded-lg border border-transparent",
        open && "border-border bg-muted/40"
      )}
    >
      <button
        type="button"
        className="flex w-full items-start gap-2 rounded-lg px-2 py-1.5 text-left hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        aria-expanded={open}
        aria-controls={detailId}
        onClick={onToggle}
      >
        <span className="mt-1.5 shrink-0">
          {task.status === "working" ? (
            <ClaudeMinion working className="-mt-0.5 h-3 w-4" />
          ) : (
            <i
              className={cn(
                "block size-2 rounded-full",
                STATUS[task.status].dot
              )}
              aria-hidden
            />
          )}
        </span>
        <span className="min-w-0 flex-1">
          <span
            className="block text-sm leading-snug font-medium break-words hyphens-auto"
            lang="de"
          >
            {task.title}
          </span>
          <span className="block truncate text-[11px] text-muted-foreground">
            {ref} · {STATUS[task.status].label} · {when(task.updatedAt)}
            {task.thread.length > 0 && ` · ${task.thread.length} im Verlauf`}
          </span>
        </span>
      </button>
      {open && (
        <TaskDetail
          id={detailId}
          task={task}
          onJump={onJump}
          onThread={onThread}
        />
      )}
    </li>
  );
}

function TaskDetail({
  id,
  task,
  onJump,
  onThread,
}: {
  id: string;
  task: AgentTask;
  onJump: () => void;
  onThread: (taskId: string, thread: AgentTask["thread"]) => void;
}) {
  return (
    <div id={id} className="space-y-3 px-3 pt-1 pb-3 text-sm">
      {task.request && (
        <p className="line-clamp-4 text-xs whitespace-pre-wrap text-muted-foreground">
          {task.request}
        </p>
      )}
      {task.baseTitle && <p className="text-xs">Basis: {task.baseTitle}</p>}

      <ol
        className="flex flex-wrap items-center gap-x-1 gap-y-1 text-[11px]"
        aria-label="Schritte"
      >
        {task.steps.map((s, i) => (
          <li key={s.key} className="flex items-center gap-1">
            {i > 0 && (
              <span
                className={cn(
                  "h-px w-3",
                  s.done ? "bg-foreground/50" : "bg-border"
                )}
                aria-hidden
              />
            )}
            <span
              className={cn(
                "flex items-center gap-1 rounded-full border px-2 py-0.5",
                s.done
                  ? "border-foreground/30 text-foreground"
                  : "border-dashed text-muted-foreground",
                s.key === "cancelled" &&
                  "border-destructive/50 text-destructive"
              )}
              title={
                s.at
                  ? new Date(s.at).toLocaleString("de-DE")
                  : s.done
                  ? "Zeit nicht erfasst"
                  : "noch nicht"
              }
            >
              {s.done ? (
                <Check className="size-3" aria-hidden />
              ) : (
                <Circle className="size-2.5" aria-hidden />
              )}
              {s.label}
              {s.at && (
                <span className="text-muted-foreground">{when(s.at)}</span>
              )}
              <span className="sr-only">{s.done ? "erledigt" : "offen"}</span>
            </span>
          </li>
        ))}
      </ol>

      {task.target && (
        <div className="flex items-center gap-2 text-xs">
          <span
            className="min-w-0 flex-1 truncate text-muted-foreground"
            title={task.target.spacePath.join(" › ")}
          >
            {task.target.spacePath.join(" › ")}
          </span>
          <Button
            size="xs"
            variant="outline"
            onClick={() => {
              enterSpace(task.target!.spaceId, task.target!.designId);
              onJump();
            }}
          >
            Zur Karte <ArrowUpRight />
          </Button>
        </div>
      )}

      {task.thread.length > 0 && (
        <ol
          className="max-h-56 space-y-2 overflow-y-auto rounded-md border bg-background p-2"
          aria-label="Verlauf"
        >
          {task.thread.map((m) => (
            <li key={m.id} className="text-xs">
              <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                <b
                  className={cn(
                    "font-semibold",
                    m.role === "human"
                      ? "text-sky-600 dark:text-sky-400"
                      : "text-[#C2643F] dark:text-[#E89A7C]"
                  )}
                >
                  {m.role === "human"
                    ? "Du"
                    : m.role === "agent"
                    ? "Agent"
                    : m.role}
                </b>
                <time dateTime={m.at}>{when(m.at)}</time>
              </span>
              <p className="mt-0.5 whitespace-pre-wrap break-words">
                {m.content}
              </p>
            </li>
          ))}
        </ol>
      )}

      {task.noteId ? (
        <NoteComposer noteId={task.noteId} onThread={onThread} />
      ) : (
        <p className="text-xs text-muted-foreground">
          Ohne Agentation-Notiz kann hier keine Notiz angehängt werden.
        </p>
      )}
    </div>
  );
}

function NoteComposer({
  noteId,
  onThread,
}: {
  noteId: string;
  onThread: (taskId: string, thread: AgentTask["thread"]) => void;
}) {
  // Kept in the local `drafts` key per note, so a reload doesn't lose a half-written note.
  const [text, setText] = useDraft(`note:${noteId}`);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState("");
  const fieldId = useId();
  const send = async () => {
    const value = text.trim();
    if (!value) {
      setError("Bitte einen Text eingeben.");
      return;
    }
    setSending(true);
    setError("");
    try {
      const res = await fetch(
        `/api/agent-tasks/${encodeURIComponent(noteId)}/notes`,
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ text: value }),
        }
      );
      const body = (await res.json().catch(() => ({}))) as {
        thread?: AgentTask["thread"];
        error?: string;
      };
      if (!res.ok || !body.thread)
        throw new Error(body.error ?? `Fehler ${res.status}`);
      onThread(noteId, body.thread);
      setText("");
      toast.success("Notiz angehängt");
    } catch (e) {
      setError(e instanceof Error ? e.message : "Notiz nicht gesendet");
    } finally {
      setSending(false);
    }
  };
  return (
    <form
      className="space-y-1.5"
      onSubmit={(e) => {
        e.preventDefault();
        void send();
      }}
    >
      <label htmlFor={fieldId} className="sr-only">
        Notiz zur Aufgabe
      </label>
      <Textarea
        id={fieldId}
        value={text}
        rows={2}
        maxLength={2000}
        placeholder="Notiz an den Agenten …"
        aria-invalid={!!error || undefined}
        className="min-h-14 text-sm"
        onChange={(e) => {
          setText(e.target.value);
          if (error) setError("");
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            void send();
          }
        }}
      />
      <div className="flex items-center gap-2">
        <p className="min-w-0 flex-1 text-xs text-destructive" role="status">
          {error}
        </p>
        <Button size="xs" type="submit" disabled={sending}>
          {sending ? (
            <Loader2 className="animate-spin" />
          ) : (
            <MessageSquarePlus />
          )}
          Notiz anhängen
        </Button>
      </div>
    </form>
  );
}
