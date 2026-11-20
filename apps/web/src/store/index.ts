"use client";
import { create } from "zustand";
import { persist } from "zustand/middleware";
import { createCoreSlice, type CoreSlice, type Run } from "./core";
import { createNestedSlice, type NestedSlice } from "@/features/nested/slice";
import { createPollSlice, type PollSlice } from "@/features/poll/slice";
import { createSpokesSlice, type SpokesSlice } from "@/features/spokes/slice";
import { createAgentationSlice, type AgentationSlice } from "@/features/agentation/slice";
import { createMultiSlice, type MultiSlice } from "@/features/multi/slice";
import { isOwnRun, persistable, startSync, syncedStorage } from "./sync";

export type AppState = CoreSlice & NestedSlice & PollSlice & SpokesSlice & AgentationSlice & MultiSlice;

export const useApp = create<AppState>()(
  persist(
    (...a) => ({
      ...createCoreSlice(...a),
      ...createNestedSlice(...a),
      ...createPollSlice(...a),
      ...createSpokesSlice(...a),
      ...createAgentationSlice(...a),
      ...createMultiSlice(...a),
    }),
    {
      name: "prototypie",
      // Server-backed (data/state.json, shared by every browser) with IndexedDB as the offline cache.
      // Which keys are shared, per-browser or transient is decided in KEY_POLICY (./key-policy.ts).
      // Hydration already includes the server's state, so the checks below see the shared canvas.
      storage: syncedStorage,
      // Persist data only (functions are recreated, transient keys dropped).
      partialize: persistable,
      onRehydrateStorage: () => (state) => {
        if (typeof window !== "undefined") startSync(useApp);
        if (!state) return;
        // Generations keep running on the server across reloads: reattach and let them finish. Runs are
        // shared, but only the browser that started one drives it (two followers would duplicate designs).
        // Exception: once a run's plan has landed (all placeholders exist), replaying it only fills those
        // same cards, so any browser can adopt it. Otherwise a run whose starter is gone (closed tab, an
        // agent's headless page) would show "Designing…" forever.
        const adoptable = (run: Run) =>
          isOwnRun(run.id) ||
          (!!run.serverJobId && run.designIds.length > 0 && run.designIds.every((id) => !!id && !!state.designs[id]));
        state.resumeRuns(adoptable);
        // A foreign run whose plan never landed can't be adopted, but if the server has lost its job
        // (dev server restart, TTL) nobody will ever finish it: fail its cards instead of "Designing…".
        void state.reapLostRuns((run) => !adoptable(run));
        // Placeholders that belong to no run can't be recovered.
        const live = new Set(Object.values(state.runs).flatMap((r) => r.designIds));
        for (const d of Object.values(state.designs)) {
          if (d.status === "generating" && !live.has(d.id)) state.updateDesign(d.id, { status: "error", error: "Interrupted" });
        }
      },
    },
  ),
);

// Lets non-React helpers (e.g. cancelJob) read the store without an import cycle.
(globalThis as { __prototypieStore?: () => AppState }).__prototypieStore = () => useApp.getState();
