"use client";
import { useEffect, useRef, useState } from "react";
import { AlertTriangle, Boxes, Loader2, Plus, RotateCcw, Wand2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import { useApp } from "@/store";
import { useDraft } from "@/store/use-draft";
import type { SpokeComponent, SpokesResponse } from "@/app/api/spokes/route";
import { launchLane, spokeRowOrigins, useSpokesUI } from "./lib";
import { appendDictation, VoiceButton } from "./voice";

/** Small segmented 1..max picker. */
export function CountPicker({
  value,
  onChange,
  max = 6,
  label = "Directions",
}: {
  value: number;
  onChange: (n: number) => void;
  max?: number;
  label?: string;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="text-xs text-muted-foreground">{label}</span>
      <div role="radiogroup" aria-label={label} className="flex rounded-lg border p-0.5">
        {Array.from({ length: max }, (_, i) => i + 1).map((n) => (
          <button
            key={n}
            type="button"
            role="radio"
            aria-checked={value === n}
            onClick={() => onChange(n)}
            className={cn(
              "h-6 w-6 max-md:h-10 max-md:w-10 rounded-md text-xs tabular-nums text-muted-foreground transition-colors hover:text-foreground",
              value === n && "bg-primary text-primary-foreground hover:text-primary-foreground",
            )}
          >
            {n}
          </button>
        ))}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ block of clay */

export function ClayDialog() {
  const open = useSpokesUI((s) => s.clayOpen);
  const setOpen = useSpokesUI((s) => s.setClayOpen);
  const activeSpaceId = useApp((s) => s.activeSpaceId);
  // Local `drafts` key: the brief survives a reload; cleared on submit.
  const [brief, setBrief] = useDraft("clay:brief");
  const base = useRef("");

  const submit = () => {
    const instruction = brief.trim();
    if (!instruction) return;
    void launchLane(
      {
        spaceId: activeSpaceId,
        kind: "page",
        label: "Block of clay",
        request: {
          instruction: `${instruction}\n\nThis is a rough first "block of clay": a complete, plausible full page with every section a real version would need. Favour breadth and structure over polish; the designer will refine it component by component.`,
          count: 1,
          scope: "page",
        },
      },
      { type: "clay", name: "Block of clay" },
    );
    toast("Shaping a block of clay…", { description: "It appears on the canvas as soon as it's ready." });
    setBrief("");
    setOpen(false);
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>New page</DialogTitle>
          <DialogDescription>
            Get a rough full-page prototype to break apart into spokes. Don&apos;t over-specify; it&apos;s clay.
          </DialogDescription>
        </DialogHeader>
        <div className="relative">
          <Textarea
            autoFocus
            rows={5}
            value={brief}
            onChange={(e) => setBrief(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) submit();
            }}
            placeholder="e.g. Landing page for a climbing gym: hero, class schedule, pricing tiers, coach profiles, FAQ"
            className="min-h-28 pr-10"
          />
          <VoiceButton
            className="absolute right-1.5 top-1.5"
            onStart={() => (base.current = brief)}
            onText={(t) => setBrief(appendDictation(base.current, t))}
          />
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => setOpen(false)}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={!brief.trim()}>
            <Wand2 /> Generate page
            <kbd className="ml-1 text-xs md:text-[10px] opacity-60">⌘↵</kbd>
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/* ------------------------------------------------------------------ break into spokes */

type Analysis =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; components: SpokeComponent[] };

// Per-hub cache so reopening the dialog doesn't re-ask the agent.
const analysisCache = new Map<string, SpokeComponent[]>();

async function analyse(html: string, title: string, signal: AbortSignal): Promise<SpokeComponent[]> {
  const res = await fetch("/api/spokes", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ html, title }),
    signal,
  });
  let data: SpokesResponse | undefined;
  try {
    data = (await res.json()) as SpokesResponse;
  } catch {
    throw new Error(`Request failed (${res.status})`);
  }
  if ("error" in data) throw new Error(data.error);
  if (!res.ok) throw new Error(`Request failed (${res.status})`);
  return data.components;
}

