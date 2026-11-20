"use client";
import { useEffect, useRef, useState } from "react";
import { Check, ChevronLeft, ChevronRight, Loader2, MessageSquareText } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { DesignPreview } from "@/components/design-preview";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import { useApp } from "@/store";
import { useUI } from "@/store/ui";
import { useAgentationHost } from "@/features/agentation/bridge";
import { NotesPanel } from "@/features/agentation/notes";
import { EditRequestBox } from "@/features/ai-edit/edit-request-box";

/**
 * Full-size, interactive view of one design. This is where Agentation notes get pinned onto a design.
 * Opened with a set (the canvas selection), it steps through those designs: ←/→, the arrows or the title strip.
 */
export function FullscreenDialog() {
  const id = useUI((s) => s.fullscreenId);
  const close = useUI((s) => s.openFullscreen);
  const design = useApp((s) => (id ? s.designs[id] : undefined));
  const setHost = useAgentationHost((s) => s.setContainer);
  const ids = useUI((s) => s.fullscreenSet);
  // Titles of the set, skipping designs deleted meanwhile.
  // Select plain strings: useShallow compares items by identity, so fresh {id, title} objects looped forever.
  const titles = useApp(useShallow((s) => (ids ? ids.map((i) => s.designs[i]?.title ?? null) : [])));
  const steps = ids ? ids.flatMap((i, k) => (titles[k] == null ? [] : [{ id: i, title: titles[k] as string }])) : [];
  const at = steps.findIndex((d) => d.id === id);
  const multi = steps.length > 1 && at >= 0;
  const go = (delta: number) => {
    if (!multi) return;
    const next = steps[(at + delta + steps.length) % steps.length];
    close(next.id, ids ?? undefined);
  };

  // Esc in browser fullscreen (app-shell toggle) only exits fullscreen. Remember whether it was on when Esc went
  // down (capture phase, before the dialog sees it) and when it last changed, since the browser may exit first.
  const fs = useRef({ escWhileFull: false, changedAt: -Infinity });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") fs.current.escWhileFull = !!document.fullscreenElement;
    };
    const onChange = () => (fs.current.changedAt = performance.now());
    window.addEventListener("keydown", onKey, true);
    document.addEventListener("fullscreenchange", onChange);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      document.removeEventListener("fullscreenchange", onChange);
    };
  }, []);
  const escLeavesFullscreen = () =>
    !!document.fullscreenElement || fs.current.escWhileFull || performance.now() - fs.current.changedAt < 500;

  // ←/→ step through the set, unless typing (Agentation's note box, the notes panel).
  useEffect(() => {
    if (!multi) return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.key !== "ArrowLeft" && e.key !== "ArrowRight") || e.metaKey || e.ctrlKey || e.altKey) return;
      const t = e.target as HTMLElement | null;
      if (t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))) return;
      e.preventDefault();
      const s = useUI.getState();
      const list = (s.fullscreenSet ?? []).filter((i) => useApp.getState().designs[i]);
      const i = list.indexOf(s.fullscreenId ?? "");
      if (i < 0) return;
      s.openFullscreen(list[(i + (e.key === "ArrowRight" ? 1 : -1) + list.length) % list.length], s.fullscreenSet ?? undefined);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [multi]);

  return (
    <Dialog
      open={!!design}
      onOpenChange={(o, details) => {
        // In browser fullscreen, Esc belongs to the browser: it only leaves fullscreen, the full view stays open.
        if (!o && details.reason === "escape-key" && escLeavesFullscreen()) return;
        if (!o) {
          close(null);
          setHost(null);
        }
      }}
    >
      <DialogContent
        // Agentation must render inside the modal's focus boundary to be usable here.
        ref={setHost}
        data-fullscreen-dialog
        // Focus the dialog itself: the preview's iframe mounts with the dialog now, and focus inside it would keep
        // Esc (and ←/→) from reaching the dialog.
        initialFocus={() => document.querySelector<HTMLElement>("[data-fullscreen-dialog]")}
        className="inset-0 top-0 left-0 flex h-dvh w-screen max-w-none translate-x-0 translate-y-0 flex-col gap-3 rounded-none bg-background ring-0 sm:max-w-none max-md:pb-[max(1rem,env(safe-area-inset-bottom))] max-md:[&>[data-slot=dialog-close]]:size-10"
      >
        <DialogHeader>
          <DialogTitle className={cn(multi && "flex items-center gap-2 pr-10")}>
            {multi && (
              <span className="inline-flex shrink-0 items-center gap-0.5">
                <Button size="icon-sm" variant="ghost" className="max-md:size-10" aria-label="Previous design (←)" title="Previous (←)" onClick={() => go(-1)}>
                  <ChevronLeft className="size-4" />
                </Button>
                <span className="min-w-10 text-center text-xs font-normal text-muted-foreground tabular-nums" aria-live="polite">
                  {at + 1} / {steps.length}
                </span>
                <Button size="icon-sm" variant="ghost" className="max-md:size-10" aria-label="Next design (→)" title="Next (→)" onClick={() => go(1)}>
                  <ChevronRight className="size-4" />
                </Button>
              </span>
            )}
            <span className="truncate">{design?.title}</span>
          </DialogTitle>
          <DialogDescription>{design?.rationale}</DialogDescription>
          {multi && (
            <nav aria-label="Designs in this set" className="flex gap-1 overflow-x-auto pb-0.5 [scrollbar-width:none]">
              {steps.map((d, i) => (
                <button
                  key={d.id}
                  type="button"
                  aria-current={i === at || undefined}
                  title={d.title}
                  onClick={() => close(d.id, ids ?? undefined)}
                  className={cn(
                    "inline-flex h-7 max-md:h-10 max-w-56 shrink-0 items-center gap-1.5 rounded-full border px-2.5 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    i === at ? "border-primary bg-primary text-primary-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground",
                  )}
                >
                  <span className="tabular-nums opacity-70">{i + 1}</span>
                  <span className="truncate">{d.title}</span>
                </button>
              ))}
            </nav>
          )}
        </DialogHeader>
        {design && (
          <div className="flex min-h-0 flex-1 gap-4 max-md:flex-col max-md:overflow-y-auto">
            <DesignPreview
              key={design.id}
              html={design.html}
              viewport={design.viewport ?? 1280}
              screenHeight={design.viewportHeight}
              interactive
              fullscreenTarget
              designId={design.id}
              // Phone-width designs render at native size, centred; wide ones fill the space. A fixed-size screen
              // keeps its ratio: portrait fits the height (never above native size), landscape fills the width.
              className={
                design.viewportHeight
                  ? design.viewportHeight > (design.viewport ?? 1280)
                    ? "mx-auto min-h-0 shrink-0 self-center rounded-md border"
                    : "min-h-0 flex-1 self-start rounded-md border"
                  : (design.viewport ?? 1280) < 600
                    ? "mx-auto min-h-0 shrink-0 rounded-md border"
                    : "min-h-0 flex-1 rounded-md border"
              }
              style={
                design.viewportHeight
                  ? design.viewportHeight > (design.viewport ?? 1280)
                    ? { height: "100%", maxHeight: design.viewportHeight }
                    : { maxWidth: design.viewport, maxHeight: "100%" }
                  : (design.viewport ?? 1280) < 600
                    ? { width: design.viewport }
                    : undefined
              }
            />
            <aside className="w-72 shrink-0 space-y-3 overflow-y-auto max-md:w-full max-md:overflow-visible">
              {/* Keyed per design: stepping through a set gives each design its own note. */}
              <DesignNotes key={design.id} designId={design.id} />
              <EditRequestBox key={design.id} design={design} />
              <p className="flex items-start gap-1.5 text-xs text-muted-foreground">
                <MessageSquareText className="mt-0.5 size-3.5 shrink-0" />
                Use the Agentation toolbar to click elements in the design and leave notes. Then apply them as a new round.
              </p>
              <NotesPanel design={design} compact />
            </aside>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

const SAVE_DELAY_MS = 600;

/** The designer's own note on a design: autosaves (debounced) into the design, so it syncs via /api/state. */
function DesignNotes({ designId }: { designId: string }) {
  const stored = useApp((s) => s.designs[designId]?.notes ?? "");
  const [text, setText] = useState(stored);
  // Typed but not saved yet: a remote change must not overwrite it.
  const [dirty, setDirty] = useState(false);
  const pending = useRef<{ timer: ReturnType<typeof setTimeout>; value: string } | null>(null);
  const saving = dirty;

  // A change from elsewhere (another browser) shows up as long as nothing is waiting to be saved here.
  const [seen, setSeen] = useState(stored);
  if (stored !== seen) {
    setSeen(stored);
    if (!dirty) setText(stored);
  }

  const save = (value: string) => {
    pending.current = null;
    setDirty(false);
    useApp.getState().updateDesign(designId, { notes: value.trim() ? value : undefined });
  };
  // Leaving the design (next/previous, closing) saves what was typed right away.
  useEffect(
    () => () => {
      const p = pending.current;
      if (p) {
        clearTimeout(p.timer);
        save(p.value);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [designId],
  );

  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between">
        <label htmlFor={`notes-${designId}`} className="text-xs font-medium">
          Notiz
        </label>
        <span className="inline-flex items-center gap-1 text-[11px] text-muted-foreground" aria-live="polite">
          {saving ? (
            <>
              <Loader2 className="size-3 animate-spin" /> Speichert…
            </>
          ) : stored ? (
            <>
              <Check className="size-3" /> Gespeichert
            </>
          ) : null}
        </span>
      </div>
      <Textarea
        id={`notes-${designId}`}
        value={text}
        rows={4}
        placeholder="Gedanken zu diesem Design…"
        className="max-h-72 min-h-24 resize-y text-sm"
        onChange={(e) => {
          const value = e.target.value;
          setText(value);
          setDirty(true);
          if (pending.current) clearTimeout(pending.current.timer);
          pending.current = { value, timer: setTimeout(() => save(value), SAVE_DELAY_MS) };
        }}
        onBlur={() => {
          const p = pending.current;
          if (p) {
            clearTimeout(p.timer);
            save(p.value);
          }
        }}
      />
    </div>
  );
}
