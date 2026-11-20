"use client";
import { useCallback, useEffect, useId, useMemo, useRef, useState, type RefObject } from "react";
import { AtSign, Database, FileText, Folder, Lock, Server, Shapes, X } from "lucide-react";
import { Checkbox } from "@/components/ui/checkbox";
import { cn } from "@/lib/utils";
import type { PodAccess, PodCatalog, PodCatalogEntry, PodContextEntry, PodEntryKind } from "@/lib/types";
import { ACCESS_LABEL, KIND_LABEL, fieldKey, matchEntries, sourceOf } from "./catalog";

const KIND_ONE: Record<PodEntryKind, string> = { resource: "Resource", container: "Container", shape: "Shape · type", pod: "Pod" };
const KIND_ICON: Record<PodEntryKind, typeof FileText> = { resource: FileText, container: Folder, shape: Shapes, pod: Server };
/** Tone per kind: chip background + text, readable in light and dark. */
const KIND_TONE: Record<PodEntryKind, string> = {
  resource: "bg-sky-500/12 text-sky-700 dark:text-sky-300",
  container: "bg-amber-500/14 text-amber-700 dark:text-amber-300",
  shape: "bg-emerald-500/12 text-emerald-700 dark:text-emerald-300",
  pod: "bg-violet-500/12 text-violet-700 dark:text-violet-300",
};

export function KindIcon({ kind, className }: { kind: PodEntryKind; className?: string }) {
  const Icon = KIND_ICON[kind];
  return <Icon aria-hidden className={className} />;
}

export function AccessBadge({ access }: { access: PodAccess }) {
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-0.5 rounded px-1.5 text-[10.5px] leading-4 font-semibold whitespace-nowrap",
        access === "live" && "bg-emerald-500/14 text-emerald-700 dark:text-emerald-300",
        access === "401" && "bg-amber-500/16 text-amber-700 dark:text-amber-300",
        (access === "ref" || access === "cache") && "bg-muted text-muted-foreground",
      )}
    >
      {access === "401" && <Lock aria-hidden className="size-2.5" />}
      {ACCESS_LABEL[access]}
    </span>
  );
}

/** Small read-only chip (results, known children). */
export function PodChip({ kind, label, title }: { kind: PodEntryKind; label: string; title?: string }) {
  return (
    <span title={title} className={cn("inline-flex h-5.5 max-w-full items-center gap-1 rounded-md px-1.5 text-[11.5px] font-medium", KIND_TONE[kind])}>
      <KindIcon kind={kind} className="size-3 shrink-0" />
      <span className="truncate">{label}</span>
    </span>
  );
}

/* ---------- the mention state machine for a textarea ---------- */

interface MentionOptions<T extends { id: string }> {
  /** The character that opens the list: "@" (pod data) or "#" (components). */
  trigger: "@" | "#";
  /** Options for the text typed after the trigger, in display order. */
  match: (query: string) => T[];
  value: string;
  onChange: (v: string) => void;
  mentions: string[];
  onMentions: (ids: string[]) => void;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
}

/**
 * Typing the trigger (at the start or after a space) opens a listbox; the text after it filters. ↑/↓ move,
 * Enter/Tab pick, Esc closes. Picking removes "<trigger>query" from the text and adds the option as a chip.
 * Backspace at the very start of the text removes the last chip.
 */
