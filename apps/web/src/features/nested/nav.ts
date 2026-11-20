"use client";
import { create } from "zustand";
import { toast } from "sonner";
import { useApp } from "@/store";
import { findPromotedCopy } from "./slice";

/** Transient (not persisted) UI state for the nested feature. */
interface NestedUIState {
  /** Last navigation request; the overlay frames the canvas accordingly. */
  nav: { tick: number; spaceId: string; designId?: string } | null;
  /** Space whose portal is pending delete confirmation. */
  confirmDeleteId: string | null;
  askDelete: (spaceId: string | null) => void;
}

export const useNestedUI = create<NestedUIState>()((set) => ({
  nav: null,
  confirmDeleteId: null,
  askDelete: (spaceId) => set({ confirmDeleteId: spaceId }),
}));

/** Switch the canvas to `spaceId`, optionally selecting and framing one design there. */
export function enterSpace(spaceId: string, designId?: string) {
  const app = useApp.getState();
  if (!app.spaces[spaceId]) return;
  useNestedUI.setState((s) => ({ nav: { tick: (s.nav?.tick ?? 0) + 1, spaceId, designId: designId && app.designs[designId] ? designId : undefined } }));
  if (app.activeSpaceId !== spaceId) app.setActiveSpace(spaceId);
  if (designId && app.designs[designId]) app.select(designId);
}

/** Promote designs to their parent space and confirm with a toast. */
export function promote(designIds: string[]) {
  const app = useApp.getState();
  const first = designIds.map((id) => app.designs[id]).find(Boolean);
  const parentId = first ? app.spaces[first.spaceId]?.parentSpaceId : undefined;
  const created = app.promoteDesigns(designIds);
  if (!created.length || !parentId) return created;
  const parentName = useApp.getState().spaces[parentId]?.name ?? "parent";
  toast.success(
    created.length === 1
      ? `Promoted to ${parentName}`
      : `Promoted ${created.length} designs to ${parentName}`,
    { action: { label: "Show", onClick: () => enterSpace(parentId, created[0]) } },
  );
  return created;
}

/** Jump to the parent-space copy of an already promoted design. */
export function showPromotedCopy(designId: string) {
  const app = useApp.getState();
  const copy = findPromotedCopy(app, designId);
  if (copy) enterSpace(app.designs[copy].spaceId, copy);
}
