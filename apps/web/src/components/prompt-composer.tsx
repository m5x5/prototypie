"use client";
import { forwardRef, useCallback, useRef, useState } from "react";
import { ArrowUp, History, Layers, PenLine, Wand2 } from "lucide-react";
import { ClaudeIcon, CodexIcon } from "@/components/agent-icons";
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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { brandButtonColors } from "@/components/ui/button";
import type { AgentChoice, PodCatalog } from "@/lib/types";
import { MentionButton, MentionChips, MentionListbox, usePodMention } from "@/features/pod-mention/mention";
import { ComponentButton, ComponentChips, ComponentListbox, useComponentMention } from "@/features/pod-mention/components";

export const AGENTS: Record<AgentChoice, { label: string; hint: string; Icon: typeof ClaudeIcon }> = {
  claude: { label: "Claude", hint: "Queued for Claude Code via the watcher", Icon: ClaudeIcon },
  codex: { label: "Codex", hint: "Codex CLI generates the designs directly", Icon: CodexIcon },
};

const COUNTS = [1, 2, 3, 4, 5, 6];

export interface PromptComposerProps {
  /** "new" = brief for a new design; "self" = vary the selected design; "kids" = revise its variations; "edit" = change the selected design in place. */
  mode: "new" | "self" | "kids" | "edit";
  /** Number of ready variations of the selected design (0 hides the target tabs). */
  childCount: number;
  onTarget: (t: "self" | "kids" | "edit") => void;
  value: string;
  onChange: (v: string) => void;
  history: string[];
  count: number;
  onCount: (n: number) => void;
  agent: AgentChoice;
  onAgent: (a: AgentChoice) => void;
  canSend: boolean;
  onSend: () => void;
  /** @-mention of Solid pod data: "@" or the @ button opens the catalog; picks become chips over the text. */
  pod?: { catalog: PodCatalog | null; error?: string | null; mentions: string[]; onMentions: (ids: string[]) => void };
  /** #-mention of components: "#" or the # button lists the cards of this space and its ancestors; picks become chips. */
  components?: { spaceId: string; mentions: string[]; onMentions: (ids: string[]) => void };
}

const NO_MENTIONS: string[] = [];
const noop = () => {};

/**
 * The inspector's prompt composer ("Send + Agent Avatar"): target tabs joined to one box holding the target chip,
 * history menu, textarea and a bottom bar with a draggable 1-6 count and a round send arrow whose badge picks the agent.
 */
