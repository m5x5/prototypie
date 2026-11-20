"use client";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { useReactFlow } from "@xyflow/react";
import {
  AlertTriangle,
  Check,
  CheckCheck,
  Inbox,
  Loader2,
  LocateFixed,
  RefreshCw,
  Star,
  Trash2,
  X,
} from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { DesignPreview } from "@/components/design-preview";
import { cn } from "@/lib/utils";
import type { Design } from "@/lib/types";
import { useApp, type AppState } from "@/store";
import { useDraft } from "@/store/use-draft";
import { CountPicker } from "./dialogs";
import {
  annotationsFor,
  belowOrigins,
  focusOf,
  hubOf,
  isTypingTarget,
  laneOf,
  launchLane,
  parseSelection,
  siblingsOf,
  stripSelection,
  useSpokesUI,
} from "./lib";
import { appendDictation, VoiceButton } from "./voice";
import { framePadding } from "@/lib/canvas-insets";

/** Ready designs not yet reviewed, oldest first: the stacked to-do list. */
export function selectQueueIds(s: AppState): string[] {
  return Object.values(s.designs)
    .filter((d) => d.status === "ready" && !s.reviewedIds[d.id])
    .sort((a, b) => a.createdAt - b.createdAt)
    .map((d) => d.id);
}

export function useQueueIds() {
  return useApp(useShallow(selectQueueIds));
}

export function ReviewDialog() {
  const open = useSpokesUI((s) => s.reviewOpen);
  const close = useSpokesUI((s) => s.closeReview);
  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()}>
      <DialogContent
        showCloseButton={false}
        className="flex h-[94vh] w-[96vw] max-w-[96vw] flex-col gap-0 overflow-hidden p-0 sm:max-w-[96vw] max-md:inset-0 max-md:h-dvh max-md:w-screen max-md:max-w-none max-md:translate-x-0 max-md:translate-y-0 max-md:rounded-none max-md:pb-[env(safe-area-inset-bottom)] max-md:sm:max-w-none"
      >
        {open && <ReviewMode />}
      </DialogContent>
    </Dialog>
  );
}

function laneName(s: AppState, id: string) {
  const lane = laneOf(s, id);
  if (lane) return lane.name;
  const job = Object.values(s.jobs).find((j) => j.designIds.includes(id));
  return job?.label;
}