// Pre-select the first three: the agent lists the most impactful first.
function defaultChecked(components: SpokeComponent[]): Record<string, boolean> {
  return Object.fromEntries(components.slice(0, 3).map((_, i) => [`ai-${i}`, true]));
}

export function SpokesDialog() {
  const hubId = useSpokesUI((s) => s.spokesFor);
  const setFor = useSpokesUI((s) => s.setSpokesFor);
  const hub = useApp((s) => (hubId ? s.designs[hubId] : undefined));
  return (
    <Dialog open={!!hub} onOpenChange={(o) => !o && setFor(null)}>
      <DialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-xl">
        {hub && <SpokesPicker key={hub.id} hubId={hub.id} onDone={() => setFor(null)} />}
      </DialogContent>
    </Dialog>
  );
}

interface SpokePick {
  key: string;
  name: string;
  description: string;
  selector?: string;
  custom?: boolean;
}

function SpokesPicker({ hubId, onDone }: { hubId: string; onDone: () => void }) {
  const hub = useApp((s) => s.designs[hubId]);
  const [attempt, setAttempt] = useState(0);
  const [analysis, setAnalysis] = useState<Analysis>(() => {
    const cached = analysisCache.get(hubId);
    return cached ? { status: "ready", components: cached } : { status: "loading" };
  });
  const [custom, setCustom] = useState<SpokePick[]>([]);
  const [checked, setChecked] = useState<Record<string, boolean>>(() =>
    defaultChecked(analysisCache.get(hubId) ?? []),
  );
  const [draft, setDraft] = useState("");
  const [count, setCount] = useState(3);
  const [direction, setDirection] = useState("");

  const html = hub?.html ?? "";
  const title = hub?.title ?? "";
  useEffect(() => {
    if (analysisCache.has(hubId) && attempt === 0) return;
    const ctrl = new AbortController();
    analyse(html, title, ctrl.signal)
      .then((components) => {
        analysisCache.set(hubId, components);
        setAnalysis({ status: "ready", components });
        setChecked((c) => ({ ...defaultChecked(components), ...c }));
      })
      .catch((err: unknown) => {
        if (ctrl.signal.aborted) return;
        setAnalysis({ status: "error", message: err instanceof Error ? err.message : String(err) });
      });
    return () => ctrl.abort();
  }, [hubId, html, title, attempt]);

  if (!hub) return null;

  const aiPicks: SpokePick[] =
    analysis.status === "ready"
      ? analysis.components.map((c, i) => ({ key: `ai-${i}`, name: c.name, description: c.description, selector: c.selector }))
      : [];
  const all = [...aiPicks, ...custom];
  const picked = all.filter((p) => checked[p.key]);

  const addCustom = () => {
    const text = draft.trim();
    if (!text) return;
    const key = `custom-${Date.now()}`;
    const [name, ...rest] = text.split(/\s*[:–—-]\s+/);
    setCustom((c) => [...c, { key, name: name.slice(0, 40), description: rest.join(" - ") || text, custom: true }]);
    setChecked((c) => ({ ...c, [key]: true }));
    setDraft("");
  };

  const launch = () => {
    if (!picked.length) return;
    const state = useApp.getState();
    const origins = spokeRowOrigins(state, hub, picked.length, count);
    picked.forEach((p, i) => {
      const focus = `${p.name}: ${p.description}${p.selector ? ` (in the HTML: ${p.selector})` : ""}`;
      void launchLane(
        {
          spaceId: hub.spaceId,
          parentId: hub.id,
          kind: "spoke",
          origin: origins[i],
          label: `Spoke · ${p.name}`,
          request: {
            baseHtml: hub.html,
            baseTitle: hub.title,
            instruction: direction.trim() || `Explore ${count} meaningfully different takes on the ${p.name}.`,
            count,
            focus,
            scope: "component",
          },
        },
        { type: "spoke", name: p.name, focus, hubId: hub.id },
      );
    });
    toast(`Exploring ${picked.length} spoke${picked.length === 1 ? "" : "s"} in parallel`, {
      description: "Results join the review queue as each one lands.",
    });
    onDone();
  };

  return (
    <>
      <DialogHeader>
        <DialogTitle className="flex items-center gap-2">
          <Boxes className="size-4" /> Break into spokes
        </DialogTitle>
        <DialogDescription>
          Pick the pieces of <span className="font-medium text-foreground">{hub.title}</span> to iterate on in
          isolation. Each one runs as its own parallel lane.
        </DialogDescription>
      </DialogHeader>

      <div className="max-h-[50dvh] space-y-1 overflow-y-auto pr-1">
        {analysis.status === "loading" && (
          <div className="space-y-1.5">
            <div className="flex items-center gap-2 px-1 pb-1 text-xs text-muted-foreground">
              <Loader2 className="size-3.5 animate-spin" /> Looking for components…
            </div>
            {[0, 1, 2, 3].map((i) => (
              <div key={i} className="h-12 animate-pulse rounded-lg bg-muted/60" />
            ))}
          </div>
        )}
        {analysis.status === "error" && (
          <div className="flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-xs">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-destructive" />
            <div className="min-w-0 flex-1">
              <p className="font-medium text-destructive">Couldn&apos;t identify components</p>
              <p className="mt-0.5 break-words text-muted-foreground">{analysis.message}</p>
              <p className="mt-1 text-muted-foreground">You can still add components by hand below.</p>
            </div>
            <Button
              size="xs"
              variant="outline"
              onClick={() => {
                setAnalysis({ status: "loading" });
                setAttempt((a) => a + 1);
              }}
            >
              <RotateCcw /> Retry
            </Button>
          </div>
        )}
        {all.map((p) => (
          <label
            key={p.key}
            className={cn(
              "flex cursor-pointer items-start gap-3 rounded-lg border p-2.5 transition-colors hover:bg-muted/50",
              checked[p.key] && "border-primary/40 bg-primary/5",
            )}
          >
            <Checkbox
              className="mt-0.5"
              checked={!!checked[p.key]}
              onCheckedChange={(v) => setChecked((c) => ({ ...c, [p.key]: !!v }))}
            />
            <span className="min-w-0 flex-1">
              <span className="block text-sm font-medium">{p.name}</span>
              <span className="block text-xs text-muted-foreground">{p.description}</span>
              {p.selector && <code className="mt-1 block truncate text-xs md:text-[10px] text-muted-foreground/80">{p.selector}</code>}
            </span>
            {p.custom && (
              <button
                type="button"
                aria-label={`Remove ${p.name}`}
                className="text-muted-foreground hover:text-foreground"
                onClick={(e) => {
                  e.preventDefault();
                  setCustom((c) => c.filter((x) => x.key !== p.key));
                }}
              >
                <X className="size-3.5" />
              </button>
            )}
          </label>
        ))}
      </div>

      <div className="flex gap-2">
        <Input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addCustom();
            }
          }}
          placeholder="Add a component, e.g. Pricing card: the middle tier"
        />
        <Button variant="outline" onClick={addCustom} disabled={!draft.trim()}>
          <Plus /> Add
        </Button>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="spoke-direction" className="text-xs">
          Direction for every spoke <span className="font-normal text-muted-foreground">(optional)</span>
        </Label>
        <Input
          id="spoke-direction"
          value={direction}
          onChange={(e) => setDirection(e.target.value)}
          placeholder="e.g. more editorial, try bolder type"
        />
      </div>

      <DialogFooter className="items-center sm:justify-between">
        <CountPicker value={count} onChange={setCount} label="Per spoke" />
        <Button onClick={launch} disabled={!picked.length}>
          <Boxes /> Explore {picked.length || ""} spoke{picked.length === 1 ? "" : "s"}
          {picked.length > 0 && <span className="opacity-70">· {picked.length * count} designs</span>}
        </Button>
      </DialogFooter>
    </>
  );
}
