"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { AlertTriangle, BarChart3, Crown, Loader2, MessageSquare, RefreshCw, Star, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DesignPreview } from "@/components/design-preview";
import { cn } from "@/lib/utils";
import { useApp } from "@/store";
import { usePollUI } from "./client-utils";
import { ShareLink } from "./share-link";
import { readError, type DecisionTally, type Vote } from "./schema";
import type { PublishedPoll } from "./slice";

interface Results {
  votes: Vote[];
  tallies: DecisionTally[];
  at: number;
}
type Loaded = Results | { error: string; at: number };

export function PollResults() {
  const polls = useApp((s) => s.publishedPolls);
  const selectedId = usePollUI((s) => s.resultsPollId);
  const select = usePollUI((s) => s.showResults);
  const poll = polls.find((p) => p.id === selectedId) ?? polls[0];

  if (!poll) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-10 text-center">
        <BarChart3 className="size-6 text-muted-foreground" />
        <p className="font-medium">No published polls yet</p>
        <p className="max-w-sm text-sm text-muted-foreground">
          Publish a poll from the Build tab. Votes from stakeholders show up here.
        </p>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 max-md:flex-col">
      <aside className="w-60 shrink-0 overflow-y-auto border-r bg-muted/30 p-3 max-md:max-h-40 max-md:w-full max-md:border-r-0 max-md:border-b">
        <p className="px-1 pb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">Published</p>
        <ul className="flex flex-col gap-1">
          {polls.map((p) => (
            <li key={p.id}>
              <button
                type="button"
                onClick={() => select(p.id)}
                className={cn(
                  "w-full rounded-md px-2 py-1.5 text-left text-sm transition hover:bg-muted",
                  p.id === poll.id && "bg-background font-medium shadow-xs ring-1 ring-border",
                )}
              >
                <span className="block truncate">{p.title}</span>
                <span className="text-[11px] text-muted-foreground">
                  {new Date(p.createdAt).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })} ·{" "}
                  {p.decisions.length} decisions
                </span>
              </button>
            </li>
          ))}
        </ul>
      </aside>
      <PollResultDetail key={poll.id} poll={poll} />
    </div>
  );
}

