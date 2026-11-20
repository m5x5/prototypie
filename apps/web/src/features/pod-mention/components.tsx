"use client";
import { useCallback, useEffect } from "react";
import { Component, Hash, LayoutTemplate, X } from "lucide-react";
import { cn } from "@/lib/utils";
import type { ComponentRef, Design } from "@/lib/types";
import { useApp, type AppState } from "@/store";
import { ROOT_SPACE_ID } from "@/store/core";
import { useMention } from "./mention";

/**
 * "#" mentions of components: cards from the current space and its ancestor spaces (not the main canvas, unless
 * that is where you are). Components come first: cards flagged `component` (component-scope results, imports, agent
 * cards), plus spokes. Other finished cards follow, so any card can be reused. Titles are shown as they are; the
 * space name is only a hint on cards from a parent space.
 * Mentioned cards travel with the request as `components: [{id, title, html}]`.
 */

export interface ComponentOption {
  id: string;
  title: string;
  component: boolean;
  /** Name of the parent space the card lives in; absent for cards of the current space. */
  from?: string;
}

const isComponent = (d: Design) => !!d.component || d.request?.scope === "component" || d.kind === "spoke";

/** The current space, then its ancestors up to (not including) the main canvas; just the main canvas when you're on it. */
function spaceChain(s: AppState, spaceId: string): string[] {
  if (spaceId === ROOT_SPACE_ID) return [ROOT_SPACE_ID];
  const out: string[] = [];
  for (let cur: string | undefined = spaceId; cur && cur !== ROOT_SPACE_ID && out.length < 20; cur = s.spaces[cur]?.parentSpaceId) out.push(cur);
  return out;
}

export function componentOptions(s: AppState, spaceId: string, query: string): ComponentOption[] {
  const chain = spaceChain(s, spaceId);
  const q = query.trim().toLowerCase();
  const name = (id: string) => (id === ROOT_SPACE_ID ? "Main canvas" : (s.spaces[id]?.name ?? id));
  return Object.values(s.designs)
    .filter((d) => chain.includes(d.spaceId) && d.status === "ready" && !!d.html && (!q || d.title.toLowerCase().includes(q)))
    .map((d) => ({ d, component: isComponent(d), depth: chain.indexOf(d.spaceId) }))
    .sort((a, b) => Number(b.component) - Number(a.component) || a.depth - b.depth || a.d.title.localeCompare(b.d.title))
    .map(({ d, component, depth }) => ({ id: d.id, title: d.title, component, ...(depth > 0 ? { from: name(d.spaceId) } : {}) }));
}

/** What a request carries for the mentioned cards (current html; cards deleted meanwhile drop out). */
export function toComponentRefs(ids: string[]): ComponentRef[] {
  const designs = useApp.getState().designs;
  return ids.flatMap((id) => {
    const d = designs[id];
    return d?.html ? [{ id, title: d.title, html: d.html }] : [];
  });
}

type MentionBase = Omit<Parameters<typeof useMention<ComponentOption>>[0], "trigger" | "match">;

export function useComponentMention({ spaceId, ...rest }: MentionBase & { spaceId: string }) {
  const match = useCallback((q: string) => componentOptions(useApp.getState(), spaceId, q), [spaceId]);
  const mention = useMention({ trigger: "#", match, ...rest });
  // Chips of cards that were deleted meanwhile disappear.
  const gone = useApp((s) => rest.mentions.some((id) => !s.designs[id]));
  const { mentions, onMentions } = rest;
  useEffect(() => {
    if (gone) onMentions(mentions.filter((id) => !!useApp.getState().designs[id]));
  }, [gone, mentions, onMentions]);
  return mention;
}

export type ComponentMention = ReturnType<typeof useComponentMention>;

const TONE = "bg-fuchsia-500/12 text-fuchsia-700 dark:text-fuchsia-300";