export function useMention<T extends { id: string }>({ trigger, match, value, onChange, mentions, onMentions, textareaRef }: MentionOptions<T>) {
  const [query, setQuery] = useState<string | null>(null);
  const [start, setStart] = useState(-1);
  const [active, setActive] = useState(0);
  const caret = useRef<number | null>(null);
  const listId = useId();
  const options = useMemo(() => (query !== null ? match(query) : []), [match, query]);
  const open = query !== null;
  const pattern = useMemo(() => (trigger === "@" ? /(^|\s)@([^\s@]*)$/ : /(^|\s)#([^\s#]*)$/), [trigger]);

  // Restore the caret after a controlled value change made here.
  useEffect(() => {
    const ta = textareaRef.current;
    if (ta && caret.current !== null) {
      ta.setSelectionRange(caret.current, caret.current);
      caret.current = null;
    }
  }, [value, textareaRef]);

  const close = useCallback(() => {
    setQuery(null);
    setStart(-1);
    setActive(0);
  }, []);

  const detect = useCallback(
    (text: string, pos: number) => {
      const m = pattern.exec(text.slice(0, pos));
      if (!m) return close();
      setQuery((q) => {
        if (q !== m[2]) setActive(0);
        return m[2];
      });
      setStart(pos - m[2].length - 1);
    },
    [close, pattern],
  );

  const pick = useCallback(
    (id: string) => {
      if (start >= 0) {
        const end = start + 1 + (query ?? "").length;
        caret.current = start;
        onChange(value.slice(0, start) + value.slice(end));
      }
      if (!mentions.includes(id)) onMentions([...mentions, id]);
      close();
      textareaRef.current?.focus();
    },
    [start, query, value, onChange, mentions, onMentions, close, textareaRef],
  );

  const remove = useCallback((id: string) => onMentions(mentions.filter((m) => m !== id)), [mentions, onMentions]);

  /** The trigger button: insert the trigger at the caret (with a space before it when needed) and open the list. */
  const openFromButton = useCallback(() => {
    const ta = textareaRef.current;
    if (open) return close();
    const pos = ta?.selectionStart ?? value.length;
    const before = value.slice(0, pos);
    const lead = before && !/\s$/.test(before) ? " " : "";
    caret.current = pos + lead.length + 1;
    onChange(before + lead + trigger + value.slice(pos));
    setQuery("");
    setStart(pos + lead.length);
    setActive(0);
    ta?.focus();
  }, [open, close, value, onChange, textareaRef, trigger]);

  /** Returns true when the key was handled here (the composer must not send then). */
  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (open && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      if (options.length) setActive((a) => (a + (e.key === "ArrowDown" ? 1 : -1) + options.length) % options.length);
      return true;
    }
    if (open && (e.key === "Enter" || e.key === "Tab") && options.length) {
      e.preventDefault();
      pick(options[Math.min(active, options.length - 1)].id);
      return true;
    }
    if (open && e.key === "Escape") {
      e.preventDefault();
      close();
      return true;
    }
    const ta = e.currentTarget;
    if (e.key === "Backspace" && ta.selectionStart === 0 && ta.selectionEnd === 0 && mentions.length) {
      e.preventDefault();
      remove(mentions[mentions.length - 1]);
      return true;
    }
    return false;
  };

  const activeId = open && options.length ? `${listId}-${options[Math.min(active, options.length - 1)].id}` : undefined;
  return { open, query, options, active, listId, activeId, detect, pick, remove, close, openFromButton, onKeyDown, setActive };
}

/** "@" mentions of Solid pod data. */
export function usePodMention({
  catalog,
  ...rest
}: Omit<MentionOptions<PodCatalogEntry>, "trigger" | "match"> & { catalog: PodCatalog | null }) {
  const match = useCallback((q: string) => (catalog ? matchEntries(catalog, q) : []), [catalog]);
  return useMention({ trigger: "@", match, ...rest });
}

export type PodMention = ReturnType<typeof usePodMention>;

/* ---------- chips over the text ---------- */

export function MentionChips({ catalog, mentions, onRemove }: { catalog: PodCatalog | null; mentions: string[]; onRemove: (id: string) => void }) {
  if (!mentions.length) return null;
  return (
    <div className="flex flex-wrap gap-1 px-2 pt-2" aria-label="Mentioned pod data">
      {mentions.map((id) => {
        const e = catalog?.entries.find((x) => x.id === id);
        if (!e) return null;
        return (
          <span key={id} title={sourceOf(catalog!, e)} className={cn("inline-flex h-6 max-w-full items-center gap-1 rounded-md pr-0.5 pl-1.5 text-xs font-medium", KIND_TONE[e.kind])}>
            <KindIcon kind={e.kind} className="size-3 shrink-0" />
            <span className="truncate">{e.label}</span>
            <button
              type="button"
              aria-label={`Remove ${e.label}`}
              onClick={() => onRemove(id)}
              className="grid size-4.5 place-items-center rounded opacity-80 outline-none hover:bg-foreground/10 hover:opacity-100 focus-visible:ring-2 focus-visible:ring-ring"
            >
              <X className="size-3" />
            </button>
          </span>
        );
      })}
    </div>
  );
}

export function MentionButton({ mention, disabled }: { mention: PodMention; disabled?: boolean }) {
  return (
    <button
      type="button"
      aria-label="Mention pod data (@)"
      title="Mention pod data · @"
      aria-expanded={mention.open}
      aria-controls={mention.listId}
      disabled={disabled}
      // Keep the textarea's caret: don't take focus on mouse down.
      onMouseDown={(e) => e.preventDefault()}
      onClick={mention.openFromButton}
      className="grid size-6.5 shrink-0 place-items-center rounded-md text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50 aria-expanded:bg-muted aria-expanded:text-foreground"
    >
      <AtSign className="size-3.5" />
    </button>
  );
}

/* ---------- the listbox ---------- */

export function MentionListbox({ mention, catalog, mentions, error }: { mention: PodMention; catalog: PodCatalog | null; mentions: string[]; error?: string | null }) {
  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!mention.activeId) return;
    document.getElementById(mention.activeId)?.scrollIntoView?.({ block: "nearest" });
  }, [mention.activeId]);
  if (!mention.open) return null;
  let index = -1;
  return (
    <div
      ref={listRef}
      id={mention.listId}
      role="listbox"
      aria-label="Pod data"
      // Clicks inside must not blur the textarea.
      onMouseDown={(e) => e.preventDefault()}
      className="absolute inset-x-0 top-[calc(100%+6px)] z-30 max-h-[380px] overflow-y-auto rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg"
    >
      <div className="mb-1 flex items-center gap-1.5 border-b px-2 pt-1 pb-2 text-xs text-muted-foreground">
        <AtSign className="size-3" />
        {mention.query ? <b className="font-medium text-foreground">{mention.query}</b> : <span>Pod data</span>}
      </div>
      {!catalog && <div className="px-2 py-2.5 text-[12.5px] text-muted-foreground">{error ? `Pod catalog unavailable (${error})` : "Loading pod data…"}</div>}
      {catalog && !mention.options.length && <div className="px-2 py-2.5 text-[12.5px] text-muted-foreground">No pod data found</div>}
      {catalog &&
        (["resource", "container", "shape", "pod"] as PodEntryKind[]).map((kind) => {
          const group = mention.options.filter((e) => e.kind === kind);
          if (!group.length) return null;
          return (
            <div key={kind} role="group" aria-label={KIND_LABEL[kind]}>
              <div className="px-2 pt-1.5 pb-0.5 text-[11.5px] font-medium text-muted-foreground" aria-hidden>
                {KIND_LABEL[kind]}
              </div>
              {group.map((e) => {
                index += 1;
                const i = index;
                const selected = i === Math.min(mention.active, mention.options.length - 1);
                const used = mentions.includes(e.id);
                return (
                  <div
                    key={e.id}
                    id={`${mention.listId}-${e.id}`}
                    role="option"
                    aria-selected={selected}
                    aria-disabled={used || undefined}
                    onMouseEnter={() => mention.setActive(i)}
                    onClick={() => mention.pick(e.id)}
                    className={cn(
                      "grid cursor-pointer grid-cols-[22px_minmax(0,1fr)_auto] items-center gap-x-2 rounded-md px-2 py-1.5",
                      selected && "bg-accent",
                      used && "opacity-55",
                    )}
                  >
                    <span className={cn("row-span-2 grid size-5.5 place-items-center rounded-md", KIND_TONE[e.kind])}>
                      <KindIcon kind={e.kind} className="size-3.5" />
                    </span>
                    <span className="truncate text-[13px] font-medium">{e.label}</span>
                    <AccessBadge access={e.access} />
                    <span className="col-span-2 truncate font-mono text-[11px] text-muted-foreground">{e.path || sourceOf(catalog, e)}</span>
                  </div>
                );
              })}
            </div>
          );
        })}
    </div>
  );
}