function PollResultDetail({ poll }: { poll: PublishedPoll }) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const removePublished = useApp((s) => s.removePublishedPoll);

  const load = useCallback(async (): Promise<Loaded> => {
    try {
      const res = await fetch(`/api/polls/${poll.id}/votes`, { cache: "no-store" });
      if (!res.ok) return { error: await readError(res), at: Date.now() };
      return { ...((await res.json()) as Omit<Results, "at">), at: Date.now() };
    } catch (err) {
      return { error: err instanceof Error ? err.message : String(err), at: Date.now() };
    }
  }, [poll.id]);

  useEffect(() => {
    let alive = true;
    void load().then((r) => alive && setLoaded(r));
    return () => {
      alive = false;
    };
  }, [load]);

  async function refresh() {
    setRefreshing(true);
    setLoaded(await load());
    setRefreshing(false);
  }

  const results = loaded && "votes" in loaded ? loaded : null;
  const voters = results ? new Set(results.votes.map((v) => v.voterId)).size : 0;

  return (
    <div className="min-w-0 flex-1 overflow-y-auto px-4 py-5 md:px-6">
      <div className="flex flex-wrap items-start gap-3">
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-lg font-semibold">{poll.title}</h3>
          <p className="text-sm text-muted-foreground">
            {results ? `${voters} ${voters === 1 ? "person has" : "people have"} voted` : "Loading votes…"}
            {loaded && ` · updated ${new Date(loaded.at).toLocaleTimeString()}`}
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={refresh} disabled={refreshing}>
          <RefreshCw className={cn(refreshing && "animate-spin")} /> Refresh
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Forget this poll"
          title="Remove from this list (the shared link keeps working)"
          onClick={() => {
            removePublished(poll.id);
            toast("Removed from your list", { description: "The shared link keeps working." });
          }}
        >
          <Trash2 />
        </Button>
      </div>
      <ShareLink url={poll.shareUrl} className="mt-3 max-w-xl" />

      {!loaded && (
        <div className="flex items-center gap-2 py-10 text-sm text-muted-foreground">
          <Loader2 className="size-4 animate-spin" /> Loading results…
        </div>
      )}
      {loaded && "error" in loaded && (
        <div className="mt-6 flex gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
          <AlertTriangle className="mt-0.5 size-4 shrink-0" /> {loaded.error}
        </div>
      )}
      {results && (
        <div className="mt-6 flex flex-col gap-6">
          {poll.decisions.map((d, i) => (
            <DecisionResult
              key={d.id}
              poll={poll}
              index={i}
              decision={d}
              tally={results.tallies.find((t) => t.decisionId === d.id)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function DecisionResult({
  poll,
  decision,
  index,
  tally,
}: {
  poll: PublishedPoll;
  decision: PublishedPoll["decisions"][number];
  index: number;
  tally?: DecisionTally;
}) {
  const designs = useApp((s) => s.designs);
  const winners = useApp((s) => s.pollWinners);
  const setWinner = useApp((s) => s.setPollWinner);
  const updateDesign = useApp((s) => s.updateDesign);
  const winnerId = winners[`${poll.id}/${decision.id}`];
  const total = tally?.total ?? 0;

  function toggleWinner(optionId: string, designId: string) {
    const prev = decision.options.find((o) => o.id === winnerId);
    const isOtherWinner = (dId: string) =>
      Object.entries(winners).some(([k, oid]) => {
        if (k === `${poll.id}/${decision.id}`) return false;
        const [pid, did] = k.split("/");
        const p = useApp.getState().publishedPolls.find((x) => x.id === pid);
        return p?.decisions.find((x) => x.id === did)?.options.find((o) => o.id === oid)?.designId === dId;
      });
    if (prev && !isOtherWinner(prev.designId)) updateDesign(prev.designId, { starred: false });
    if (winnerId === optionId) {
      setWinner(poll.id, decision.id, null);
      return;
    }
    setWinner(poll.id, decision.id, optionId);
    if (designs[designId]) {
      updateDesign(designId, { starred: true });
      toast.success("Winner starred on the canvas");
    } else toast("Winner marked", { description: "The original design is no longer on the canvas." });
  }

  return (
    <section className="rounded-xl border bg-card p-4">
      <div className="flex items-baseline gap-2">
        <span className="text-xs font-semibold text-muted-foreground">{index + 1}</span>
        <h4 className="font-semibold">{decision.question}</h4>
        <span className="ml-auto shrink-0 text-xs text-muted-foreground">
          {total} vote{total === 1 ? "" : "s"}
        </span>
      </div>
      <div className="mt-3 grid grid-cols-2 gap-3 lg:grid-cols-4">
        {decision.options.map((o, i) => {
          const t = tally?.options.find((x) => x.optionId === o.id);
          const count = t?.count ?? 0;
          const pct = total ? Math.round((count / total) * 100) : 0;
          const leading = tally?.leaders.includes(o.id);
          const isWinner = winnerId === o.id;
          const design = designs[o.designId];
          return (
            <div
              key={o.id}
              className={cn(
                "flex flex-col overflow-hidden rounded-lg border bg-background",
                isWinner && "ring-2 ring-amber-400",
              )}
            >
              {design?.status === "ready" ? (
                <DesignPreview html={design.html} className="aspect-[4/3] w-full" />
              ) : (
                <div className="flex aspect-[4/3] items-center justify-center bg-muted p-2 text-center text-xs text-muted-foreground">
                  {o.title}
                </div>
              )}
              <div className="flex flex-1 flex-col gap-2 border-t p-2.5">
                <div className="flex items-center gap-1.5">
                  <span className="text-[10px] font-semibold text-muted-foreground">{String.fromCharCode(65 + i)}</span>
                  <span className="truncate text-sm font-medium" title={o.label}>
                    {o.label}
                  </span>
                  {leading && total > 0 && (
                    <Badge variant="secondary" className="ml-auto shrink-0">
                      <Crown /> Leading
                    </Badge>
                  )}
                </div>
                <div>
                  <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                    <div
                      className={cn("h-full rounded-full transition-all", leading ? "bg-primary" : "bg-muted-foreground/40")}
                      style={{ width: `${pct}%` }}
                    />
                  </div>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {count} vote{count === 1 ? "" : "s"} · {pct}%
                  </p>
                </div>
                {t && t.voters.length > 0 && (
                  <ul className="flex flex-col gap-1.5 text-xs">
                    {t.voters.map((v, j) => (
                      <li key={j}>
                        <span className="font-medium">{v.name}</span>
                        {v.comment && (
                          <span className="mt-0.5 flex gap-1 text-muted-foreground">
                            <MessageSquare className="mt-0.5 size-3 shrink-0" />
                            <span className="break-words">{v.comment}</span>
                          </span>
                        )}
                      </li>
                    ))}
                  </ul>
                )}
                <Button
                  size="sm"
                  variant={isWinner ? "secondary" : "outline"}
                  className="mt-auto"
                  onClick={() => toggleWinner(o.id, o.designId)}
                >
                  <Star className={cn(isWinner && "fill-amber-400 text-amber-400")} />
                  {isWinner ? "Winner" : "Mark winner"}
                </Button>
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}