export const PromptComposer = forwardRef<HTMLTextAreaElement, PromptComposerProps>(function PromptComposer(
  { mode, childCount, onTarget, value, onChange, history, count, onCount, agent, onAgent, canSend, onSend, pod, components },
  ref,
) {
  // The textarea is needed here too (caret for @-mentions); forward it as before.
  const inner = useRef<HTMLTextAreaElement | null>(null);
  const setRefs = useCallback(
    (el: HTMLTextAreaElement | null) => {
      inner.current = el;
      if (typeof ref === "function") ref(el);
      else if (ref) ref.current = el;
    },
    [ref],
  );
  const mention = usePodMention({
    catalog: pod?.catalog ?? null,
    value,
    onChange,
    mentions: pod?.mentions ?? NO_MENTIONS,
    onMentions: pod?.onMentions ?? noop,
    textareaRef: inner,
  });
  const comp = useComponentMention({
    spaceId: components?.spaceId ?? "",
    value,
    onChange,
    mentions: components?.mentions ?? NO_MENTIONS,
    onMentions: components?.onMentions ?? noop,
    textareaRef: inner,
  });
  const mentionable = !!pod || !!components;
  // The list that is open right now (at most one: "@" and "#" never match at the same caret).
  const openList = pod && mention.open ? mention : components && comp.open ? comp : null;
  // Varying a design always shows the target tabs (as in the design); "Its variations" is disabled until there are some.
  const tabs = mode !== "new";
  const kids = mode === "kids";
  const edit = mode === "edit";
  const sendLabel =
    mode === "new"
      ? `Generate ${count} direction${count === 1 ? "" : "s"}`
      : kids
        ? `Revise ${childCount} variation${childCount === 1 ? "" : "s"}`
        : edit
          ? "Edit this design in place"
          : `Propose ${count} variation${count === 1 ? "" : "s"}`;
  const AgentIcon = AGENTS[agent].Icon;
  // Recent prompts sit in the bottom bar, right after the variations count.
  const historyMenu = history.length > 0 && (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <button
            type="button"
            aria-label="Recent prompts"
            title="Recent prompts"
            className="grid size-6.5 max-md:size-10 shrink-0 place-items-center rounded-md text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
          />
        }
      >
        <History className="size-3.5" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-72 max-w-[calc(100vw-1.5rem)]">
        <DropdownMenuGroup>
          <DropdownMenuLabel>Recent prompts</DropdownMenuLabel>
          {history.slice(0, 10).map((p) => (
            <DropdownMenuItem key={p} title={p} onClick={() => onChange(p)}>
              <History />
              <span className="truncate">{p}</span>
            </DropdownMenuItem>
          ))}
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );

  return (
    <div aria-label="Prompt">
      {tabs && (
        <div role="group" aria-label="Target" className="relative z-10 flex items-center">
          <TabButton active={mode === "self"} onClick={() => onTarget("self")} title="New variations of this design">
            <PenLine className="size-3.5" /> This design
          </TabButton>
          <TabButton
            active={kids}
            disabled={!childCount}
            onClick={() => onTarget("kids")}
            title={childCount ? "Revise each variation, one revision each" : "No variations of this design yet"}
          >
            <Layers className="size-3.5" /> Its variations
            <span className="rounded-full bg-muted px-1.5 text-[10.5px] tabular-nums">{childCount}</span>
          </TabButton>
          <TabButton active={edit} onClick={() => onTarget("edit")} title="Change this design in place with AI (undo on the card)">
            <Wand2 className="size-3.5" /> Edit
          </TabButton>
        </div>
      )}
      <div
        className={cn(
          "relative rounded-xl border border-input bg-muted/40 transition-colors focus-within:border-ring dark:bg-[#151515]",
          tabs && "-mt-px",
          tabs && (edit ? "rounded-tr-none" : kids ? "" : "rounded-tl-none"),
        )}
      >
        <label htmlFor="instruction" className="sr-only">
          {mode === "new"
            ? "Brief"
            : kids
              ? `What should change in its ${childCount} variations?`
              : edit
                ? "What should change in this design?"
                : "How should it vary?"}
        </label>
        {pod && <MentionChips catalog={pod.catalog} mentions={pod.mentions} onRemove={mention.remove} />}
        {components && <ComponentChips mentions={components.mentions} onRemove={comp.remove} />}
        <textarea
          id="instruction"
          ref={setRefs}
          rows={3}
          value={value}
          placeholder={
            mode === "new"
              ? "Describe what to build, e.g. landing page for a climbing gym…"
              : kids
                ? "What should change in all of them? e.g. use Lucide icons…"
                : edit
                  ? "What should change? e.g. make the save button pill-shaped…"
                  : "How should it vary? e.g. bolder hierarchy, try a dark theme…"
          }
          title={[pod && "@ mentions pod data", components && "# mentions a component"].filter(Boolean).join(" · ") || undefined}
          className="block min-h-15 w-full resize-none bg-transparent px-2.5 pt-2 pb-1 text-[13.5px] leading-snug max-md:min-h-24 max-md:text-base outline-none placeholder:text-muted-foreground"
          role={mentionable ? "combobox" : undefined}
          aria-multiline={mentionable ? true : undefined}
          aria-autocomplete={mentionable ? "list" : undefined}
          aria-controls={openList?.listId}
          aria-expanded={mentionable ? !!openList : undefined}
          aria-activedescendant={openList?.activeId}
          onChange={(e) => {
            onChange(e.target.value);
            const pos = e.target.selectionStart ?? e.target.value.length;
            if (pod) mention.detect(e.target.value, pos);
            if (components) comp.detect(e.target.value, pos);
          }}
          onClick={(e) => {
            const pos = e.currentTarget.selectionStart ?? 0;
            if (pod) mention.detect(e.currentTarget.value, pos);
            if (components) comp.detect(e.currentTarget.value, pos);
          }}
          onBlur={() => {
            if (pod) mention.close();
            if (components) comp.close();
          }}
          onKeyDown={(e) => {
            if (pod && mention.onKeyDown(e)) return;
            if (components && comp.onKeyDown(e)) return;
            // Enter sends, Shift+Enter is a new line (⌘/Ctrl+Enter also sends). Not while composing (IME).
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              if (canSend) onSend();
            }
          }}
        />
        <div className="flex items-center gap-1.5 border-t p-1.5 max-md:flex-wrap">
          {kids || edit ? (
            <span className="px-1 text-xs text-muted-foreground">{kids ? "One revision each" : "Replaces this card"}</span>
          ) : (
            <CountPicker value={count} onChange={onCount} />
          )}
          {historyMenu}
          {pod && <MentionButton mention={mention} />}
          {components && <ComponentButton mention={comp} />}
          <div className="relative ml-auto">
            <Tooltip>
              <TooltipTrigger
                render={
                  <button
                    type="button"
                    aria-label={`${sendLabel} (Enter)`}
                    disabled={!canSend}
                    onClick={onSend}
                    className={cn(
                      "grid size-8 max-md:size-10 place-items-center rounded-full transition-[transform,background-color] outline-none focus-visible:ring-2 active:scale-95 disabled:pointer-events-none disabled:opacity-50",
                      brandButtonColors,
                    )}
                  />
                }
              >
                <ArrowUp className="size-4" />
              </TooltipTrigger>
              <TooltipContent>
                {sendLabel} · ↵
              </TooltipContent>
            </Tooltip>
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <button
                    type="button"
                    aria-label={`Agent: ${AGENTS[agent].label}. Change agent`}
                    title={`${AGENTS[agent].label} works on it`}
                    className="absolute -right-1.5 -bottom-1.5 grid size-4.5 max-md:size-6 place-items-center rounded-full border-2 border-[#151515] bg-muted outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
                  />
                }
              >
                <AgentIcon className="size-2.5" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" side="top" className="w-44">
                <DropdownMenuGroup>
                  <DropdownMenuLabel>Who works on it</DropdownMenuLabel>
                  <DropdownMenuRadioGroup value={agent} onValueChange={(v) => onAgent(v as AgentChoice)}>
                    {(Object.keys(AGENTS) as AgentChoice[]).map((k) => {
                      const { label, hint, Icon } = AGENTS[k];
                      return (
                        <DropdownMenuRadioItem key={k} value={k} title={hint}>
                          <Icon className="size-4" />
                          {label}
                        </DropdownMenuRadioItem>
                      );
                    })}
                  </DropdownMenuRadioGroup>
                </DropdownMenuGroup>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
        {pod && <MentionListbox mention={mention} catalog={pod.catalog} mentions={pod.mentions} error={pod.error} />}
        {components && <ComponentListbox mention={comp} mentions={components.mentions} />}
      </div>
    </div>
  );
});

