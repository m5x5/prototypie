"use client";
import { useEffect, useRef, useState } from "react";
import { Panel, useReactFlow, useStore } from "@xyflow/react";
import { ArrowUp, ChevronDown, ChevronUp, Combine, History, Layers, Maximize2, PanelsTopLeft, X } from "lucide-react";
import { AGENTS } from "@/components/prompt-composer";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import type { AgentChoice } from "@/lib/types";
import { useApp } from "@/store";
import { useDraft } from "@/store/use-draft";
import { canvasOrder, useUI } from "@/store/ui";
import { createLiveView } from "@/features/live-view/actions";
import { generateFromSources } from "./generate";

/** Ids of the selected design nodes, as one string so the selector stays cheap and stable. */
const selectedKey = (s: { nodes: { id: string; type?: string; selected?: boolean }[] }) =>
  s.nodes
    .filter((n) => n.selected && n.type === "design")
    .map((n) => n.id)
    .join(",");

const MAX_REVISIONS = 3;
const MAX_COMBINATIONS = 4;

/** "each" = revise every selected design on its own; "combine" = merge all of them into new designs. */
type Mode = "each" | "combine";

function parseOpts(raw: string): { revisions: number; mode: Mode } {
  try {
    const o = raw ? (JSON.parse(raw) as { revisions?: unknown; mode?: unknown }) : {};
    const r = typeof o.revisions === "number" && o.revisions >= 1 ? Math.floor(o.revisions) : 1;
    return { revisions: r, mode: o.mode === "combine" ? "combine" : "each" };
  } catch {
    return { revisions: 1, mode: "each" };
  }
}

/**
 * "Pill that Expands": appears at the bottom of the canvas when several designs are selected (shift/⌘-click or
 * shift-drag). At rest it is a slim 44px pill (selected count, one-line input, agent, send) that stays out of the
 * canvas's way; focusing the input expands it upward into the full composer (label + history, textarea, revisions
 * per design, split Revise button with the agent picker). Esc or the chevron collapses it, keeping the text.
 * "Revise each" sends the message to every selected ready design: `revisions` revisions of each, branching off it.
 * "Combine" sends ONE request carrying all selected ready designs as `bases`: `revisions` new designs that merge
 * them, steered by the (optional) message, placed in a row below the selection.
 */
