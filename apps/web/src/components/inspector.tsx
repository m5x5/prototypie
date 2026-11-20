"use client";
import { useEffect, useRef, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { Check, Info } from "lucide-react";
import { Popover } from "@base-ui/react/popover";
import { ClaudeMinion } from "@/components/agent-icons";
import { PromptComposer } from "@/components/prompt-composer";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useApp } from "@/store";
import { useUI } from "@/store/ui";
import { NestedInspector } from "@/features/nested";
import { ROOT_ONLY_PROJECTS, atMainCanvas, canCreateIn } from "@/features/nested/root-guard";
import { PollInspector } from "@/features/poll";
import { SpokesInspector } from "@/features/spokes";
import { NotesPanel } from "@/features/agentation/notes";
import { usePodCatalog, toPodContext } from "@/features/pod-mention/catalog";
import { PodContextPanel } from "@/features/pod-mention/mention";
import { toComponentRefs } from "@/features/pod-mention/components";
import { startAiEdit } from "@/features/ai-edit";
import { cn } from "@/lib/utils";
import { GLASS_PANEL } from "@/lib/glass";

/** The selected design's title; clicking it (or Enter/Space) copies the title to the clipboard. */
function CopyTitle({ title }: { title: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(t);
  }, [copied]);
  const copy = () =>
    void navigator.clipboard.writeText(title).then(
      () => setCopied(true),
      () => {},
    );
  return (
    <h2 className="flex min-w-0 items-center gap-1 text-sm font-semibold">
      <Tooltip>
        <TooltipTrigger
          render={
            <button
              type="button"
              onClick={copy}
              aria-label={`Titel kopieren: ${title}`}
              className="min-w-0 cursor-pointer truncate rounded-sm text-left decoration-muted-foreground/50 underline-offset-2 hover:underline focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
            />
          }
        >
          {title}
        </TooltipTrigger>
        <TooltipContent>{copied ? "Kopiert" : "Titel kopieren"}</TooltipContent>
      </Tooltip>
      {copied && <Check aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />}
      <span role="status" className="sr-only">
        {copied ? "Titel kopiert" : ""}
      </span>
    </h2>
  );
}

