"use client";
import { useState } from "react";
import { nanoid } from "nanoid";
import { toast } from "sonner";
import {
  AlertTriangle,
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Check,
  ListPlus,
  Loader2,
  Plus,
  Send,
  Sparkles,
  Trash2,
  Vote,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { DesignPreview } from "@/components/design-preview";
import type { Design } from "@/lib/types";
import { useApp } from "@/store";
import { useDraft } from "@/store/use-draft";
import { shareUrlFor, usePollUI } from "./client-utils";
import { PollResults } from "./results";
import { ShareLink } from "./share-link";
import {
  readError,
  type CreatePollRequest,
  type CreatePollResponse,
  type DecisionsRequest,
  type DecisionsResponse,
} from "./schema";
import { moveItem, type DraftDecision, type PublishedPoll } from "./slice";

export function DecisionsPanel() {
  const open = usePollUI((s) => s.open);
  const setOpen = usePollUI((s) => s.setOpen);
  const tab = usePollUI((s) => s.tab);
  const setTab = usePollUI((s) => s.setTab);
  const publishedCount = useApp((s) => s.publishedPolls.length);

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="flex h-[90vh] w-[96vw] max-w-[96vw] flex-col gap-0 overflow-hidden p-0 sm:max-w-6xl max-md:inset-0 max-md:top-0 max-md:left-0 max-md:h-dvh max-md:w-screen max-md:max-w-none max-md:translate-x-0 max-md:translate-y-0 max-md:rounded-none max-md:pb-[env(safe-area-inset-bottom)] max-md:[&>[data-slot=dialog-close]]:size-10">
        <DialogHeader className="border-b px-4 pt-5 pb-4 md:px-6 max-md:pr-14">
          <DialogTitle className="flex items-center gap-2 text-lg">
            <Vote className="size-5" /> Decisions
          </DialogTitle>
          <DialogDescription>
            Don&apos;t share the messy file. Package the competing directions into a few clear questions and let
            stakeholders vote on live prototypes.
          </DialogDescription>
        </DialogHeader>
        <Tabs
          value={tab}
          onValueChange={(v) => setTab(v as "build" | "results")}
          className="flex min-h-0 flex-1 flex-col gap-0"
        >
          <div className="border-b px-4 py-2 md:px-6">
            <TabsList>
              <TabsTrigger value="build" className="px-3">
                Build poll
              </TabsTrigger>
              <TabsTrigger value="results" className="px-3">
                Results
                {publishedCount > 0 && (
                  <Badge variant="secondary" className="ml-1 h-4 px-1.5 text-[10px]">
                    {publishedCount}
                  </Badge>
                )}
              </TabsTrigger>
            </TabsList>
          </div>
          <TabsContent value="build" className="min-h-0 flex-1">
            <BuildTab />
          </TabsContent>
          <TabsContent value="results" className="min-h-0 flex-1">
            <PollResults />
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

// ---------------------------------------------------------------------------------------------------------------

function useCandidates(): Design[] {
  const ids = useApp((s) => s.pollCandidates);
  const designs = useApp((s) => s.designs);
  return ids.map((id) => designs[id]).filter((d): d is Design => !!d);
}

function BuildTab() {
  const candidates = useCandidates();
  const decisions = useApp((s) => s.pollDecisions);
  const title = useApp((s) => s.pollTitle);
  const setTitle = useApp((s) => s.setPollTitle);
  const addDecision = useApp((s) => s.addPollDecision);
  const [published, setPublished] = useState<PublishedPoll | null>(null);

  return (
    <div className="flex h-full min-h-0 max-md:flex-col max-md:overflow-y-auto">
      <CandidateTray candidates={candidates} />
      <div className="flex min-w-0 flex-1 flex-col max-md:shrink-0">
        <div className="min-h-0 flex-1 overflow-y-auto max-md:overflow-visible px-4 py-5 md:px-6">
          <label className="mb-1 block text-xs font-medium text-muted-foreground" htmlFor="poll-title">
            Poll title
          </label>
          <Input
            id="poll-title"
            value={title}
            placeholder="e.g. Checkout redesign - pick a direction"
            onChange={(e) => setTitle(e.target.value)}
            className="mb-6 h-10 text-base font-medium md:text-base"
          />

          {decisions.length === 0 ? (
            <EmptyDecisions hasCandidates={candidates.length >= 2} />
          ) : (
            <ol className="flex flex-col gap-4">
              {decisions.map((d, i) => (
                <DecisionEditor key={d.id} decision={d} index={i} count={decisions.length} candidates={candidates} />
              ))}
            </ol>
          )}

          <Button variant="outline" className="mt-4 w-full border-dashed" onClick={() => addDecision()}>
            <Plus /> Add a decision manually
          </Button>
        </div>
        <PublishBar candidates={candidates} published={published} onPublished={setPublished} />
      </div>
    </div>
  );
}

function EmptyDecisions({ hasCandidates }: { hasCandidates: boolean }) {
  return (
    <div className="flex flex-col items-center justify-center rounded-xl border border-dashed bg-muted/30 px-6 py-14 text-center">
      <ListPlus className="mb-3 size-6 text-muted-foreground" />
      <p className="font-medium">No decisions yet</p>
      <p className="mt-1 max-w-sm text-sm text-muted-foreground">
        {hasCandidates
          ? "Let the agent find the core decisions in your candidates, or add one yourself."
          : "Add two or more designs to the poll from the canvas (the “Poll” button on a design), then let the agent find the decisions."}
      </p>
    </div>
  );
}

// ---- candidate tray ---------------------------------------------------------------------------------------------

function CandidateTray({ candidates }: { candidates: Design[] }) {
  const toggle = useApp((s) => s.togglePollCandidate);
  const decisions = useApp((s) => s.pollDecisions);
  const apply = useApp((s) => s.applySuggestedDecisions);
  const setDecisions = useApp((s) => s.setPollDecisions);
  const title = useApp((s) => s.pollTitle);
  const setTitle = useApp((s) => s.setPollTitle);
  // Local `drafts` key: the context survives a reload (kept after a search, like before).
  const [context, setContext] = useDraft("poll:context");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const ready = candidates.filter((d) => d.status === "ready" && d.html);

  async function findDecisions() {
    setLoading(true);
    setError(null);
    try {
      const body: DecisionsRequest = {
        designs: ready.map((d) => ({ id: d.id, title: d.title, rationale: d.rationale, html: d.html })),
        context: context.trim() || undefined,
      };
      const res = await fetch("/api/decisions", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) throw new Error(await readError(res));
      const data = (await res.json()) as DecisionsResponse;
      const previous = decisions;
      apply(data.decisions);
      if (!title.trim()) setTitle("Which direction should we take?");
      toast.success(`The agent found ${data.decisions.length} decision${data.decisions.length === 1 ? "" : "s"}`, {
        action: previous.length ? { label: "Undo", onClick: () => setDecisions(previous) } : undefined,
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  return (
    <aside className="flex w-72 shrink-0 flex-col border-r bg-muted/30 max-md:w-full max-md:border-r-0 max-md:border-b">
      <div className="flex items-center justify-between px-4 pt-4 pb-2">
        <span className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Poll tray</span>
        <span className="text-xs text-muted-foreground">{candidates.length} designs</span>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-4 max-md:overflow-visible">
        {candidates.length === 0 ? (
          <p className="rounded-lg border border-dashed p-4 text-center text-xs text-muted-foreground">
            Use <span className="font-medium text-foreground">Poll</span> on a design node to collect the competing
            variations here.
          </p>
        ) : (
          <ul className="flex flex-col gap-3">
            {candidates.map((d) => (
              <li key={d.id} className="group relative overflow-hidden rounded-lg border bg-background">
                {d.status === "ready" ? (
                  <DesignPreview html={d.html} className="aspect-[4/3] w-full" />
                ) : (
                  <div className="flex aspect-[4/3] items-center justify-center text-xs text-muted-foreground">
                    {d.status === "generating" ? "Still designing…" : "Failed to generate"}
                  </div>
                )}
                <div className="flex items-center gap-1 border-t px-2 py-1.5">
                  <span className="truncate text-xs font-medium" title={d.title}>
                    {d.title}
                  </span>
                  <Button
                    size="icon-xs"
                    variant="ghost"
                    className="ml-auto"
                    aria-label={`Remove ${d.title} from poll`}
                    onClick={() => toggle(d.id)}
                  >
                    <X />
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
      <div className="flex flex-col gap-2 border-t bg-background/60 p-4">
        <Textarea
          value={context}
          onChange={(e) => setContext(e.target.value)}
          placeholder="Optional: what are you torn about?"
          className="max-h-24 min-h-12 resize-none text-xs md:text-xs"
        />
        <Button onClick={findDecisions} disabled={loading || ready.length < 2}>
          {loading ? <Loader2 className="animate-spin" /> : <Sparkles />}
          {loading ? "Finding decisions…" : "Let the agent find the decisions"}
        </Button>
        {ready.length < 2 && !loading && (
          <p className="text-center text-[11px] text-muted-foreground">Needs at least two finished designs.</p>
        )}
        {error && (
          <div role="alert" className="flex gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-2.5 text-xs text-destructive">
            <AlertTriangle className="mt-px size-3.5 shrink-0" />
            <div className="min-w-0">
              <p className="font-medium">Couldn&apos;t reach the agent</p>
              <p className="mt-0.5 break-words text-destructive/80">{error}</p>
              <p className="mt-1 text-muted-foreground">You can still build decisions by hand.</p>
            </div>
          </div>
        )}
      </div>
    </aside>
  );
}

// ---- decision editor -------------------------------------------------------------------------------------------

function DecisionEditor({
  decision,
  index,
  count,
  candidates,
}: {
  decision: DraftDecision;
  index: number;
  count: number;
  candidates: Design[];
}) {
  const update = useApp((s) => s.updatePollDecision);
  const remove = useApp((s) => s.removePollDecision);
  const moveDecision = useApp((s) => s.movePollDecision);
  const designs = useApp((s) => s.designs);
  const patch = (p: Partial<DraftDecision>) => update(decision.id, p);
  const available = candidates.filter((c) => !decision.options.some((o) => o.designId === c.id));

  return (
    <li className="rounded-xl border bg-card p-4 shadow-xs">
      <div className="flex items-start gap-3">
        <span className="mt-1.5 flex size-6 shrink-0 items-center justify-center rounded-full bg-primary text-xs font-semibold text-primary-foreground">
          {index + 1}
        </span>
        <div className="min-w-0 flex-1">
          <Input
            value={decision.question}
            placeholder="The question, e.g. How prominent should pricing be?"
            onChange={(e) => patch({ question: e.target.value })}
            className="h-9 border-transparent px-1.5 text-base font-semibold shadow-none hover:border-input md:text-base"
            aria-label="Question"
          />
          <Input
            value={decision.why}
            placeholder="Why it matters (one line)"
            onChange={(e) => patch({ why: e.target.value })}
            className="mt-0.5 h-7 border-transparent px-1.5 text-sm text-muted-foreground shadow-none hover:border-input"
            aria-label="Why it matters"
          />
        </div>
        <div className="flex shrink-0 items-center">
          <Button size="icon-sm" variant="ghost" aria-label="Move up" disabled={index === 0} onClick={() => moveDecision(decision.id, -1)}>
            <ArrowUp />
          </Button>
          <Button
            size="icon-sm"
            variant="ghost"
            aria-label="Move down"
            disabled={index === count - 1}
            onClick={() => moveDecision(decision.id, 1)}
          >
            <ArrowDown />
          </Button>
          <Button size="icon-sm" variant="ghost" aria-label="Remove decision" onClick={() => remove(decision.id)}>
            <Trash2 />
          </Button>
        </div>
      </div>

      <div className="mt-4 grid grid-cols-2 gap-3 lg:grid-cols-4">
        {decision.options.map((o, i) => {
          const d = designs[o.designId];
          return (
            <div key={o.designId} className="flex flex-col overflow-hidden rounded-lg border bg-background">
              <div className="relative">
                {d?.status === "ready" ? (
                  <DesignPreview html={d.html} className="aspect-[4/3] w-full" />
                ) : (
                  <div className="flex aspect-[4/3] items-center justify-center bg-muted text-xs text-muted-foreground">
                    {d ? "Not ready" : "Design deleted"}
                  </div>
                )}
                <span className="absolute top-1.5 left-1.5 rounded bg-background/90 px-1.5 text-[10px] font-semibold shadow-xs">
                  {String.fromCharCode(65 + i)}
                </span>
              </div>
              <div className="flex flex-col gap-1 border-t p-2">
                <Input
                  value={o.label}
                  placeholder="Short label"
                  aria-label="Option label"
                  onChange={(e) =>
                    patch({ options: decision.options.map((x, j) => (j === i ? { ...x, label: e.target.value } : x)) })
                  }
                  className="h-7 text-xs md:text-xs"
                />
                <div className="flex items-center gap-0.5">
                  <span className="truncate text-[11px] text-muted-foreground" title={d?.title}>
                    {d?.title ?? "Missing design"}
                  </span>
                  <span className="ml-auto flex shrink-0">
                    <Button
                      size="icon-xs"
                      variant="ghost"
                      aria-label="Move option left"
                      disabled={i === 0}
                      onClick={() => patch({ options: moveItem(decision.options, i, i - 1) })}
                    >
                      <ArrowLeft />
                    </Button>
                    <Button
                      size="icon-xs"
                      variant="ghost"
                      aria-label="Move option right"
                      disabled={i === decision.options.length - 1}
                      onClick={() => patch({ options: moveItem(decision.options, i, i + 1) })}
                    >
                      <ArrowRight />
                    </Button>
                    <Button
                      size="icon-xs"
                      variant="ghost"
                      aria-label="Remove option"
                      onClick={() => patch({ options: decision.options.filter((_, j) => j !== i) })}
                    >
                      <X />
                    </Button>
                  </span>
                </div>
              </div>
            </div>
          );
        })}

        {decision.options.length < 4 && (
          <DropdownMenu>
            <DropdownMenuTrigger
              disabled={!available.length}
              render={
                <button
                  type="button"
                  className="flex aspect-[4/3] flex-col items-center justify-center gap-1 rounded-lg border border-dashed text-xs text-muted-foreground transition hover:bg-muted/50 hover:text-foreground disabled:opacity-50 disabled:hover:bg-transparent"
                />
              }
            >
              <Plus className="size-4" />
              {available.length ? "Add option" : "No more candidates"}
            </DropdownMenuTrigger>
            <DropdownMenuContent className="w-64">
              <DropdownMenuLabel>From the poll tray</DropdownMenuLabel>
              {available.map((c) => (
                <DropdownMenuItem
                  key={c.id}
                  onClick={() => patch({ options: [...decision.options, { designId: c.id, label: c.title }] })}
                >
                  <span className="truncate">{c.title}</span>
                </DropdownMenuItem>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>
    </li>
  );
}

// ---- publish ----------------------------------------------------------------------------------------------------

function validate(decisions: DraftDecision[], designs: Record<string, Design>): string | null {
  if (!decisions.length) return "Add at least one decision.";
  for (const [i, d] of decisions.entries()) {
    const n = `Decision ${i + 1}`;
    if (!d.question.trim()) return `${n} needs a question.`;
    if (d.options.length < 2) return `${n} needs at least two options.`;
    if (d.options.some((o) => designs[o.designId]?.status !== "ready")) return `${n} has an option that isn't ready.`;
  }
  return null;
}

function PublishBar({
  candidates,
  published,
  onPublished,
}: {
  candidates: Design[];
  published: PublishedPoll | null;
  onPublished: (p: PublishedPoll | null) => void;
}) {
  const decisions = useApp((s) => s.pollDecisions);
  const designs = useApp((s) => s.designs);
  const title = useApp((s) => s.pollTitle);
  const addPublished = useApp((s) => s.addPublishedPoll);
  const showResults = usePollUI((s) => s.showResults);
  const [busy, setBusy] = useState(false);
  const issue = validate(decisions, designs);

  async function publish() {
    setBusy(true);
    try {
      const req: CreatePollRequest = {
        title: title.trim() || "Which direction should we take?",
        intro: "",
        decisions: decisions.map((d) => ({
          id: d.id,
          question: d.question.trim(),
          why: d.why.trim(),
          options: d.options.map((o) => {
            const design = designs[o.designId];
            return {
              id: nanoid(8),
              designId: o.designId,
              label: o.label.trim() || design.title,
              title: design.title,
              rationale: design.rationale,
              html: design.html,
            };
          }),
        })),
      };
      const res = await fetch("/api/polls", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(req),
      });
      if (!res.ok) throw new Error(await readError(res));
      const data = (await res.json()) as CreatePollResponse;
      const poll: PublishedPoll = {
        id: data.id,
        title: req.title,
        createdAt: Date.now(),
        shareUrl: shareUrlFor(data),
        decisions: req.decisions.map((d) => ({
          id: d.id,
          question: d.question,
          options: d.options.map((o) => ({ id: o.id, designId: o.designId, label: o.label, title: o.title })),
        })),
      };
      addPublished(poll);
      onPublished(poll);
      toast.success("Poll published");
    } catch (err) {
      toast.error("Couldn't publish the poll", { description: err instanceof Error ? err.message : String(err) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="border-t bg-muted/30 px-4 py-3 md:px-6">
      {published && (
        <div className="mb-3 flex flex-wrap items-center gap-2 rounded-lg border border-emerald-500/30 bg-emerald-500/5 p-3">
          <Check className="size-4 text-emerald-600" />
          <span className="text-sm font-medium">Published.</span>
          <span className="text-sm text-muted-foreground">Share this link with your stakeholders:</span>
          <ShareLink url={published.shareUrl} className="min-w-0 flex-1 max-md:basis-full md:min-w-64" />
          <Button size="sm" variant="ghost" onClick={() => showResults(published.id)}>
            See results
          </Button>
          <Button size="icon-sm" variant="ghost" aria-label="Dismiss" onClick={() => onPublished(null)}>
            <X />
          </Button>
        </div>
      )}
      <div className="flex items-center gap-3">
        <span className="text-xs text-muted-foreground">
          {issue ??
            `${decisions.length} decision${decisions.length === 1 ? "" : "s"} · ${candidates.length} candidates · snapshots the current HTML`}
        </span>
        <Button className="ml-auto" disabled={!!issue || busy} onClick={publish}>
          {busy ? <Loader2 className="animate-spin" /> : <Send />} Publish poll
        </Button>
      </div>
    </div>
  );
}
