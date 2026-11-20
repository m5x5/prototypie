"use client";
import { useMemo, useState, useSyncExternalStore } from "react";
import { nanoid } from "nanoid";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  Loader2,
  Maximize2,
  MessageSquare,
  Monitor,
  PartyPopper,
  RefreshCw,
  Smartphone,
  Vote as VoteIcon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { DesignPreview } from "@/components/design-preview";
import { cn } from "@/lib/utils";
import {
  readError,
  tallyDecision,
  type Poll,
  type PollDecision,
  type PollOption,
  type Vote,
  type VoteRequest,
} from "@/features/poll/schema";

// ---- voter identity (localStorage, read via useSyncExternalStore to stay hydration-safe) --------------------

const VOTER_KEY = "prototypie:voter";
interface Voter {
  id: string;
  name: string;
}
const listeners = new Set<() => void>();
function subscribe(cb: () => void) {
  listeners.add(cb);
  window.addEventListener("storage", cb);
  return () => {
    listeners.delete(cb);
    window.removeEventListener("storage", cb);
  };
}
function readVoterRaw(): string | null {
  try {
    return localStorage.getItem(VOTER_KEY);
  } catch {
    return null;
  }
}
function saveVoter(v: Voter | null) {
  try {
    if (v) localStorage.setItem(VOTER_KEY, JSON.stringify(v));
    else localStorage.removeItem(VOTER_KEY);
  } catch {
    // private mode etc. - the name just won't be remembered
  }
  listeners.forEach((l) => l());
}
function useVoter(): [Voter | null, boolean] {
  const raw = useSyncExternalStore<string | null | undefined>(subscribe, readVoterRaw, () => undefined);
  const hydrated = raw !== undefined;
  const voter = useMemo(() => {
    if (!raw) return null;
    try {
      const v = JSON.parse(raw) as Voter;
      return v?.id && v?.name ? v : null;
    } catch {
      return null;
    }
  }, [raw]);
  return [voter, hydrated];
}

// ---- player -------------------------------------------------------------------------------------------------

type Choice = { optionId: string; comment: string };
type Viewport = "desktop" | "mobile";

