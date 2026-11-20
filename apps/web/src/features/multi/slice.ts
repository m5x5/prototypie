import { nanoid } from "nanoid";
import type { StateCreator } from "zustand";
import type { AppState } from "@/store";

// Owned by the "multi" feature: draft cards started by dragging a card's handle onto empty canvas.

/** A card that doesn't exist yet: where it sits and which designs it's made from (1 = variation, 2+ = mix). */
export interface MixDraft {
  id: string;
  spaceId: string;
  position: { x: number; y: number };
  sourceIds: string[];
  createdAt: number;
  /** What the user typed, kept with the draft so a reload doesn't lose it. */
  text?: string;
  /** How many designs to make (1–4). */
  count?: number;
}

export interface MultiSlice {
  /** Per browser (KEY_POLICY "local"): open draft cards. */
  mixDrafts: Record<string, MixDraft>;
  startMix: (spaceId: string, position: { x: number; y: number }, sourceIds: string[]) => string;
  addMixSource: (draftId: string, sourceId: string) => void;
  removeMixSource: (draftId: string, sourceId: string) => void;
  moveMix: (draftId: string, position: { x: number; y: number }) => void;
  /** Stores the draft's text and/or count (per keystroke; persisted like the draft itself). */
  setMixInput: (draftId: string, input: { text?: string; count?: number }) => void;
  closeMix: (draftId: string) => void;
}

export const createMultiSlice: StateCreator<AppState, [], [], MultiSlice> = (set) => ({
  mixDrafts: {},
  startMix: (spaceId, position, sourceIds) => {
    const id = nanoid(8);
    set((s) => ({
      mixDrafts: { ...s.mixDrafts, [id]: { id, spaceId, position, sourceIds: [...new Set(sourceIds)], createdAt: Date.now() } },
    }));
    return id;
  },
  addMixSource: (draftId, sourceId) =>
    set((s) => {
      const d = s.mixDrafts[draftId];
      if (!d || d.sourceIds.includes(sourceId)) return {};
      return { mixDrafts: { ...s.mixDrafts, [draftId]: { ...d, sourceIds: [...d.sourceIds, sourceId] } } };
    }),
  removeMixSource: (draftId, sourceId) =>
    set((s) => {
      const d = s.mixDrafts[draftId];
      if (!d) return {};
      return { mixDrafts: { ...s.mixDrafts, [draftId]: { ...d, sourceIds: d.sourceIds.filter((x) => x !== sourceId) } } };
    }),
  moveMix: (draftId, position) =>
    set((s) => (s.mixDrafts[draftId] ? { mixDrafts: { ...s.mixDrafts, [draftId]: { ...s.mixDrafts[draftId], position } } } : {})),
  setMixInput: (draftId, input) =>
    set((s) => (s.mixDrafts[draftId] ? { mixDrafts: { ...s.mixDrafts, [draftId]: { ...s.mixDrafts[draftId], ...input } } } : {})),
  closeMix: (draftId) =>
    set((s) => {
      const next = { ...s.mixDrafts };
      delete next[draftId];
      return { mixDrafts: next };
    }),
});