export function SelectionBar() {
  const key = useStore(selectedKey);
  const ids = key ? key.split(",") : [];
  const n = ids.length;
  // Message, revisions and mode are a draft per space (local `drafts`), so a reload keeps them.
  const spaceId = useApp((s) => s.activeSpaceId);
  const [message, setMessage] = useDraft(`selection:${spaceId}`);
  const [optsRaw, setOptsRaw] = useDraft(`selection-opts:${spaceId}`);
  const opts = parseOpts(optsRaw);
  const revisions = opts.revisions;
  const mode = opts.mode;
  const setRevisions = (r: number) => setOptsRaw(r === 1 && mode === "each" ? "" : JSON.stringify({ revisions: r, mode }));
  const setMode = (m: Mode) => setOptsRaw(revisions === 1 && m === "each" ? "" : JSON.stringify({ revisions, mode: m }));
  const [open, setOpen] = useState(false);
  const history = useApp((s) => s.promptHistory);
  const agent = useApp((s) => s.agentChoice);
  const setAgent = useApp((s) => s.setAgentChoice);
  const rf = useReactFlow();
  const areaRef = useRef<HTMLTextAreaElement>(null);
  const oneRef = useRef<HTMLInputElement>(null);
  // Esc hands focus back to the pill input; that one focus must not expand it again.
  const quietFocus = useRef(false);

  // Expanding moves focus from the one-line input into the textarea (after the grow transition starts).
  useEffect(() => {
    if (!open) return;
    const t = setTimeout(() => {
      const ta = areaRef.current;
      if (!ta) return;
      ta.focus();
      ta.setSelectionRange(ta.value.length, ta.value.length);
    }, 60);
    return () => clearTimeout(t);
  }, [open]);

  // A new selection (or none) starts collapsed again (adjusting state during render, not in an effect).
  if (n < 2 && open) setOpen(false);

  if (n < 2) return null;

  const openFullscreen = useUI.getState().openFullscreen;
  // Fullscreen with only the selected (ready) designs, in canvas order; Esc returns with the selection kept.
  const viewable = canvasOrder(ids);
  const view = () => viewable.length && openFullscreen(viewable[0], viewable);
  const clear = () => rf.setNodes((ns) => ns.map((nd) => (nd.selected ? { ...nd, selected: false } : nd)));
  const collapse = () => setOpen(false);
  const combine = mode === "combine";
  const maxCount = combine ? MAX_COMBINATIONS : MAX_REVISIONS;
  const count = Math.min(revisions, maxCount);
  // Combining needs no prompt (it steers, it isn't required); revising each one does.
  const canSend = combine || !!message.trim();
  const sendLabel = combine
    ? `Combine ${n} into ${count} design${count === 1 ? "" : "s"}`
    : `Revise ${n} designs${count > 1 ? ` ×${count}` : ""}`;
  const send = () => {
    const text = message.trim();
    const s = useApp.getState();
    if (combine) {
      const ready = ids.filter((id) => s.designs[id]?.status === "ready");
      if (ready.length < 2) return;
      generateFromSources({ sourceIds: ready, instruction: text, count });
      setMessage("");
      setOpen(false);
      return;
    }
    if (!text) return;
    for (const id of ids) {
      const d = s.designs[id];
      if (!d || d.status !== "ready") continue;
      void s.generate({
        spaceId: d.spaceId,
        parentId: d.id,
        kind: "variation",
        label: `${text.slice(0, 40)}: ${d.title}`,
        request: { instruction: text, count, baseHtml: d.html, baseTitle: d.title, agent: s.agentChoice },
      });
    }
    setMessage("");
    setOpen(false);
  };

  return (
    // From md up: centred in the canvas area left of the floating inspector (INSPECTOR_OVERLAY = 372px).
    <Panel position="bottom-center" className="nodrag nopan !mb-6 w-[min(560px,calc(100%-2rem))] max-md:!mb-[calc(5rem+env(safe-area-inset-bottom))] max-md:w-[calc(100%-1rem)] md:w-[min(560px,calc(100%-2rem-372px))] md:![transform:translateX(calc(-50%-186px))]">
      <div
        role="dialog"
        aria-label="Revise selected designs"
        data-open={open || undefined}
        className={cn(
          "border bg-popover text-popover-foreground shadow-[0_18px_50px_-14px_rgb(0_0_0/0.55)] transition-[border-radius] duration-250",
          open ? "rounded-[14px]" : "rounded-[22px]",
        )}
        onKeyDown={(e) => {
          // Esc collapses the composer (and doesn't also reach the canvas). Menus handle their own Esc first.
          if (e.key === "Escape" && open) {
            e.stopPropagation();
            collapse();
            requestAnimationFrame(() => {
              quietFocus.current = true;
              oneRef.current?.focus({ preventScroll: true });
              quietFocus.current = false;
            });
          }
        }}
      >
        {/* Full composer: grows upward from the pill (0fr -> 1fr rows). */}
        <div
          className={cn(
            "grid transition-[grid-template-rows] duration-300 ease-[cubic-bezier(.2,.8,.2,1)]",
            open ? "grid-rows-[1fr]" : "grid-rows-[0fr]",
          )}
          inert={!open}
        >
          <div className="min-h-0 overflow-hidden">
            <div className="grid gap-2.5 px-3.5 pt-3.5">
              <div role="radiogroup" aria-label="What to do with the selection" className="grid grid-cols-2 gap-0.5 rounded-[10px] bg-muted p-0.5">
                <ModeButton active={!combine} onClick={() => setMode("each")} title="One revision of each selected design">
                  <Layers className="size-3.5" /> Revise each
                </ModeButton>
                <ModeButton active={combine} onClick={() => setMode("combine")} title="New designs that merge the selected ones">
                  <Combine className="size-3.5" /> Combine
                </ModeButton>
              </div>
              <div className="flex min-h-5 items-center gap-2">
                <label htmlFor="multi-instruction" className="text-sm leading-none font-medium">
                  {combine ? `Combine all ${n}: what should the result be?` : `How should all ${n} change?`}
                </label>
                {history.length > 0 && (
                  <DropdownMenu>
                    <DropdownMenuTrigger
                      render={
                        <button
                          type="button"
                          aria-label="Recent prompts"
                          title="Recent prompts"
                          className="ml-auto grid size-6.5 max-md:size-10 place-items-center rounded-md text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-expanded:bg-muted aria-expanded:text-foreground"
                        />
                      }
                    >
                      <History className="size-3.5" />
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end" className="w-72 max-w-[calc(100vw-1.5rem)]">
                      <DropdownMenuGroup>
                        <DropdownMenuLabel>Recent prompts</DropdownMenuLabel>
                        {history.slice(0, 10).map((p) => (
                          <DropdownMenuItem
                            key={p}
                            title={p}
                            onClick={() => {
                              setMessage(p);
                              requestAnimationFrame(() => areaRef.current?.focus());
                            }}
                          >
                            <History />
                            <span className="truncate">{p}</span>
                          </DropdownMenuItem>
                        ))}
                      </DropdownMenuGroup>
                    </DropdownMenuContent>
                  </DropdownMenu>
                )}
              </div>
              <textarea
                id="multi-instruction"
                ref={areaRef}
                rows={3}
                value={message}
                placeholder={
                  combine
                    ? "Optional: steer the mix, e.g. layout of the first, colours of the second…"
                    : "What should change in all of them?"
                }
                className="block w-full resize-none rounded-[10px] border border-input bg-foreground/[0.04] px-2.5 py-2 text-sm max-md:min-h-24 max-md:text-base leading-[1.45] outline-none placeholder:text-muted-foreground focus:border-ring focus:ring-3 focus:ring-ring/50"
                onChange={(e) => setMessage(e.target.value)}
                onKeyDown={(e) => {
                  // Enter (or ⌘/Ctrl+Enter) sends, Shift+Enter is a new line. Not while composing (IME).
                  if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                    e.preventDefault();
                    send();
                  }
                }}
              />
              <div className="grid gap-2">
                <div className="flex items-center justify-between">
                  <label htmlFor="multi-revisions" className="text-[13px] leading-none font-medium">
                    {combine ? "Combinations" : "Revisions per design"}
                  </label>
                  <span className="text-xs text-muted-foreground tabular-nums">{count}</span>
                </div>
                <input
                  id="multi-revisions"
                  type="range"
                  min={1}
                  max={maxCount}
                  value={count}
                  onChange={(e) => setRevisions(Number(e.target.value))}
                  className="m-0 h-3.5 w-full accent-primary max-md:h-8"
                />
              </div>
            </div>
          </div>
        </div>

        {/* The pill row. Open: just the selection chip and the collapse button. */}
        <div className={cn("flex h-11 max-md:h-14 items-center gap-2 pr-1.5 pl-2", open && "justify-between")}>
          <span className="inline-flex h-7.5 shrink-0 items-center gap-1.5 rounded-full bg-muted pr-1 pl-1 text-[12.5px] whitespace-nowrap">
            <i className="grid size-5.5 place-items-center rounded-full bg-primary text-[11px] font-semibold text-primary-foreground not-italic tabular-nums">
              {n}
            </i>
            selected
            <button
              type="button"
              aria-label="Clear selection"
              title="Clear selection"
              onClick={clear}
              className="grid size-5 max-md:size-8 place-items-center rounded-full text-muted-foreground outline-none hover:bg-background/60 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
            >
              <X className="size-3" />
            </button>
          </span>
          {viewable.length > 1 && (
            <button
              type="button"
              aria-label={`View ${viewable.length} selected in fullscreen`}
              title={`View the ${viewable.length} selected in fullscreen (←/→ to step)`}
              onClick={view}
              className="inline-flex h-7.5 max-md:h-10 shrink-0 items-center gap-1 rounded-full px-2 text-[12.5px] text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
            >
              <Maximize2 className="size-3.5" />
              View
            </button>
          )}
          {viewable.length > 1 && !open && (
            <button
              type="button"
              aria-label={`Live view of the ${viewable.length} selected`}
              title="New live view: places the selected cards by reference and always shows each one's main variant"
              onClick={() => createLiveView(viewable)}
              className="inline-flex h-7.5 max-md:h-10 shrink-0 items-center gap-1 rounded-full px-2 text-[12.5px] text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
            >
              <PanelsTopLeft className="size-3.5" />
              Live
            </button>
          )}
          {open ? (
            <button
              type="button"
              aria-label="Collapse (Esc)"
              title="Collapse (Esc)"
              onClick={collapse}
              className="grid size-6.5 max-md:size-10 place-items-center rounded-md text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
            >
              <ChevronDown className="size-4" />
            </button>
          ) : (
            <>
              <input
                ref={oneRef}
                value={message}
                aria-label="Revise selected designs"
                placeholder={combine ? `Combine all ${n}: steer it (optional)` : `Revise all ${n}: what should change?`}
                className="min-w-0 flex-1 bg-transparent text-sm max-md:text-base outline-none placeholder:text-muted-foreground"
                onFocus={() => {
                  if (!quietFocus.current) setOpen(true);
                }}
                onClick={() => setOpen(true)}
                onChange={(e) => {
                  setMessage(e.target.value);
                  setOpen(true);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && !e.nativeEvent.isComposing) {
                    e.preventDefault();
                    send();
                  }
                }}
              />
              <AgentPicker agent={agent} onAgent={setAgent} variant="pill" />
              <button
                type="button"
                aria-label={sendLabel}
                title={`${sendLabel} (↵)`}
                disabled={!canSend}
                onClick={send}
                className="grid size-8 max-md:size-10 shrink-0 place-items-center rounded-full bg-primary text-primary-foreground outline-none hover:bg-primary/80 focus-visible:ring-2 focus-visible:ring-ring active:scale-95 disabled:opacity-50"
              >
                <ArrowUp className="size-4" />
              </button>
            </>
          )}
        </div>

        {open && (
          <div className="px-3.5 pb-3.5">
            <div className="flex">
              <button
                type="button"
                title={combine ? "Combine the selected designs (↵)" : "Send to all selected (↵)"}
                disabled={!canSend}
                onClick={send}
                className="inline-flex h-8 max-md:h-11 flex-1 items-center justify-center gap-1.5 rounded-l-[10px] bg-primary px-2.5 text-sm font-medium whitespace-nowrap text-primary-foreground outline-none hover:bg-primary/80 focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
              >
                {sendLabel}
              </button>
              <AgentPicker agent={agent} onAgent={setAgent} variant="split" />
            </div>
          </div>
        )}
      </div>
    </Panel>
  );
}

