"use client";
import { MessageSquareText, Sparkles } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { useApp } from "@/store";
import type { Design } from "@/lib/types";
import { notesFocus, openNotesFor } from "./slice";

const EMPTY: never[] = [];

export function useOpenNotes(designId: string | undefined) {
  const notes = useApp((s) => (designId ? s.notes[designId] : undefined)) ?? EMPTY;
  return notes.filter((n) => !n.resolved);
}

/** Small count badge for design nodes. */
export function NotesBadge({ designId }: { designId: string }) {
  const open = useOpenNotes(designId);
  if (!open.length) return null;
  return (
    <Badge variant="outline" className="shrink-0 gap-1" title={`${open.length} Agentation note(s)`}>
      <MessageSquareText className="size-3" />
      {open.length}
    </Badge>
  );
}

/** Generate variations of a design that address its open Agentation notes. */
export function applyNotes(design: Design, count = 2) {
  const state = useApp.getState();
  const annotations = openNotesFor(state, design.id);
  if (!annotations.length) return;
  const ids = (state.notes[design.id] ?? []).filter((n) => !n.resolved).map((n) => n.id);
  void state
    .generate({
      spaceId: design.spaceId,
      parentId: design.id,
      label: `Apply ${annotations.length} note${annotations.length > 1 ? "s" : ""}: ${design.title}`,
      request: {
        instruction:
          "Redesign ONLY the annotated elements to address their pinned notes. Leave everything else in the design exactly as it is.",
        count,
        focus: notesFocus(annotations),
        scope: "page",
        baseHtml: design.html,
        baseTitle: design.title,
        annotations,
      },
    })
    .then(() => useApp.getState().resolveNotes(design.id, ids));
}

/** List of a design's notes with an "apply" action. Used in the inspector and the fullscreen view. */
export function NotesPanel({ design, compact = false }: { design: Design; compact?: boolean }) {
  const open = useOpenNotes(design.id);
  const removeNote = useApp((s) => s.removeNote);
  if (!open.length) {
    return compact ? null : (
      <p className="text-xs text-muted-foreground">
        Open the design and use the Agentation toolbar (bottom right) to pin notes on its elements.
      </p>
    );
  }
  return (
    <div className="space-y-2">
      <Label className="flex items-center gap-1.5">
        <MessageSquareText className="size-3.5" /> Agentation notes ({open.length})
      </Label>
      <ol className="space-y-1.5">
        {open.map((n, i) => (
          <li key={n.id} className="group rounded-md border bg-muted/30 p-2 text-xs">
            <div className="flex items-start gap-2">
              <span className="font-medium tabular-nums text-muted-foreground">{i + 1}.</span>
              <div className="min-w-0 flex-1">
                <div className="truncate text-muted-foreground" title={n.elementPath}>
                  {n.element}
                </div>
                <div>{n.comment}</div>
              </div>
              <button
                type="button"
                className="text-muted-foreground opacity-0 transition group-hover:opacity-100"
                onClick={() => removeNote(n.id)}
                aria-label="Dismiss note"
              >
                ×
              </button>
            </div>
          </li>
        ))}
      </ol>
      <Button size="sm" className="w-full" onClick={() => applyNotes(design)}>
        <Sparkles /> Apply notes as a new round
      </Button>
    </div>
  );
}
