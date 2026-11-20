import { nanoid } from "nanoid";
import type { StateCreator } from "zustand";
import type { AppState } from "@/store";
import type { SuggestedDecision } from "./schema";

// Owned by the "poll" feature. Everything here is persisted automatically.

export interface DraftOption {
  designId: string;
  label: string;
}
export interface DraftDecision {
  id: string;
  question: string;
  why: string;
  options: DraftOption[];
}

/** What the designer keeps locally about a poll they published (no HTML - that lives on the server). */
export interface PublishedPoll {
  id: string;
  title: string;
  createdAt: number;
  shareUrl: string;
  decisions: { id: string; question: string; options: { id: string; designId: string; label: string; title: string }[] }[];
}

export interface PollSlice {
  /** Design ids collected into the poll tray, in order added. */
  pollCandidates: string[];
  pollTitle: string;
  pollDecisions: DraftDecision[];
  publishedPolls: PublishedPoll[];
  /** `${pollId}/${decisionId}` -> winning option id. */
  pollWinners: Record<string, string>;

  togglePollCandidate: (designId: string) => void;
  setPollTitle: (title: string) => void;
  setPollDecisions: (decisions: DraftDecision[]) => void;
  applySuggestedDecisions: (suggested: SuggestedDecision[]) => void;
  addPollDecision: (decision?: Partial<DraftDecision>) => void;
  updatePollDecision: (id: string, patch: Partial<DraftDecision>) => void;
  removePollDecision: (id: string) => void;
  movePollDecision: (id: string, delta: number) => void;
  addPublishedPoll: (poll: PublishedPoll) => void;
  removePublishedPoll: (id: string) => void;
  setPollWinner: (pollId: string, decisionId: string, optionId: string | null) => void;
}

function move<T>(arr: T[], from: number, to: number): T[] {
  if (to < 0 || to >= arr.length || from === to) return arr;
  const out = [...arr];
  const [item] = out.splice(from, 1);
  out.splice(to, 0, item);
  return out;
}
export { move as moveItem };

export const createPollSlice: StateCreator<AppState, [], [], PollSlice> = (set) => ({
  pollCandidates: [],
  pollTitle: "",
  pollDecisions: [],
  publishedPolls: [],
  pollWinners: {},

  togglePollCandidate: (designId) =>
    set((s) => {
      if (!s.pollCandidates.includes(designId)) return { pollCandidates: [...s.pollCandidates, designId] };
      // Removing a candidate also takes it out of any draft decision.
      return {
        pollCandidates: s.pollCandidates.filter((id) => id !== designId),
        pollDecisions: s.pollDecisions.map((d) => ({ ...d, options: d.options.filter((o) => o.designId !== designId) })),
      };
    }),
  setPollTitle: (pollTitle) => set({ pollTitle }),
  setPollDecisions: (pollDecisions) => set({ pollDecisions }),
  applySuggestedDecisions: (suggested) =>
    set({ pollDecisions: suggested.map((d) => ({ id: nanoid(8), question: d.question, why: d.why, options: d.options })) }),
  addPollDecision: (decision) =>
    set((s) => ({
      pollDecisions: [...s.pollDecisions, { id: nanoid(8), question: "", why: "", options: [], ...decision }],
    })),
  updatePollDecision: (id, patch) =>
    set((s) => ({ pollDecisions: s.pollDecisions.map((d) => (d.id === id ? { ...d, ...patch } : d)) })),
  removePollDecision: (id) => set((s) => ({ pollDecisions: s.pollDecisions.filter((d) => d.id !== id) })),
  movePollDecision: (id, delta) =>
    set((s) => {
      const i = s.pollDecisions.findIndex((d) => d.id === id);
      return { pollDecisions: move(s.pollDecisions, i, i + delta) };
    }),
  addPublishedPoll: (poll) => set((s) => ({ publishedPolls: [poll, ...s.publishedPolls.filter((p) => p.id !== poll.id)] })),
  removePublishedPoll: (id) => set((s) => ({ publishedPolls: s.publishedPolls.filter((p) => p.id !== id) })),
  setPollWinner: (pollId, decisionId, optionId) =>
    set((s) => {
      const pollWinners = { ...s.pollWinners };
      const key = `${pollId}/${decisionId}`;
      if (optionId) pollWinners[key] = optionId;
      else delete pollWinners[key];
      return { pollWinners };
    }),
});