function ReviewMode() {
  const startId = useSpokesUI((s) => s.reviewStartId);
  const close = useSpokesUI((s) => s.closeReview);
  const queue = useQueueIds();
  const designs = useApp((s) => s.designs);
  const reviewedIds = useApp((s) => s.reviewedIds);
  const { updateDesign, removeDesign, markReviewed, unmarkReviewed, setActiveSpace, select } = useApp(
    useShallow((s) => ({
      updateDesign: s.updateDesign,
      removeDesign: s.removeDesign,
      markReviewed: s.markReviewed,
      unmarkReviewed: s.unmarkReviewed,
      setActiveSpace: s.setActiveSpace,
      select: s.select,
    })),
  );
  const flow = useReactFlow();

  const [currentId, setCurrentId] = useState<string | null>(startId);
  const effectiveId = currentId && designs[currentId] ? currentId : (queue[0] ?? null);
  const current: Design | undefined = effectiveId ? designs[effectiveId] : undefined;

  const siblingIds = useApp(useShallow((s) => (effectiveId ? siblingsOf(s, effectiveId).map((d) => d.id) : [])));
  const generatingIds = useApp(
    useShallow((s) =>
      Object.values(s.designs)
        .filter((d) => d.status === "generating")
        .sort((a, b) => a.createdAt - b.createdAt)
        .map((d) => d.id),
    ),
  );
  const meta = useApp(
    useShallow((s) =>
      effectiveId
        ? { lane: laneName(s, effectiveId), focus: focusOf(s, effectiveId), hub: hubOf(s, effectiveId)?.title }
        : { lane: undefined, focus: undefined, hub: undefined },
    ),
  );

  // Feedback + sibling selection ("3 and 6, more playful").
  // Local `drafts` key: half-written feedback survives a reload; cleared when a round is sent.
  const [feedback, setFeedback] = useDraft("review:feedback");
  const [overrides, setOverrides] = useState<Record<number, boolean>>({});
  const [roundCount, setRoundCount] = useState(3);
  const dictationBase = useRef("");
  const feedbackRef = useRef<HTMLTextAreaElement>(null);

  const parsed = useMemo(() => parseSelection(feedback, siblingIds.length), [feedback, siblingIds.length]);
  const selectedNums = siblingIds
    .map((_, i) => i + 1)
    .filter((n) => overrides[n] ?? parsed.includes(n));
  const currentNum = effectiveId ? siblingIds.indexOf(effectiveId) + 1 : 0;
  const queueIdx = effectiveId ? queue.indexOf(effectiveId) : -1;

  const toggleNum = (n: number) => setOverrides((o) => ({ ...o, [n]: !(o[n] ?? parsed.includes(n)) }));

  // Reset manual picks when moving to another round.
  const roundKey = siblingIds.join(",");
  const [prevRoundKey, setPrevRoundKey] = useState(roundKey);
  if (prevRoundKey !== roundKey) {
    setPrevRoundKey(roundKey);
    setOverrides({});
  }

  const go = useCallback(
    (delta: 1 | -1) => {
      if (!queue.length) return;
      if (queueIdx === -1) {
        setCurrentId(delta === 1 ? queue[0] : queue[queue.length - 1]);
        return;
      }
      const next = queue[Math.min(Math.max(queueIdx + delta, 0), queue.length - 1)];
      setCurrentId(next);
    },
    [queue, queueIdx],
  );

  /** The queue item to show once the current one leaves the queue. */
  const nextAfterCurrent = useCallback(() => {
    if (queueIdx === -1) return queue.find((id) => id !== effectiveId) ?? null;
    return queue[queueIdx + 1] ?? queue[queueIdx - 1] ?? null;
  }, [queue, queueIdx, effectiveId]);

  const done = useCallback(() => {
    if (!effectiveId) return;
    const next = nextAfterCurrent();
    markReviewed([effectiveId]);
    setCurrentId(next);
  }, [effectiveId, markReviewed, nextAfterCurrent]);

  const discard = useCallback(() => {
    if (!current) return;
    const next = nextAfterCurrent();
    const snapshot = current;
    removeDesign(current.id);
    setCurrentId(next);
    toast(`Discarded "${snapshot.title}"`, {
      action: { label: "Undo", onClick: () => useApp.getState().addDesign(snapshot) },
    });
  }, [current, nextAfterCurrent, removeDesign]);

  const toggleStar = useCallback(() => {
    if (current) updateDesign(current.id, { starred: !current.starred });
  }, [current, updateDesign]);

  const moveInStrip = useCallback(
    (delta: 1 | -1) => {
      if (!siblingIds.length || !effectiveId) return;
      const i = siblingIds.indexOf(effectiveId);
      const next = siblingIds[(i + delta + siblingIds.length) % siblingIds.length];
      setCurrentId(next);
    },
    [siblingIds, effectiveId],
  );

  const anotherRound = () => {
    if (!current) return;
    const state = useApp.getState();
    const picked = selectedNums.map((n) => state.designs[siblingIds[n - 1]]).filter((d) => d?.status === "ready");
    const targets = picked.length ? picked : current.status === "ready" ? [current] : [];
    if (!targets.length) return;
    const instruction =
      stripSelection(feedback) || "Push this direction further: keep what works and explore fresh takes on it.";
    const origins = belowOrigins(state, targets, roundCount);
    const lane = laneName(state, current.id);
    targets.forEach((t, i) => {
      const focus = focusOf(state, t.id);
      const num = siblingIds.indexOf(t.id) + 1;
      const name = `${lane ? `${lane.replace(/^Spoke · /, "")} ` : ""}#${num || 1} → round`;
      void launchLane(
        {
          spaceId: t.spaceId,
          parentId: t.id,
          kind: t.kind,
          origin: origins[i],
          label: name,
          request: {
            baseHtml: t.html,
            baseTitle: t.title,
            instruction,
            count: roundCount,
            focus,
            scope: t.kind === "spoke" ? "component" : undefined,
            annotations: annotationsFor(state, t.id),
          },
        },
        { type: "round", name, focus, hubId: hubOf(state, t.id)?.id },
      );
    });
    toast(`Another round on ${targets.length === 1 ? "1 design" : `${targets.length} designs`}`, {
      description: `${targets.length * roundCount} new directions will join the queue as they land.`,
    });
    // Feedback given counts as reviewed: move on.
    const next = nextAfterCurrent();
    markReviewed([current.id, ...targets.map((t) => t.id)]);
    setCurrentId(next);
    setFeedback("");
    setOverrides({});
  };

  const showOnCanvas = () => {
    if (!current) return;
    setActiveSpace(current.spaceId);
    select(current.id);
    close();
    setTimeout(() => void flow.fitView({ nodes: [{ id: current.id }], duration: 500, padding: framePadding(0.4), maxZoom: 1 }), 60);
  };

  // Keyboard shortcuts (never while typing).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || isTypingTarget(e.target)) return;
      const t = e.target as HTMLElement | null;
      const onControl = !!t?.closest("button, a, [role=radio], [role=checkbox]");
      const k = e.key;
      if (k === "j" || k === "J" || k === "ArrowDown") go(1);
      else if (k === "k" || k === "K" || k === "ArrowUp") go(-1);
      else if (k === "ArrowRight") moveInStrip(1);
      else if (k === "ArrowLeft") moveInStrip(-1);
      else if (k === "s" || k === "S") toggleStar();
      else if (k === "x" || k === "X") discard();
      else if (k === "Enter" && !onControl) done();
      else if (k === "f" || k === "/") feedbackRef.current?.focus();
      else if (/^[1-9]$/.test(k) && Number(k) <= siblingIds.length) toggleNum(Number(k));
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const total = queue.length;
  const position = queueIdx >= 0 ? queueIdx + 1 : null;
  const roundTargets = selectedNums.length ? selectedNums : currentNum ? [currentNum] : [];

  return (
    <>
      {/* Header */}
      <div className="flex h-12 shrink-0 items-center gap-2 border-b px-3 md:gap-3 md:px-4">
        <DialogTitle className="flex items-center gap-2 text-sm font-semibold">
          <Inbox className="size-4" /> Review
        </DialogTitle>
        <DialogDescription className="min-w-0 truncate text-xs tabular-nums">
          {total ? (position ? `${position} of ${total} to review` : `${total} to review`) : "All caught up"}
          {generatingIds.length > 0 && ` · ${generatingIds.length} still designing`}
        </DialogDescription>
        <div className="ml-auto flex shrink-0 items-center gap-1">
          {total > 0 && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                const ids = [...queue];
                markReviewed(ids);
                toast(`Marked ${ids.length} as reviewed`, {
                  action: { label: "Undo", onClick: () => ids.forEach((id) => unmarkReviewed(id)) },
                });
              }}
            >
              <CheckCheck /> <span className="max-md:sr-only">Mark all reviewed</span>
            </Button>
          )}
          <Button size="icon-sm" variant="ghost" className="max-md:size-10" aria-label="Close review" onClick={close}>
            <X />
          </Button>
        </div>
      </div>

      <div className="flex min-h-0 flex-1 max-md:flex-col max-md:overflow-y-auto">
        {/* Queue stack */}
        <aside className="hidden w-60 shrink-0 flex-col border-r bg-muted/20 md:flex">
          <div className="min-h-0 flex-1 overflow-y-auto p-2">
            <p className="px-2 pb-1.5 pt-1 text-xs md:text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
              Queue
            </p>
            {!queue.length && <p className="px-2 text-xs text-muted-foreground">Nothing waiting.</p>}
            {queue.map((id, i) => (
              <QueueItem
                key={id}
                id={id}
                index={i + 1}
                active={id === effectiveId}
                onClick={() => setCurrentId(id)}
              />
            ))}
            {generatingIds.length > 0 && (
              <>
                <p className="px-2 pb-1.5 pt-4 text-xs md:text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
                  Designing
                </p>
                {generatingIds.map((id) => (
                  <div key={id} className="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs text-muted-foreground">
                    <Loader2 className="size-3 shrink-0 animate-spin" />
                    <span className="truncate">{designs[id]?.title}</span>
                  </div>
                ))}
              </>
            )}
          </div>
        </aside>

        {/* Stage */}
        <section className="flex min-w-0 flex-1 flex-col bg-muted/30 max-md:flex-none">
          {current ? (
            <>
              <div className="min-h-0 flex-1 p-4 max-md:h-[42dvh] max-md:flex-none max-md:p-2">
                {current.status === "ready" ? (
                  <DesignPreview
                    key={current.id}
                    html={current.html}
                    interactive
                    viewport={current.kind === "spoke" ? 1024 : 1280}
                    className="h-full w-full rounded-lg border shadow-sm"
                  />
                ) : current.status === "generating" ? (
                  <StageMessage icon={<Loader2 className="size-5 animate-spin" />} text="Still designing…" />
                ) : (
                  <StageMessage
                    icon={<AlertTriangle className="size-5 text-destructive" />}
                    text={current.error ?? "Generation failed"}
                  />
                )}
              </div>
              {siblingIds.length > 1 && (
                <div className="shrink-0 border-t bg-background/80 px-4 py-3">
                  <div className="flex gap-3 overflow-x-auto pb-1">
                    {siblingIds.map((id, i) => (
                      <SiblingThumb
                        key={id}
                        id={id}
                        num={i + 1}
                        current={id === effectiveId}
                        reviewed={!!reviewedIds[id]}
                        selected={selectedNums.includes(i + 1)}
                        onOpen={() => setCurrentId(id)}
                        onToggle={() => toggleNum(i + 1)}
                      />
                    ))}
                  </div>
                </div>
              )}
            </>
          ) : (
            <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
              <div className="rounded-full bg-emerald-500/10 p-3 text-emerald-600">
                <Check className="size-6" />
              </div>
              <div>
                <p className="font-medium">Inbox zero</p>
                <p className="mt-1 max-w-xs text-sm text-muted-foreground">
                  {generatingIds.length
                    ? `${generatingIds.length} design${generatingIds.length === 1 ? " is" : "s are"} still being made. Each one shows up here the moment it's ready.`
                    : "Break a page into spokes or start another round. Finished designs land here."}
                </p>
              </div>
              {generatingIds.length > 0 && <Loader2 className="size-4 animate-spin text-muted-foreground" />}
            </div>
          )}
        </section>

        {/* Feedback panel */}
        {current && (
          <aside className="flex w-[340px] shrink-0 flex-col border-l max-md:w-full max-md:border-l-0 max-md:border-t">
            <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-4 max-md:overflow-visible">
              <div className="space-y-1.5">
                <div className="flex items-start gap-2">
                  {currentNum > 0 && siblingIds.length > 1 && (
                    <span className="mt-0.5 grid size-5 shrink-0 place-items-center rounded-md bg-foreground text-xs font-semibold tabular-nums text-background">
                      {currentNum}
                    </span>
                  )}
                  <h3 className="text-sm font-semibold leading-snug">{current.title}</h3>
                </div>
                <div className="flex flex-wrap items-center gap-1.5">
                  <Badge variant="secondary" className="capitalize">
                    {current.kind}
                  </Badge>
                  {meta.lane && <Badge variant="outline">{meta.lane}</Badge>}
                  {reviewedIds[current.id] && (
                    <Badge variant="outline" className="text-emerald-600">
                      <Check /> Reviewed
                    </Badge>
                  )}
                </div>
                {current.rationale && <p className="text-xs text-muted-foreground">{current.rationale}</p>}
                {meta.focus && (
                  <p className="text-xs text-muted-foreground">
                    <span className="font-medium text-foreground">Component:</span> {meta.focus}
                  </p>
                )}
                {meta.hub && (
                  <p className="text-xs text-muted-foreground">
                    <span className="font-medium text-foreground">Hub:</span> {meta.hub}
                  </p>
                )}
              </div>

              <div className="grid grid-cols-3 gap-1.5">
                <Button size="sm" variant="outline" className="max-md:h-10" onClick={toggleStar}>
                  <Star className={cn(current.starred && "fill-amber-400 text-amber-400")} />
                  {current.starred ? "Starred" : "Star"}
                  <Kbd>S</Kbd>
                </Button>
                <Button size="sm" variant="outline" className="max-md:h-10" onClick={discard}>
                  <Trash2 /> Discard <Kbd>X</Kbd>
                </Button>
                <Button size="sm" className="max-md:h-10" onClick={done}>
                  <Check /> Done <Kbd className="text-primary-foreground/70">↵</Kbd>
                </Button>
              </div>

              <Separator />

              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <label htmlFor="review-feedback" className="text-xs font-medium">
                    Feedback
                  </label>
                  <span className="text-xs md:text-[11px] text-muted-foreground">Try “3 and 6, more playful”</span>
                </div>
                <div className="relative">
                  <Textarea
                    id="review-feedback"
                    ref={feedbackRef}
                    rows={4}
                    value={feedback}
                    onChange={(e) => setFeedback(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                        e.preventDefault();
                        anotherRound();
                      } else if (e.key === "Escape" && feedback) {
                        e.stopPropagation();
                        (e.target as HTMLTextAreaElement).blur();
                      }
                    }}
                    placeholder="Talk or type. Numbers at the start pick siblings from this round."
                    className="min-h-24 pr-10"
                  />
                  <VoiceButton
                    hotkey="m"
                    className="absolute right-1.5 top-1.5"
                    onStart={() => (dictationBase.current = feedback)}
                    onText={(t) => setFeedback(appendDictation(dictationBase.current, t))}
                  />
                </div>
                {siblingIds.length > 1 && (
                  <div className="flex flex-wrap items-center gap-1">
                    <span className="mr-1 text-xs md:text-[11px] text-muted-foreground">Round on</span>
                    {siblingIds.map((_, i) => {
                      const n = i + 1;
                      const on = roundTargets.includes(n);
                      return (
                        <button
                          key={n}
                          type="button"
                          role="checkbox"
                          aria-checked={on}
                          aria-label={`Include #${n}`}
                          onClick={() => toggleNum(n)}
                          className={cn(
                            "grid h-6 min-w-6 max-md:h-10 max-md:min-w-10 place-items-center rounded-md border px-1 text-xs font-medium tabular-nums transition-colors",
                            on
                              ? "border-amber-500 bg-amber-500/15 text-amber-700 dark:text-amber-300"
                              : "text-muted-foreground hover:text-foreground",
                          )}
                        >
                          {n}
                        </button>
                      );
                    })}
                  </div>
                )}
                <div className="flex items-center justify-between gap-2 pt-1">
                  <CountPicker value={roundCount} onChange={setRoundCount} max={5} label="Each" />
                </div>
                <Button
                  className="w-full max-md:h-11"
                  onClick={anotherRound}
                  disabled={!roundTargets.length || current.status !== "ready"}
                >
                  <RefreshCw /> Another round
                  {roundTargets.length > 0 && (
                    <span className="opacity-70">
                      on {roundTargets.map((n) => `#${n}`).join(", ")}
                    </span>
                  )}
                  <Kbd className="ml-auto text-primary-foreground/70">⌘↵</Kbd>
                </Button>
              </div>

              <Button size="sm" variant="ghost" className="w-full justify-start text-muted-foreground max-md:h-10" onClick={showOnCanvas}>
                <LocateFixed /> Show on canvas
              </Button>
            </div>
            <Shortcuts />
          </aside>
        )}
      </div>
    </>
  );
}

