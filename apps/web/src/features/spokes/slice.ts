import type { StateCreator } from "zustand";
import type { AppState } from "@/store";

/** Metadata for one exploration lane (one `generate` call launched by this feature), keyed by core job id. */
export interface LaneMeta {
  jobId: string;
  /** "clay" = block of clay page, "spoke" = component exploration, "round" = review follow-up, "merge" = spoke merged into hub. */
  type: "clay" | "spoke" | "round" | "merge";
  /** Short display name, e.g. the component name. */
  name: string;
  /** Component description the lane explores (spokes). */
  focus?: string;
  /** Hub page the lane belongs to. */
  hubId?: string;
  /** Number of designs requested. */
  count: number;
  startedAt: number;
}

// Owned by the "spokes" feature. Add state + actions here; everything is persisted automatically.
export interface SpokesSlice {
  /** Design ids the designer has reviewed (removed from the review queue). */
  reviewedIds: Record<string, true>;
  /** Lane metadata by core job id. Jobs themselves are transient; this survives reloads. */
  lanes: Record<string, LaneMeta>;
  /** Component focus of spoke designs, by design id (filled once a lane's plan is known). */
  spokeFocus: Record<string, string>;

  markReviewed: (ids: string[]) => void;
  unmarkReviewed: (id: string) => void;
  registerLane: (lane: LaneMeta) => void;
  setSpokeFocus: (ids: string[], focus: string) => void;
}

export const createSpokesSlice: StateCreator<AppState, [], [], SpokesSlice> = (set) => ({
  reviewedIds: {},
  lanes: {},
  spokeFocus: {},

  markReviewed: (ids) =>
    set((s) => {
      const reviewedIds = { ...s.reviewedIds };
      for (const id of ids) reviewedIds[id] = true;
      return { reviewedIds };
    }),
  unmarkReviewed: (id) =>
    set((s) => {
      const reviewedIds = { ...s.reviewedIds };
      delete reviewedIds[id];
      return { reviewedIds };
    }),
  registerLane: (lane) =>
    set((s) => {
      // Keep the most recent 60 lanes so persisted state stays small.
      const all = [...Object.values(s.lanes), lane].sort((a, b) => b.startedAt - a.startedAt).slice(0, 60);
      return { lanes: Object.fromEntries(all.map((l) => [l.jobId, l])) };
    }),
  setSpokeFocus: (ids, focus) =>
    set((s) => {
      const spokeFocus = { ...s.spokeFocus };
      for (const id of ids) spokeFocus[id] = focus;
      return { spokeFocus };
    }),
});
