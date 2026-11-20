// Which store keys sync through the server. Plain data, no client deps: imported by the client sync
// engine (./sync.ts) and by the server state store, which rejects keys that aren't shared.
import type { AppState } from "./index";

export type DataKey = {
  [K in keyof AppState]-?: AppState[K] extends (...args: never[]) => unknown ? never : K;
}[keyof AppState];

/**
 * - shared:     Record<id, T>, one server record per entry.
 * - sharedList: T[] of objects with an `id`, one server record per item (order rebuilt by LIST_ORDER).
 * - local:      this browser only (IndexedDB), never sent to the server.
 * - transient:  not persisted at all.
 * Typed as Record<DataKey, ...> so a slice that adds state must decide here (tsc fails otherwise).
 */
export const KEY_POLICY: Record<DataKey, "shared" | "sharedList" | "local" | "transient"> = {
  // core
  spaces: "shared",
  designs: "shared",
  // In-flight generations. Shared so another browser doesn't flag the placeholders as "Interrupted"
  // on load, and so a reload anywhere can reattach to the server job.
  runs: "shared",
  jobs: "transient", // per-tab progress of fetches this tab drives
  activeSpaceId: "local", // where this browser is looking
  selectedId: "local",
  drafts: "local", // half-typed prompts, per browser
  promptHistory: "local", // personal recall list
  agentChoice: "local", // per-browser preference
  // nested
  portals: "shared",
  promotedFrom: "shared",
  seededFrom: "shared",
  // agentation (designId -> notes on that design)
  notes: "shared",
  // spokes
  reviewedIds: "shared",
  lanes: "shared",
  spokeFocus: "shared",
  // poll: the tray and draft decisions are this designer's scratchpad; published polls are shared
  pollCandidates: "local",
  pollTitle: "local",
  pollDecisions: "local",
  publishedPolls: "sharedList",
  pollWinners: "shared",
  // multi: draft cards from dragging a handle onto empty canvas (this designer's scratchpad)
  mixDrafts: "local",
};

export const LIST_ORDER: Partial<Record<DataKey, (a: never, b: never) => number>> = {
  publishedPolls: (a: { createdAt: number }, b: { createdAt: number }) => b.createdAt - a.createdAt,
};

/** Store keys synced through /api/state. The server rejects records for any other key. */
export const SHARED_KEYS = (Object.keys(KEY_POLICY) as DataKey[]).filter((k) => KEY_POLICY[k].startsWith("shared"));