function QueueItem({ id, index, active, onClick }: { id: string; index: number; active: boolean; onClick: () => void }) {
  const d = useApp((s) => s.designs[id]);
  const lane = useApp((s) => laneName(s, id));
  const ref = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (active) ref.current?.scrollIntoView({ block: "nearest" });
  }, [active]);
  if (!d) return null;
  return (
    <button
      ref={ref}
      type="button"
      onClick={onClick}
      className={cn(
        "flex w-full items-start gap-2 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-muted",
        active && "bg-background shadow-sm ring-1 ring-border",
      )}
    >
      <span className="mt-0.5 w-4 shrink-0 text-right text-xs md:text-[10px] tabular-nums text-muted-foreground">{index}</span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1 truncate text-xs font-medium">
          {d.starred && <Star className="size-3 shrink-0 fill-amber-400 text-amber-400" />}
          <span className="truncate">{d.title}</span>
        </span>
        <span className="block truncate text-xs md:text-[11px] text-muted-foreground">{lane ?? d.kind}</span>
      </span>
    </button>
  );
}

function SiblingThumb({
  id,
  num,
  current,
  reviewed,
  selected,
  onOpen,
  onToggle,
}: {
  id: string;
  num: number;
  current: boolean;
  reviewed: boolean;
  selected: boolean;
  onOpen: () => void;
  onToggle: () => void;
}) {
  const d = useApp((s) => s.designs[id]);
  if (!d) return null;
  return (
    <div className="relative w-36 shrink-0 md:w-40">
      <button
        type="button"
        onClick={onOpen}
        aria-label={`Open #${num}: ${d.title}`}
        className={cn(
          "block w-full overflow-hidden rounded-md border bg-background text-left transition",
          current ? "ring-2 ring-primary" : "opacity-80 hover:opacity-100",
          selected && !current && "ring-2 ring-amber-500",
        )}
      >
        {d.status === "ready" ? (
          <DesignPreview html={d.html} className="aspect-[4/3] w-full" />
        ) : d.status === "generating" ? (
          <div className="grid aspect-[4/3] w-full place-items-center bg-muted/50">
            <Loader2 className="size-4 animate-spin text-muted-foreground" />
          </div>
        ) : (
          <div className="grid aspect-[4/3] w-full place-items-center bg-destructive/5">
            <AlertTriangle className="size-4 text-destructive" />
          </div>
        )}
        <span className="block truncate px-1.5 py-1 text-xs md:text-[11px]">{d.title}</span>
      </button>
      <button
        type="button"
        onClick={onToggle}
        aria-label={selected ? `Deselect #${num}` : `Select #${num} for another round`}
        title={selected ? "Selected for another round" : "Select for another round"}
        className={cn(
          "absolute left-1.5 top-1.5 grid size-5 place-items-center rounded-md text-xs font-semibold tabular-nums shadow-sm transition-colors",
          selected ? "bg-amber-500 text-white" : "bg-foreground/80 text-background hover:bg-foreground",
        )}
      >
        {num}
      </button>
      {(reviewed || d.starred) && (
        <span className="absolute right-1.5 top-1.5 flex gap-0.5">
          {d.starred && <Star className="size-3.5 fill-amber-400 text-amber-400 drop-shadow" />}
          {reviewed && <Check className="size-3.5 rounded-full bg-emerald-500 p-0.5 text-white" />}
        </span>
      )}
    </div>
  );
}