function TabButton({
  active,
  disabled,
  onClick,
  title,
  children,
}: {
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      disabled={disabled}
      title={title}
      onClick={onClick}
      className={cn(
        "flex h-8 max-md:h-11 flex-1 items-center justify-center gap-1.5 rounded-t-[10px] border border-b-0 text-[12.5px] font-medium outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50",
        active ? "border-input bg-muted/40 text-foreground dark:bg-[#151515]" : "border-transparent text-muted-foreground hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

/**
 * 1-6 picker that also scrubs like a slider: press and drag across it (ew-resize cursor, pointer capture),
 * or use ←/→/Home/End. It looks the same while dragging.
 */
function CountPicker({ value, onChange }: { value: number; onChange: (n: number) => void }) {
  const groupRef = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState(false);

  const pick = (clientX: number) => {
    const buttons = groupRef.current?.querySelectorAll<HTMLButtonElement>("button");
    if (!buttons?.length) return;
    let best = value;
    let bestDist = Infinity;
    buttons.forEach((b, i) => {
      const r = b.getBoundingClientRect();
      const d = Math.abs(clientX - (r.left + r.width / 2));
      if (d < bestDist) {
        bestDist = d;
        best = COUNTS[i];
      }
    });
    if (best !== value) onChange(best);
  };

  return (
    <div
      ref={groupRef}
      role="group"
      aria-label="Variations (drag, click or use arrow keys)"
      title="Drag, click or use ←/→"
      className={cn(
        "inline-flex cursor-ew-resize touch-none gap-px rounded-md bg-foreground/5 p-0.5 select-none",
        dragging && "[&_*]:cursor-ew-resize",
      )}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        try {
          e.currentTarget.setPointerCapture(e.pointerId);
        } catch {
          // No active pointer (e.g. synthetic events): dragging still works while over the row.
        }
        setDragging(true);
        pick(e.clientX);
      }}
      onPointerMove={(e) => {
        if (dragging) pick(e.clientX);
      }}
      onPointerUp={(e) => {
        if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
        setDragging(false);
      }}
      onPointerCancel={() => setDragging(false)}
      onKeyDown={(e) => {
        const next =
          e.key === "ArrowRight" || e.key === "ArrowUp"
            ? Math.min(6, value + 1)
            : e.key === "ArrowLeft" || e.key === "ArrowDown"
              ? Math.max(1, value - 1)
              : e.key === "Home"
                ? 1
                : e.key === "End"
                  ? 6
                  : null;
        if (next === null) return;
        e.preventDefault();
        onChange(next);
        groupRef.current?.querySelectorAll<HTMLButtonElement>("button")[next - 1]?.focus();
      }}
    >
      {COUNTS.map((n) => (
        <button
          key={n}
          type="button"
          aria-pressed={n === value}
          aria-label={`${n} variation${n === 1 ? "" : "s"}`}
          tabIndex={n === value ? 0 : -1}
          onClick={() => onChange(n)}
          className={cn(
            "h-5.5 w-5 max-md:h-10 max-md:w-7 cursor-ew-resize rounded-[5px] text-[11.5px] tabular-nums outline-none focus-visible:ring-2 focus-visible:ring-ring",
            n === value ? "bg-primary font-semibold text-primary-foreground" : "text-muted-foreground hover:text-foreground",
          )}
        >
          {n}
        </button>
      ))}
    </div>
  );
}