/** Right-hand panel: prompt the agent for variations of the selected design, or a new design from a brief. */
export function Inspector() {
  const design = useApp((s) => (s.selectedId ? s.designs[s.selectedId] : null)) ?? null;
  const activeSpaceId = useApp((s) => s.activeSpaceId);
  const generate = useApp((s) => s.generate);
  const tick = useUI((s) => s.promptFocusTick);
  const promptTarget = useUI((s) => s.promptTarget);
  // Prompts are preserved: a draft per selected design (or "new") survives reloads, and sent prompts are kept in history.
  const draftKey = design?.id ?? "new";
  const instruction = useApp((s) => s.drafts[draftKey] ?? "");
  const setDraft = useApp((s) => s.setDraft);
  const history = useApp((s) => s.promptHistory);
  const setInstruction = (t: string) => setDraft(draftKey, t);
  const agent = useApp((s) => s.agentChoice);
  const setAgent = useApp((s) => s.setAgentChoice);
  const [count, setCount] = useState(3);
  const ref = useRef<HTMLTextAreaElement>(null);
  // Variations already made from the selected design: the prompt can be addressed to all of them at once.
  const childIds = useApp(
    useShallow((s) =>
      design ? Object.values(s.designs).filter((d) => d.parentId === design.id && d.status === "ready").map((d) => d.id) : [],
    ),
  );
  const [target, setTarget] = useState<"self" | "kids" | "edit">("self");
  const [editError, setEditError] = useState<string | null>(null);
  // @-mentioned pod data and the fields switched off in "Flows into generation"; cleared after sending.
  const { catalog: podCatalog, error: podError } = usePodCatalog();
  const [mentions, setMentions] = useState<string[]>([]);
  const [fieldsOff, setFieldsOff] = useState<Record<string, boolean>>({});
  const podContext = podCatalog && mentions.length ? toPodContext(podCatalog, mentions, fieldsOff) : undefined;
  // #-mentioned component cards, sent as `components` (their current html) with every kind of request.
  const [componentIds, setComponentIds] = useState<string[]>([]);
  const clearPod = () => {
    setMentions([]);
    setFieldsOff({});
    setComponentIds([]);
  };
  const toChildren = target === "kids" && childIds.length > 0;

  // "Edit with AI" on a card opens the composer on its Edit tab (adjusted during render, not in an effect).
  const [seenTick, setSeenTick] = useState(tick);
  if (tick !== seenTick) {
    setSeenTick(tick);
    if (promptTarget === "edit") setTarget("edit");
  }

  useEffect(() => {
    if (tick) ref.current?.focus();
  }, [tick]);

  const canVary = design?.status === "ready";
  const toEdit = target === "edit" && canVary;
  const editing = !!design?.editing;
  const submit = () => {
    if (!canCreateIn(activeSpaceId)) return;
    if (toEdit) {
      const text = instruction.trim();
      if (!text || editing) return;
      setEditError(null);
      void startAiEdit(design!, text, { agent, podContext, components: toComponentRefs(componentIds) }).then((err) => err && setEditError(err));
      setInstruction("");
      clearPod();
      return;
    }
    const components = componentIds.length ? toComponentRefs(componentIds) : undefined;
    if (toChildren) {
      if (!instruction.trim()) return;
      // One revision per existing variation, each branching off that variation.
      const designs = useApp.getState().designs;
      for (const id of childIds) {
        const child = designs[id];
        if (!child) continue;
        void generate({
          spaceId: child.spaceId,
          parentId: child.id,
          kind: "variation",
          label: `${instruction.trim().slice(0, 40)}: ${child.title}`,
          request: { instruction: instruction.trim(), count: 1, baseHtml: child.html, baseTitle: child.title, agent, podContext, components },
        });
      }
      setInstruction("");
      clearPod();
      return;
    }
    if (!instruction.trim() && !canVary) return;
    void generate({
      spaceId: activeSpaceId,
      parentId: canVary ? design!.id : undefined,
      kind: canVary ? "variation" : "page",
      request: {
        instruction: instruction.trim(),
        count,
        baseHtml: canVary ? design!.html : undefined,
        baseTitle: canVary ? design!.title : undefined,
        agent,
        podContext,
        components,
      },
    });
    // Clear the box after sending; the prompt stays reachable in the recent-prompts menu.
    setInstruction("");
    clearPod();
  };

  return (
    // From md up: a glass panel floating over the canvas, just below the header and 16px from the right and bottom.
    // INSPECTOR_OVERLAY (lib/canvas-insets) mirrors its width + insets; keep the two in sync.
    // Phones keep the plain full-screen pane (the max-md overrides undo the glass).
    <aside
      className={cn(
        "flex h-full w-full shrink-0 flex-col md:absolute md:top-1 md:right-4 md:bottom-4 md:z-20 md:h-auto md:w-[340px] md:overflow-hidden md:rounded-2xl",
        GLASS_PANEL,
        "max-md:border-0 max-md:bg-background max-md:shadow-none max-md:backdrop-blur-none max-md:backdrop-saturate-100",
      )}
    >
      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-4 p-4 max-md:pb-28">
          {design && (
            <div className="space-y-2">
              <div className="flex items-center gap-1.5">
                <CopyTitle key={`title-${design.id}`} title={design.title} />
                {design.instruction && (
                  <MadeFrom key={`made-${design.id}`} prompt={design.instruction} onUse={() => setInstruction(design.instruction!)} />
                )}
              </div>
              <Rationale key={design.id} text={design.rationale} />
            </div>
          )}

          {atMainCanvas(activeSpaceId) ? (
            <p className="rounded-lg border border-dashed px-3 py-2.5 text-xs text-muted-foreground">{ROOT_ONLY_PROJECTS}</p>
          ) : (
            <PromptComposer
              ref={ref}
              mode={toEdit ? "edit" : toChildren ? "kids" : canVary ? "self" : "new"}
              childCount={canVary ? childIds.length : 0}
              onTarget={(t) => {
                setTarget(t);
                setEditError(null);
              }}
              value={instruction}
              onChange={setInstruction}
              history={history}
              count={count}
              onCount={setCount}
              agent={agent}
              onAgent={setAgent}
              canSend={toEdit ? !!instruction.trim() && !editing : toChildren ? !!instruction.trim() : !!instruction.trim() || canVary}
              onSend={submit}
              components={{ spaceId: activeSpaceId, mentions: componentIds, onMentions: setComponentIds }}
              pod={{
                catalog: podCatalog,
                error: podError,
                mentions,
                onMentions: (ids) => {
                  setMentions(ids);
                  // Forget switched-off fields of entries that are no longer mentioned.
                  setFieldsOff((o) => Object.fromEntries(Object.entries(o).filter(([k]) => ids.includes(k.slice(0, k.lastIndexOf(":"))))));
                },
              }}
            />
          )}
          {toEdit && editError && <p className="text-xs text-destructive">{editError}</p>}
          <PodContextPanel
            catalog={podCatalog}
            mentions={mentions}
            off={fieldsOff}
            onToggle={(k, on) => setFieldsOff((o) => {
              const next = { ...o };
              if (on) delete next[k];
              else next[k] = true;
              return next;
            })}
          />

          {design?.status === "ready" && <NotesPanel design={design} compact />}
          <SpokesInspector design={design} />
          <NestedInspector design={design} />
          <PollInspector design={design} />
          <Separator />
          <JobList />
        </div>
      </ScrollArea>
    </aside>
  );
}

