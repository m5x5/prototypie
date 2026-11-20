"use client";
import { useState } from "react";
import { Check, Loader2, Send, Undo2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import type { Design } from "@/lib/types";
import { useApp } from "@/store";
import { cancelAiEdit, startAiEdit, undoAiEdit } from "./index";

/**
 * "Änderung anfragen" in the fullscreen side panel: an in-place AI edit of this design (same flow as the
 * composer's Edit tab, routed to the chosen agent). The preview refreshes in place when the edit lands.
 * Mount it with key={design.id} so stepping through a set starts each design fresh.
 */
export function EditRequestBox({ design }: { design: Design }) {
  const d = design;
  const agent = useApp((s) => s.agentChoice);
  const updateDesign = useApp((s) => s.updateDesign);
  const [text, setText] = useState("");
  const [sendError, setSendError] = useState<string | null>(null);
  // "Done" = an edit sent from here finished: it was running, now it isn't, and it left no error.
  const [sent, setSent] = useState(false);
  const [wasEditing, setWasEditing] = useState(!!d.editing);
  const [done, setDone] = useState(false);
  if (!!d.editing !== wasEditing) {
    setWasEditing(!!d.editing);
    if (!d.editing && sent && !d.editError) setDone(true);
  }

  const running = !!d.editing;
  const error = sendError ?? d.editError ?? null;
  const last = d.versions?.at(-1);

  const send = () => {
    const instruction = text.trim();
    if (!instruction || running || d.status !== "ready") return;
    setSendError(null);
    setDone(false);
    setSent(true);
    setText("");
    void startAiEdit(d, instruction, { agent }).then((err) => err && setSendError(err));
  };

  return (
    <section aria-label="Änderung anfragen" className="space-y-2">
      <label htmlFor={`edit-req-${d.id}`} className="text-xs font-medium">
        Änderung anfragen
      </label>
      <div className="relative">
        <Textarea
          id={`edit-req-${d.id}`}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              send();
            }
          }}
          placeholder="z. B. Uhr größer, Softkeys höher setzen"
          disabled={running}
          rows={3}
          className="min-h-20 resize-none pr-10 text-sm"
        />
        <Button
          size="icon-sm"
          className="absolute bottom-1.5 right-1.5"
          aria-label="Änderung senden"
          title="Senden (Enter)"
          disabled={!text.trim() || running || d.status !== "ready"}
          onClick={send}
        >
          <Send className="size-3.5" />
        </Button>
      </div>

      {running && (
        <p className="flex items-center gap-1.5 text-xs text-primary" title={d.editing?.instruction}>
          <Loader2 className="size-3.5 animate-spin" />
          <span className="min-w-0 flex-1 truncate">Bearbeite… {d.editing?.instruction}</span>
          <button
            type="button"
            aria-label="Cancel AI edit"
            title="Abbrechen"
            className="rounded-sm text-muted-foreground hover:text-foreground"
            onClick={() => cancelAiEdit(d.id)}
          >
            <X className="size-3.5" />
          </button>
        </p>
      )}

      {!running && error && (
        <p className="flex items-start gap-1.5 text-xs text-destructive">
          <span className="min-w-0 flex-1">{error}</span>
          <button
            type="button"
            aria-label="Dismiss"
            className="rounded-sm hover:text-foreground"
            onClick={() => {
              setSendError(null);
              if (d.editError) updateDesign(d.id, { editError: undefined });
            }}
          >
            <X className="size-3.5" />
          </button>
        </p>
      )}

      {!running && !error && done && (
        <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <Check className="size-3.5 text-emerald-500" /> Fertig
        </p>
      )}

      {!running && last && (
        <Button size="sm" variant="outline" className="w-full justify-start" onClick={() => { undoAiEdit(d.id); setDone(false); }}>
          <Undo2 className="size-3.5" />
          <span className="min-w-0 truncate">Rückgängig{last.instruction ? `: ${last.instruction}` : ""}</span>
        </Button>
      )}
    </section>
  );
}
