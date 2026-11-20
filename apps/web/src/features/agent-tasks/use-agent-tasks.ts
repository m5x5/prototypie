"use client";
import { useEffect, useMemo, useSyncExternalStore } from "react";
import { linkedDesignIds } from "@/lib/implement-links";
import type { AgentCapacity, AgentTask } from "@/lib/server/agent-tasks";
import { useApp } from "@/store";

/**
 * One shared poll of GET /api/agent-tasks for every consumer (the header pill and its panel, the Inspector's
 * progress), so a second view never fetches twice. Faster while the panel is open, slow in a background tab.
 * Also carries the panel's open state, so other views can open it on a task ("Im Aufgabenpanel öffnen").
 */

export type AgentTasksData = { capacity: AgentCapacity; tasks: AgentTask[]; agentation: boolean };

type State = {
  data: AgentTasksData | null;
  failed: boolean;
  /** The header panel: open, and the task to expand and scroll to. `tick` re-triggers the same task. */
  panel: { open: boolean; taskId: string | null; tick: number };
};

const POLL_OPEN = 4000;
const POLL_CLOSED = 8000;
const POLL_HIDDEN = 30000;

let state: State = { data: null, failed: false, panel: { open: false, taskId: null, tick: 0 } };
const listeners = new Set<() => void>();
let users = 0;
let timer = 0;
let inflight: Promise<void> | null = null;

function set(next: Partial<State>) {
  state = { ...state, ...next };
  listeners.forEach((l) => l());
}

export function reloadAgentTasks(): Promise<void> {
  if (inflight) return inflight;
  inflight = (async () => {
    try {
      const res = await fetch("/api/agent-tasks", { cache: "no-store" });
      if (!res.ok) throw new Error(String(res.status));
      set({ data: (await res.json()) as AgentTasksData, failed: false });
    } catch {
      set({ failed: true });
    } finally {
      inflight = null;
    }
  })();
  return inflight;
}

function schedule() {
  window.clearTimeout(timer);
  if (!users) return;
  const wait = document.hidden ? POLL_HIDDEN : state.panel.open ? POLL_OPEN : POLL_CLOSED;
  timer = window.setTimeout(async () => {
    await reloadAgentTasks();
    schedule();
  }, wait);
}

function onVisible() {
  if (document.hidden || !users) return;
  void reloadAgentTasks().then(schedule);
}

/** Keeps the shared poll running while at least one component uses the data. */
function usePolling() {
  useEffect(() => {
    users += 1;
    if (users === 1) {
      document.addEventListener("visibilitychange", onVisible);
      void reloadAgentTasks().then(schedule);
    }
    return () => {
      users -= 1;
      if (!users) {
        window.clearTimeout(timer);
        document.removeEventListener("visibilitychange", onVisible);
      }
    };
  }, []);
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}
const getState = () => state;

export function useAgentTasks() {
  usePolling();
  return useSyncExternalStore(subscribe, getState, getState);
}

/** Open (or close) the header's task panel, optionally on one task. */
export function setTaskPanel(open: boolean, taskId: string | null = null) {
  set({ panel: { open, taskId: open ? taskId : null, tick: state.panel.tick + 1 } });
  schedule();
  if (open) void reloadAgentTasks();
}

/** Open the header panel on `taskId` ("Im Aufgabenpanel öffnen"). */
export function openTaskPanel(taskId: string) {
  setTaskPanel(true, taskId);
}

/**
 * Tasks linked to a canvas card (Implement request for it, or the queue item it is the placeholder of). Pass the
 * card's linked ids (useLinkedDesignIds) so a promoted copy shows its source's tasks and vice versa.
 */
export function tasksForDesign(data: AgentTasksData | null, designIds: string | readonly string[] | undefined): AgentTask[] {
  if (!data || !designIds?.length) return [];
  const ids = new Set(typeof designIds === "string" ? [designIds] : designIds);
  return data.tasks.filter((t) => t.designId !== undefined && ids.has(t.designId));
}

/** The card plus its promoted source and copies (same decision, same agent tasks). Stable while the links don't change. */
export function useLinkedDesignIds(designId: string | undefined): string[] {
  const promotedFrom = useApp((s) => s.promotedFrom);
  return useMemo(() => (designId ? linkedDesignIds(promotedFrom, designId) : []), [promotedFrom, designId]);
}
