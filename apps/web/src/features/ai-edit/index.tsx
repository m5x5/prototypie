"use client";
import { Loader2, Undo2, Wand2, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { EditRequest } from "@/lib/ai-edit";
import type { Design } from "@/lib/types";
import { useApp } from "@/store";
import { useUI } from "@/store/ui";
import { notesFocus, openNotesFor } from "@/features/agentation/slice";

/**
 * In-place AI edit of a card (src/lib/ai-edit.ts): the server rewrites the card's html and syncs it to every
 * browser; these are the client actions and the card's busy / undo controls.
 */

/** Start an edit of `design`. Its open Agentation notes become the focus, like "Apply notes". Resolves with an error message, if any. */
export async function startAiEdit(
  design: Design,
  instruction: string,
  opts: { agent?: EditRequest["agent"]; podContext?: EditRequest["podContext"]; components?: EditRequest["components"] } = {},
): Promise<string | undefined> {
  const annotations = openNotesFor(useApp.getState(), design.id);
  const body: EditRequest = {
    designId: design.id,
    instruction,
    agent: opts.agent,
    podContext: opts.podContext?.length ? opts.podContext : undefined,
    components: opts.components?.length ? opts.components : undefined,
    ...(annotations.length ? { annotations, focus: notesFocus(annotations) } : {}),
  };
  useApp.getState().rememberPrompt(instruction);
  try {
    const res = await fetch("/api/edit", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    if (res.ok) return undefined;
    const err = (await res.json().catch(() => ({}))) as { error?: unknown };
    return typeof err.error === "string" ? err.error : `Edit failed (${res.status})`;
  } catch (err) {
    return err instanceof Error ? err.message : String(err);
  }
}

export function cancelAiEdit(designId: string) {
  void fetch(`/api/edit/${encodeURIComponent(designId)}`, { method: "DELETE" }).catch(() => {});
}

/** Restore the content the last AI edit replaced. */
export function undoAiEdit(designId: string) {
  const { designs, updateDesign } = useApp.getState();
  const d = designs[designId];
  const prev = d?.versions?.at(-1);
  if (!d || !prev || d.editing) return;
  const patch: Partial<Design> = {
    html: prev.html,
    title: prev.title,
    rationale: prev.rationale,
    viewport: prev.viewport,
    viewportHeight: prev.viewportHeight,
    versions: d.versions!.slice(0, -1),
    editError: undefined,
  };
  updateDesign(designId, patch);
}

/** Card controls: "Edit with AI", the busy badge while an edit runs, a failed-edit badge and undo. */
export function AiEditActions({ design }: { design: Design }) {
  const d = design;
  const focusPrompt = useUI((s) => s.focusPrompt);
  const updateDesign = useApp((s) => s.updateDesign);
  const ready = d.status === "ready";

  if (d.editing) {
    return (
      <Badge variant="outline" className="gap-1 border-primary/40 text-primary" title={d.editing.instruction}>
        <Loader2 className="animate-spin" /> Bearbeite…
        <button
          type="button"
          aria-label="Cancel AI edit"
          title="Cancel"
          className="-mr-1 rounded-sm text-muted-foreground hover:text-foreground"
          onClick={() => cancelAiEdit(d.id)}
        >
          <X />
        </button>
      </Badge>
    );
  }

  return (
    <>
      <Tooltip>
        <TooltipTrigger
          render={
            <Button
              size="icon-sm"
              variant="ghost"
              aria-label="Edit with AI"
              disabled={!ready}
              onClick={() => focusPrompt(d.id, "edit")}
            />
          }
        >
          <Wand2 className="size-3.5" />
        </TooltipTrigger>
        <TooltipContent>Edit with AI (changes this card in place)</TooltipContent>
      </Tooltip>
      {!!d.versions?.length && (
        <Tooltip>
          <TooltipTrigger
            render={<Button size="icon-sm" variant="ghost" aria-label="Undo AI edit" onClick={() => undoAiEdit(d.id)} />}
          >
            <Undo2 className="size-3.5" />
          </TooltipTrigger>
          <TooltipContent>
            Undo AI edit{d.versions.at(-1)?.instruction ? `: ${d.versions.at(-1)!.instruction!.slice(0, 80)}` : ""}
          </TooltipContent>
        </Tooltip>
      )}
      {d.editError && (
        <Badge
          variant="outline"
          className="cursor-pointer gap-1 border-destructive/40 text-destructive"
          title={`${d.editError} (click to dismiss)`}
          render={<button type="button" onClick={() => updateDesign(d.id, { editError: undefined })} />}
        >
          Edit failed
        </Badge>
      )}
    </>
  );
}