export function PollPlayer({ poll }: { poll: Poll }) {
  const [voter, hydrated] = useVoter();
  const [votes, setVotes] = useState<Vote[]>(poll.votes);
  const [step, setStep] = useState(-1); // -1 intro, 0..n-1 decisions, n done
  const [local, setLocal] = useState<Record<string, Choice>>({});
  const [viewport, setViewport] = useState<Viewport>("desktop");
  const [expanded, setExpanded] = useState<PollOption | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const n = poll.decisions.length;
  const decision = step >= 0 && step < n ? poll.decisions[step] : null;

  /** The voter's choice for a decision: unsaved local edits first, then what they voted before. */
  function choiceFor(d: PollDecision): Choice | null {
    if (local[d.id]) return local[d.id];
    const prev = voter && votes.find((v) => v.voterId === voter.id && v.decisionId === d.id);
    return prev ? { optionId: prev.optionId, comment: prev.comment ?? "" } : null;
  }
  const choice = decision ? choiceFor(decision) : null;
  const setChoice = (patch: Partial<Choice>) => {
    if (!decision) return;
    const base = choiceFor(decision) ?? { optionId: "", comment: "" };
    setLocal((l) => ({ ...l, [decision.id]: { ...base, ...patch } }));
  };

  function go(to: number) {
    setError(null);
    setStep(to);
    window.scrollTo({ top: 0, behavior: "smooth" });
  }

  async function submit() {
    if (!decision || !voter || !choice?.optionId) return;
    setSubmitting(true);
    setError(null);
    try {
      const body: VoteRequest = {
        voterId: voter.id,
        voter: voter.name,
        decisionId: decision.id,
        optionId: choice.optionId,
        comment: choice.comment.trim() || undefined,
      };
      const res = await fetch(`/api/polls/${poll.id}/votes`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
      if (!res.ok) throw new Error(await readError(res));
      const data = (await res.json()) as { votes: Vote[] };
      setVotes(data.votes);
      setLocal((l) => {
        const next = { ...l };
        delete next[decision.id];
        return next;
      });
      go(step + 1);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  }

  async function refreshResults() {
    try {
      const res = await fetch(`/api/polls/${poll.id}/votes`, { cache: "no-store" });
      if (res.ok) setVotes(((await res.json()) as { votes: Vote[] }).votes);
    } catch {
      // keep what we have
    }
  }

  if (!hydrated) return <div className="min-h-dvh flex-1 bg-muted/40" />;

  if (step < 0 || !voter) {
    return <Intro poll={poll} voter={voter} onStart={() => go(0)} />;
  }

  if (!decision) {
    return <Done poll={poll} votes={votes} voter={voter} onReview={() => go(0)} onRefresh={refreshResults} />;
  }

  const cols =
    decision.options.length === 2
      ? "md:grid-cols-2"
      : decision.options.length === 3
        ? "lg:grid-cols-3"
        : "md:grid-cols-2 2xl:grid-cols-4";

  return (
    <div className="flex min-h-dvh flex-1 flex-col bg-muted/40">
      <header className="sticky top-0 z-20 border-b bg-background/85 backdrop-blur">
        <div className="mx-auto flex h-14 max-w-[1600px] items-center gap-4 px-4 sm:px-6">
          <span className="flex min-w-0 items-center gap-2 text-sm font-medium">
            <VoteIcon className="size-4 shrink-0" />
            <span className="truncate">{poll.title}</span>
          </span>
          <Progress current={step} total={n} onJump={(i) => go(i)} />
          <span className="ml-auto hidden shrink-0 text-xs text-muted-foreground sm:block">
            Voting as <span className="font-medium text-foreground">{voter.name}</span>
          </span>
        </div>
      </header>

      <main className="mx-auto w-full max-w-[1600px] flex-1 px-4 pt-8 pb-40 sm:px-6">
        <div className="flex flex-wrap items-end gap-4">
          <div className="min-w-0 flex-1">
            <p className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
              Decision {step + 1} of {n}
            </p>
            <h1 className="mt-1 text-2xl font-semibold tracking-tight text-balance sm:text-3xl">{decision.question}</h1>
            {decision.why && <p className="mt-2 max-w-2xl text-muted-foreground text-pretty">{decision.why}</p>}
          </div>
          <div className="flex rounded-lg border bg-background p-0.5" role="group" aria-label="Preview size">
            {(["desktop", "mobile"] as const).map((v) => (
              <Button
                key={v}
                size="sm"
                variant={viewport === v ? "secondary" : "ghost"}
                aria-pressed={viewport === v}
                onClick={() => setViewport(v)}
              >
                {v === "desktop" ? <Monitor /> : <Smartphone />}
                <span className="capitalize">{v}</span>
              </Button>
            ))}
          </div>
        </div>
        <p className="mt-4 text-sm text-muted-foreground">
          These are live prototypes - click around, scroll, try things. Then choose the one you&apos;d ship.
        </p>

        <div className={cn("mt-5 grid grid-cols-1 gap-5", cols)}>
          {decision.options.map((o, i) => (
            <OptionCard
              key={o.id}
              option={o}
              letter={String.fromCharCode(65 + i)}
              viewport={viewport}
              selected={choice?.optionId === o.id}
              onSelect={() => setChoice({ optionId: o.id })}
              onExpand={() => setExpanded(o)}
            />
          ))}
        </div>
      </main>

      <footer className="fixed inset-x-0 bottom-0 z-20 border-t bg-background/90 backdrop-blur">
        <div className="mx-auto flex max-w-[1600px] flex-col gap-3 px-4 py-3 sm:flex-row sm:items-center sm:px-6">
          <div className="relative min-w-0 flex-1">
            <MessageSquare className="pointer-events-none absolute top-2.5 left-2.5 size-4 text-muted-foreground" />
            <Textarea
              value={choice?.comment ?? ""}
              onChange={(e) => setChoice({ comment: e.target.value })}
              placeholder={choice?.optionId ? "Why this one? (optional)" : "Pick an option above - you can add a comment too"}
              className="max-h-28 min-h-10 resize-none pl-8"
              rows={1}
            />
          </div>
          <div className="flex items-center gap-2">
            {error && <span className="max-w-64 text-xs text-destructive">{error}</span>}
            <Button variant="ghost" onClick={() => go(step - 1)}>
              <ArrowLeft /> Back
            </Button>
            <Button variant="ghost" className="text-muted-foreground" onClick={() => go(step + 1)}>
              Skip
            </Button>
            <Button size="lg" className="px-4" disabled={!choice?.optionId || submitting} onClick={submit}>
              {submitting ? <Loader2 className="animate-spin" /> : null}
              {step === n - 1 ? "Vote & finish" : "Vote & continue"}
              {!submitting && <ArrowRight />}
            </Button>
          </div>
        </div>
      </footer>

      <Dialog open={!!expanded} onOpenChange={(o) => !o && setExpanded(null)}>
        <DialogContent className="flex h-[94vh] w-[96vw] max-w-[96vw] flex-col gap-3 sm:max-w-[96vw]">
          {expanded && (
            <>
              <DialogHeader className="flex-row items-start gap-4 pr-10">
                <div className="min-w-0 flex-1">
                  <DialogTitle className="text-base">{expanded.label}</DialogTitle>
                  <DialogDescription className="mt-1 line-clamp-2">{expanded.rationale || expanded.title}</DialogDescription>
                </div>
                <Button
                  variant={choice?.optionId === expanded.id ? "secondary" : "default"}
                  onClick={() => {
                    setChoice({ optionId: expanded.id });
                    setExpanded(null);
                  }}
                >
                  <Check /> {choice?.optionId === expanded.id ? "Chosen" : "Choose this"}
                </Button>
              </DialogHeader>
              <div className="flex min-h-0 flex-1 justify-center">
                <DesignPreview
                  html={expanded.html}
                  viewport={viewport === "mobile" ? 390 : 1440}
                  interactive
                  className={cn("h-full rounded-lg border", viewport === "mobile" ? "w-[390px] max-w-full" : "w-full")}
                />
              </div>
            </>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}

function Progress({ current, total, onJump }: { current: number; total: number; onJump: (i: number) => void }) {
  return (
    <div className="flex items-center gap-1.5" aria-label={`Decision ${current + 1} of ${total}`}>
      {Array.from({ length: total }, (_, i) => (
        <button
          key={i}
          type="button"
          aria-label={`Go to decision ${i + 1}`}
          onClick={() => onJump(i)}
          className={cn(
            "h-1.5 rounded-full transition-all",
            i === current ? "w-8 bg-primary" : i < current ? "w-4 bg-primary/50" : "w-4 bg-border hover:bg-muted-foreground/40",
          )}
        />
      ))}
    </div>
  );
}

function OptionCard({
  option,
  letter,
  viewport,
  selected,
  onSelect,
  onExpand,
}: {
  option: PollOption;
  letter: string;
  viewport: Viewport;
  selected: boolean;
  onSelect: () => void;
  onExpand: () => void;
}) {
  return (
    <article
      className={cn(
        "flex flex-col overflow-hidden rounded-2xl border bg-card shadow-xs transition",
        selected ? "border-primary ring-2 ring-primary" : "hover:shadow-md",
      )}
    >
      <div className="flex items-center gap-3 px-4 py-3">
        <span
          className={cn(
            "flex size-7 shrink-0 items-center justify-center rounded-full border text-xs font-semibold transition",
            selected ? "border-primary bg-primary text-primary-foreground" : "bg-background",
          )}
        >
          {selected ? <Check className="size-3.5" /> : letter}
        </span>
        <div className="min-w-0 flex-1">
          <h2 className="truncate font-medium" title={option.label}>
            {option.label}
          </h2>
          {option.rationale && (
            <p className="line-clamp-1 text-xs text-muted-foreground" title={option.rationale}>
              {option.rationale}
            </p>
          )}
        </div>
        <Button size="icon-sm" variant="ghost" aria-label={`Expand ${option.label}`} onClick={onExpand}>
          <Maximize2 />
        </Button>
      </div>
      <div className="border-y bg-muted/50">
        <DesignPreview
          html={option.html}
          viewport={viewport === "mobile" ? 390 : 1280}
          interactive
          className={cn("h-[min(62vh,640px)]", viewport === "mobile" ? "mx-auto w-full max-w-[390px]" : "w-full")}
        />
      </div>
      <div className="p-3">
        <Button className="w-full" size="lg" variant={selected ? "default" : "outline"} onClick={onSelect}>
          {selected ? <Check /> : null}
          {selected ? `You picked ${letter}` : `Choose ${letter}`}
        </Button>
      </div>
    </article>
  );
}

// ---- intro + done -------------------------------------------------------------------------------------------

function Intro({ poll, voter, onStart }: { poll: Poll; voter: Voter | null; onStart: () => void }) {
  const [name, setName] = useState("");
  const [editing, setEditing] = useState(false);
  const askName = !voter || editing;
  const n = poll.decisions.length;

  function start(e: React.FormEvent) {
    e.preventDefault();
    if (askName) {
      const trimmed = name.trim();
      if (!trimmed) return;
      saveVoter({ id: voter?.id ?? nanoid(12), name: trimmed });
      setEditing(false);
    }
    onStart();
  }

  return (
    <main className="flex min-h-dvh flex-1 items-center justify-center bg-muted/40 p-4">
      <div className="w-full max-w-xl rounded-3xl border bg-card p-8 shadow-sm sm:p-10">
        <span className="inline-flex items-center gap-1.5 rounded-full border bg-muted/50 px-2.5 py-1 text-xs font-medium text-muted-foreground">
          <VoteIcon className="size-3.5" /> Design decision poll
        </span>
        <h1 className="mt-4 text-3xl font-semibold tracking-tight text-balance">{poll.title}</h1>
        <p className="mt-3 text-muted-foreground text-pretty">
          {poll.intro ||
            `${n} quick decision${n === 1 ? "" : "s"}. For each, you'll see the competing directions as live prototypes - play with them, then pick the one you'd ship.`}
        </p>
        <ol className="mt-6 flex flex-col gap-2">
          {poll.decisions.map((d, i) => (
            <li key={d.id} className="flex gap-3 text-sm">
              <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-muted text-[11px] font-semibold">
                {i + 1}
              </span>
              <span>{d.question}</span>
            </li>
          ))}
        </ol>
        <form onSubmit={start} className="mt-8 flex flex-col gap-3">
          {askName ? (
            <>
              <label htmlFor="voter-name" className="text-sm font-medium">
                What&apos;s your name?
              </label>
              <Input
                id="voter-name"
                autoFocus
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="So the designer knows who voted"
                className="h-10 text-base md:text-base"
                maxLength={80}
              />
            </>
          ) : (
            <p className="text-sm text-muted-foreground">
              Welcome back, <span className="font-medium text-foreground">{voter.name}</span>.{" "}
              <button
                type="button"
                className="underline underline-offset-2 hover:text-foreground"
                onClick={() => {
                  setName(voter.name);
                  setEditing(true);
                }}
              >
                Not you?
              </button>
            </p>
          )}
          <Button type="submit" size="lg" className="h-10" disabled={askName && !name.trim()}>
            Start voting <ArrowRight />
          </Button>
        </form>
      </div>
    </main>
  );
}

function Done({
  poll,
  votes,
  voter,
  onReview,
  onRefresh,
}: {
  poll: Poll;
  votes: Vote[];
  voter: Voter;
  onReview: () => void;
  onRefresh: () => Promise<void>;
}) {
  const [refreshing, setRefreshing] = useState(false);
  const voters = new Set(votes.map((v) => v.voterId)).size;
  return (
    <main className="flex-1 bg-muted/40 px-4 py-12 sm:px-6">
      <div className="mx-auto max-w-3xl">
        <div className="text-center">
          <span className="mx-auto flex size-12 items-center justify-center rounded-full bg-primary text-primary-foreground">
            <PartyPopper className="size-5" />
          </span>
          <h1 className="mt-4 text-3xl font-semibold tracking-tight">Thank you, {voter.name}!</h1>
          <p className="mt-2 text-muted-foreground">
            Your votes are in. Here&apos;s where things stand so far ({voters} {voters === 1 ? "person" : "people"}).
          </p>
          <div className="mt-5 flex justify-center gap-2">
            <Button variant="outline" onClick={onReview}>
              <ArrowLeft /> Change my votes
            </Button>
            <Button
              variant="ghost"
              disabled={refreshing}
              onClick={async () => {
                setRefreshing(true);
                await onRefresh();
                setRefreshing(false);
              }}
            >
              <RefreshCw className={cn(refreshing && "animate-spin")} /> Refresh
            </Button>
          </div>
        </div>

        <div className="mt-10 flex flex-col gap-4">
          {poll.decisions.map((d, i) => {
            const t = tallyDecision(d, votes);
            const mine = votes.find((v) => v.voterId === voter.id && v.decisionId === d.id)?.optionId;
            return (
              <section key={d.id} className="rounded-2xl border bg-card p-5 shadow-xs">
                <p className="text-xs font-medium text-muted-foreground">Decision {i + 1}</p>
                <h2 className="mt-0.5 font-semibold">{d.question}</h2>
                <ul className="mt-4 flex flex-col gap-3">
                  {d.options.map((o, j) => {
                    const ot = t.options.find((x) => x.optionId === o.id);
                    const count = ot?.count ?? 0;
                    const pct = t.total ? Math.round((count / t.total) * 100) : 0;
                    const leading = t.leaders.includes(o.id);
                    return (
                      <li key={o.id}>
                        <div className="flex items-baseline gap-2 text-sm">
                          <span className="w-4 text-xs font-semibold text-muted-foreground">
                            {String.fromCharCode(65 + j)}
                          </span>
                          <span className={cn("font-medium", leading && "text-foreground")}>{o.label}</span>
                          {mine === o.id && (
                            <span className="rounded-full bg-primary/10 px-1.5 text-[11px] font-medium text-primary">
                              your pick
                            </span>
                          )}
                          <span className="ml-auto text-xs text-muted-foreground tabular-nums">
                            {count} · {pct}%
                          </span>
                        </div>
                        <div className="mt-1.5 ml-6 h-2 overflow-hidden rounded-full bg-muted">
                          <div
                            className={cn("h-full rounded-full transition-all", leading ? "bg-primary" : "bg-muted-foreground/30")}
                            style={{ width: `${pct}%` }}
                          />
                        </div>
                        {ot?.voters.some((v) => v.comment) && (
                          <ul className="mt-2 ml-6 flex flex-col gap-1">
                            {ot.voters
                              .filter((v) => v.comment)
                              .map((v, k) => (
                                <li key={k} className="text-xs text-muted-foreground">
                                  <span className="font-medium text-foreground">{v.name}:</span> {v.comment}
                                </li>
                              ))}
                          </ul>
                        )}
                      </li>
                    );
                  })}
                </ul>
                {!mine && <p className="mt-3 text-xs text-muted-foreground">You skipped this one.</p>}
              </section>
            );
          })}
        </div>
      </div>
    </main>
  );
}
