import type { StateCreator } from "zustand";
import type { AppState } from "@/store";

/** The parts of an Agentation annotation we keep, keyed to the design whose iframe it was made in. */
export interface DesignNote {
  id: string; // Agentation annotation id
  designId: string;
  comment: string;
  element: string; // human-readable element name, e.g. 'button "Start trial"'
  elementPath: string; // CSS-ish path inside the design document
  selectedText?: string;
  timestamp: number;
  resolved?: boolean;
}

export interface AgentationSlice {
  /** designId -> notes */
  notes: Record<string, DesignNote[]>;
  upsertNote: (n: DesignNote) => void;
  removeNote: (id: string) => void;
  resolveNotes: (designId: string, ids?: string[]) => void;
}

export const createAgentationSlice: StateCreator<AppState, [], [], AgentationSlice> = (set) => ({
  notes: {},
  upsertNote: (n) =>
    set((s) => {
      const list = (s.notes[n.designId] ?? []).filter((x) => x.id !== n.id);
      return { notes: { ...s.notes, [n.designId]: [...list, n].sort((a, b) => a.timestamp - b.timestamp) } };
    }),
  removeNote: (id) =>
    set((s) => ({
      notes: Object.fromEntries(Object.entries(s.notes).map(([k, v]) => [k, v.filter((x) => x.id !== id)])),
    })),
  resolveNotes: (designId, ids) =>
    set((s) => ({
      notes: {
        ...s.notes,
        [designId]: (s.notes[designId] ?? []).map((x) => (!ids || ids.includes(x.id) ? { ...x, resolved: true } : x)),
      },
    })),
});

/** Open (unresolved) notes for a design, in the shape VariationRequest.annotations expects. */
export function openNotesFor(state: Pick<AgentationSlice, "notes">, designId: string) {
  return (state.notes[designId] ?? [])
    .filter((n) => !n.resolved)
    .map((n) => ({
      element: n.element && n.elementPath ? `${n.element} (at \`${n.elementPath}\`)` : n.element || n.elementPath,
      comment: n.selectedText ? `${n.comment} (re: "${n.selectedText}")` : n.comment,
    }));
}

/** VariationRequest.focus for a set of notes: the annotated elements are the only thing to change. */
export function notesFocus(annotations: { element?: string }[]) {
  const els = annotations.map((a) => a.element).filter(Boolean);
  return els.length ? `the annotated element${els.length > 1 ? "s" : ""} ${els.join("; ")}` : undefined;
}