/* ---------- "flows into generation" ---------- */

export function PodContextPanel({
  catalog,
  mentions,
  off,
  onToggle,
}: {
  catalog: PodCatalog | null;
  mentions: string[];
  off: Record<string, boolean>;
  onToggle: (key: string, on: boolean) => void;
}) {
  if (!catalog || !mentions.length) return null;
  const entries = mentions.flatMap((id) => catalog.entries.filter((e) => e.id === id));
  const fields = entries.reduce((n, e) => n + e.fields.filter((_, i) => !off[fieldKey(e.id, i)]).length, 0);
  const resources = new Set(entries.flatMap((e) => (e.kind === "resource" ? [e.id] : (e.children ?? []).filter((c) => catalog.entries.find((x) => x.id === c)?.kind === "resource")))).size;
  const shapes = entries.filter((e) => e.kind === "shape").length;
  return (
    <section aria-labelledby="pod-flow-title" className="space-y-2">
      <div className="flex items-center gap-1.5">
        <Database aria-hidden className="size-3.5 text-muted-foreground" />
        <span id="pod-flow-title" className="flex-1 text-xs font-medium text-muted-foreground">
          Flows into generation
        </span>
        <span className="text-[11.5px] text-muted-foreground tabular-nums">
          {fields} field{fields === 1 ? "" : "s"} · {resources} resource{resources === 1 ? "" : "s"}
          {shapes ? ` · ${shapes} shape${shapes === 1 ? "" : "s"}` : ""}
        </span>
      </div>
      {entries.map((e) => (
        <div key={e.id} className="overflow-hidden rounded-lg border">
          <div className="grid grid-cols-[22px_minmax(0,1fr)_auto] items-center gap-x-2 bg-muted/45 px-2.5 py-2">
            <span className={cn("row-span-2 grid size-5.5 place-items-center rounded-md", KIND_TONE[e.kind])}>
              <KindIcon kind={e.kind} className="size-3.5" />
            </span>
            <b className="min-w-0 text-[12.5px] font-semibold break-words">{e.label}</b>
            <AccessBadge access={e.access} />
            <small className="col-span-2 min-w-0 text-[11px] break-words text-muted-foreground">
              {KIND_ONE[e.kind]} · <span className="font-mono">{sourceOf(catalog, e)}</span>
              {e.stand ? ` · ${e.stand}` : ""}
            </small>
          </div>
          <ul className="py-1">
            {e.fields.map((f, i) => {
              const k = fieldKey(e.id, i);
              const on = !off[k];
              return (
                <li key={k} className={cn("grid grid-cols-[16px_minmax(0,108px)_1fr] items-start gap-1.5 px-2.5 py-1 text-xs", !on && "opacity-45")}>
                  <Checkbox checked={on} onCheckedChange={(v) => onToggle(k, v === true)} aria-label={`Include ${f.p}`} className="mt-0.5" />
                  <span className="font-mono text-[11px] break-words text-muted-foreground">{f.p}</span>
                  <span className="min-w-0 break-words">{f.v ?? f.type ?? ""}</span>
                </li>
              );
            })}
            {!!e.children?.filter((c) => c !== e.id).length && (
              <li className="flex flex-wrap gap-1 px-2.5 py-1 pl-8">
                {e.children
                  .map((c) => catalog.entries.find((x) => x.id === c))
                  .filter((c): c is NonNullable<typeof c> => !!c)
                  .map((c) => (
                    <PodChip key={c.id} kind={c.kind} label={c.label} title={sourceOf(catalog, c)} />
                  ))}
              </li>
            )}
          </ul>
          {e.note && (
            <div className="flex items-start gap-1.5 px-2.5 pb-2 text-[11.5px] text-amber-700 dark:text-amber-300">
              <Lock aria-hidden className="mt-0.5 size-3 shrink-0" />
              <span>{e.note}</span>
            </div>
          )}
        </div>
      ))}
    </section>
  );
}

/* ---------- provenance strip on result cards ---------- */

export function PodProvenance({ context }: { context: PodContextEntry[] }) {
  const fields = context.reduce((n, e) => n + e.fields.length, 0);
  return (
    <div className="space-y-1.5 border-t px-3 py-2" aria-label="Data provenance">
      <div className="flex items-center gap-1.5 text-[11.5px] text-muted-foreground">
        <Database aria-hidden className="size-3" />
        <span>
          {fields} field{fields === 1 ? "" : "s"} from the pod
        </span>
      </div>
      <div className="flex flex-wrap gap-1">
        {context.map((e) => (
          <PodChip key={e.id} kind={e.kind} label={e.label} title={`${e.source} · ${ACCESS_LABEL[e.access]}${e.stand ? ` · ${e.stand}` : ""}`} />
        ))}
      </div>
    </div>
  );
}