function StageMessage({ icon, text }: { icon: React.ReactNode; text: string }) {
  return (
    <div className="flex h-full w-full flex-col items-center justify-center gap-2 rounded-lg border border-dashed bg-background/60 p-6 text-center text-sm text-muted-foreground">
      {icon}
      <span className="max-w-md">{text}</span>
    </div>
  );
}

function Kbd({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <kbd className={cn("ml-0.5 font-sans text-xs md:text-[10px] text-muted-foreground", className)}>{children}</kbd>
  );
}

function Shortcuts() {
  const rows: [string, string][] = [
    ["J / K", "next / previous in queue"],
    ["← / →", "move through this round"],
    ["1-9", "pick siblings for a round"],
    ["S · X · ↵", "star · discard · done"],
    ["F · M", "feedback · dictate"],
  ];
  return (
    <div className="shrink-0 border-t bg-muted/30 px-4 py-3 max-md:hidden">
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs md:text-[11px]">
        {rows.map(([k, v]) => (
          <div key={k} className="contents">
            <dt className="font-mono text-foreground/80">{k}</dt>
            <dd className="text-muted-foreground">{v}</dd>
          </div>
        ))}
      </dl>
      <p className="mt-2 text-xs md:text-[10px] text-muted-foreground/80">Click outside the preview to use shortcuts.</p>
    </div>
  );
}