export function ComponentChips({ mentions, onRemove }: { mentions: string[]; onRemove: (id: string) => void }) {
  const titles = useApp((s) => mentions.map((id) => s.designs[id]?.title ?? "").join("\u0000"));
  if (!mentions.length) return null;
  const list = titles.split("\u0000");
  return (
    <div className="flex flex-wrap gap-1 px-2 pt-2" aria-label="Mentioned components">
      {mentions.map((id, i) =>
        list[i] ? (
          <span key={id} title={list[i]} className={cn("inline-flex h-6 max-w-full items-center gap-1 rounded-md pr-0.5 pl-1.5 text-xs font-medium", TONE)}>
            <Hash aria-hidden className="size-3 shrink-0" />
            <span className="truncate">{list[i]}</span>
            <button
              type="button"
              aria-label={`Remove ${list[i]}`}
              onClick={() => onRemove(id)}
              className="grid size-4.5 place-items-center rounded opacity-80 outline-none hover:bg-foreground/10 hover:opacity-100 focus-visible:ring-2 focus-visible:ring-ring"
            >
              <X className="size-3" />
            </button>
          </span>
        ) : null,
      )}
    </div>
  );
}

export function ComponentButton({ mention, disabled }: { mention: ComponentMention; disabled?: boolean }) {
  return (
    <button
      type="button"
      aria-label="Mention a component (#)"
      title="Mention a component · #"
      aria-expanded={mention.open}
      aria-controls={mention.listId}
      disabled={disabled}
      onMouseDown={(e) => e.preventDefault()}
      onClick={mention.openFromButton}
      className="grid size-6.5 shrink-0 place-items-center rounded-md text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 aria-expanded:bg-muted aria-expanded:text-foreground"
    >
      <Hash className="size-3.5" />
    </button>
  );
}

export function ComponentListbox({ mention, mentions }: { mention: ComponentMention; mentions: string[] }) {
  useEffect(() => {
    if (!mention.activeId) return;
    document.getElementById(mention.activeId)?.scrollIntoView?.({ block: "nearest" });
  }, [mention.activeId]);
  if (!mention.open) return null;
  const groups = [
    { key: "components", label: "Components", items: mention.options.filter((o) => o.component) },
    { key: "cards", label: "Other cards", items: mention.options.filter((o) => !o.component) },
  ];
  let index = -1;
  return (
    <div
      id={mention.listId}
      role="listbox"
      aria-label="Components"
      onMouseDown={(e) => e.preventDefault()}
      className="absolute inset-x-0 top-[calc(100%+6px)] z-30 max-h-[380px] overflow-y-auto rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg"
    >
      <div className="mb-1 flex items-center gap-1.5 border-b px-2 pt-1 pb-2 text-xs text-muted-foreground">
        <Hash className="size-3" />
        {mention.query ? <b className="font-medium text-foreground">{mention.query}</b> : <span>Components</span>}
      </div>
      {!mention.options.length && <div className="px-2 py-2.5 text-[12.5px] text-muted-foreground">No matching cards in this space</div>}
      {groups.map(({ key, label, items }) =>
        items.length ? (
          <div key={key} role="group" aria-label={label}>
            <div className="px-2 pt-1.5 pb-0.5 text-[11.5px] font-medium text-muted-foreground" aria-hidden>
              {label}
            </div>
            {items.map((o) => {
              index += 1;
              const i = index;
              const selected = i === Math.min(mention.active, mention.options.length - 1);
              const used = mentions.includes(o.id);
              const Icon = o.component ? Component : LayoutTemplate;
              return (
                <div
                  key={o.id}
                  id={`${mention.listId}-${o.id}`}
                  role="option"
                  aria-selected={selected}
                  aria-disabled={used || undefined}
                  title={o.from ? `${o.title} · ${o.from}` : o.title}
                  onMouseEnter={() => mention.setActive(i)}
                  onClick={() => mention.pick(o.id)}
                  className={cn(
                    "grid cursor-pointer grid-cols-[22px_minmax(0,1fr)_auto] items-center gap-x-2 rounded-md px-2 py-1.5",
                    selected && "bg-accent",
                    used && "opacity-55",
                  )}
                >
                  <span className={cn("grid size-5.5 place-items-center rounded-md", o.component ? TONE : "bg-muted text-muted-foreground")}>
                    <Icon aria-hidden className="size-3.5" />
                  </span>
                  <span className="truncate text-[13px] font-medium">{o.title}</span>
                  {o.from ? <span className="max-w-28 truncate text-[11px] text-muted-foreground">{o.from}</span> : <span />}
                </div>
              );
            })}
          </div>
        ) : null,
      )}
    </div>
  );
}