function JobList() {
  const jobs = useApp((s) => s.jobs);
  const runs = useApp((s) => s.runs);
  const list = Object.values(jobs).sort((a, b) => b.startedAt - a.startedAt).slice(0, 8);
  if (!list.length) return null;
  return (
    <div className="space-y-1.5">
      <Label>Agent runs</Label>
      {list.map((j) => {
        // While Claude Code works on a run (agent queue), its minion scuttles in place of the status dot.
        const claudeWorking = j.status === "running" && runs[j.id]?.request.agent === "claude";
        return (
          <div key={j.id} className="flex items-center gap-2 text-xs">
            {claudeWorking ? (
              <span className="-mr-1 grid size-4 shrink-0 place-items-center" role="img" aria-label="Claude Code is working" title="Claude Code is working">
                <ClaudeMinion working className="h-3 w-4" />
              </span>
            ) : (
              <span
                className={
                  j.status === "running"
                    ? "size-2 animate-pulse rounded-full bg-blue-500"
                    : j.status === "done"
                      ? "size-2 rounded-full bg-emerald-500"
                      : "size-2 rounded-full bg-destructive"
                }
              />
            )}
            <span className="truncate" title={j.error ?? j.label}>
              {j.label || "Variations"}
            </span>
          </div>
        );
      })}
    </div>
  );
}

/** The prompt a design came from, in an info popover next to its title; "Reuse prompt" puts it back in the box. */
function MadeFrom({ prompt, onUse }: { prompt: string; onUse: () => void }) {
  return (
    <Popover.Root>
      <Popover.Trigger
        aria-label="Prompt this design was made from"
        title="Made from…"
        className="shrink-0 text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      >
        <Info className="size-3.5" />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner side="bottom" align="start" sideOffset={6} className="z-50">
          <Popover.Popup className="w-72 space-y-2 rounded-lg bg-popover p-3 text-xs text-popover-foreground shadow-md ring-1 ring-foreground/10 outline-none">
            <div className="font-medium">Made from</div>
            <p className="max-h-60 overflow-y-auto whitespace-pre-wrap text-muted-foreground">{prompt}</p>
            <Popover.Close
              className="font-medium text-muted-foreground hover:text-foreground"
              onClick={onUse}
            >
              Reuse prompt
            </Popover.Close>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}

/** A design's rationale, one line until expanded. */
function Rationale({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  const [clamped, setClamped] = useState(false);
  return (
    <div className="text-xs text-muted-foreground">
      <p
        ref={(el) => {
          if (el && !open) setClamped(el.scrollWidth > el.clientWidth || el.scrollHeight > el.clientHeight + 1);
        }}
        className={open ? "whitespace-pre-wrap" : "truncate"}
      >
        {text}
      </p>
      {(clamped || open) && (
        <button type="button" className="font-medium hover:text-foreground" onClick={() => setOpen((o) => !o)}>
          {open ? "Show less" : "Read more"}
        </button>
      )}
    </div>
  );
}