function ModeButton({
  active,
  onClick,
  title,
  children,
}: {
  active: boolean;
  onClick: () => void;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={active}
      title={title}
      onClick={onClick}
      className={cn(
        "inline-flex h-7 max-md:h-10 items-center justify-center gap-1.5 rounded-[8px] text-[12.5px] font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring",
        active ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

/** Who works on it: a quiet icon in the pill, the right half of the split button when expanded. */
function AgentPicker({
  agent,
  onAgent,
  variant,
}: {
  agent: AgentChoice;
  onAgent: (a: AgentChoice) => void;
  variant: "pill" | "split";
}) {
  const { label, Icon } = AGENTS[agent];
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <button
            type="button"
            aria-label={`Agent: ${label}. Change agent`}
            title={`${label} works on it. Click to change.`}
            className={cn(
              "inline-flex h-7.5 shrink-0 items-center gap-0.5 outline-none focus-visible:ring-2 focus-visible:ring-ring",
              variant === "pill"
                ? "rounded-full px-1.5 text-muted-foreground hover:bg-muted hover:text-foreground"
                : "h-8 max-md:h-11 max-md:px-3 rounded-r-[10px] border-l border-primary-foreground/20 bg-primary px-2 text-primary-foreground hover:bg-primary/80",
            )}
          />
        }
      >
        <Icon className="size-4" />
        {variant === "pill" ? <ChevronUp className="size-3" /> : <ChevronDown className="size-3.5" />}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" side="top" className="w-56">
        <DropdownMenuGroup>
          <DropdownMenuLabel>Who works on it</DropdownMenuLabel>
          <DropdownMenuRadioGroup value={agent} onValueChange={(v) => onAgent(v as AgentChoice)}>
            {(Object.keys(AGENTS) as AgentChoice[]).map((k) => {
              const a = AGENTS[k];
              return (
                <DropdownMenuRadioItem key={k} value={k} title={a.hint}>
                  <a.Icon className="size-4" />
                  <span className="flex flex-col">
                    {a.label}
                    <small className="text-[11px] text-muted-foreground">{a.hint}</small>
                  </span>
                </DropdownMenuRadioItem>
              );
            })}
          </DropdownMenuRadioGroup>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * Alt/Option + D removes the selected design(s) on the canvas (all of a multi-selection, else the inspector's one).
 * Ignored while typing in a field.
 */
export function DeleteShortcut() {
  const rf = useReactFlow();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.code !== "KeyD" || !e.altKey || e.metaKey || e.ctrlKey || e.shiftKey) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))) return;
      const s = useApp.getState();
      const picked = rf.getNodes().filter((n) => n.selected && n.type === "design").map((n) => n.id);
      const ids = picked.length ? picked : s.selectedId ? [s.selectedId] : [];
      const existing = ids.filter((id) => s.designs[id]);
      if (!existing.length) return;
      e.preventDefault();
      for (const id of existing) s.removeDesign(id);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [rf]);
  return null;
}
