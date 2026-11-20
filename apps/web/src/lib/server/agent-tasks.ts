import { execFile } from "node:child_process";
import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";
import { withContext, type QueueItem } from "./agent-queue";

/**
 * What the header's agent capacity and task panel show (GET /api/agent-tasks), modelled on an agent pill:
 * - capacity: the proto-* agent blocks in the Rex multiplexer session (`rex block ls`), like a game's
 *   /__agents. "working" counts blocks whose screen shows Claude Code busy (`rex capture`).
 * - tasks: the agent-queue items (data/agent-queue) with their Agentation note (status + thread), plus open
 *   Agentation notes that are not queue items (Implement requests, notes on the app itself).
 * Notes from the panel are added to the Agentation thread as `role: "human"`, where workers and the coordinator
 * read them.
 */

const AGENTATION = "http://localhost:4747";
const QUEUE_DIR = path.join(process.cwd(), "data", "agent-queue");
const REX = process.env.REX_BIN_PATH ?? "rex";
const REX_SESSION = process.env.REX_SESSION ?? "prototypie";
const MAX_QUEUE_TASKS = 80;

export interface AgentCapacity {
  /** null when Rex is not reachable: the header then shows no count instead of guessing. */
  working: number | null;
  total: number | null;
  agents: { name: string; status: string }[];
}

export type TaskStatus = "waiting" | "working" | "delivered" | "resolved" | "cancelled";

export interface TaskStep {
  key: "received" | "announced" | "working" | "delivered" | "resolved" | "cancelled";
  label: string;
  /** ISO time when this step happened; null when it has not (yet), or its time is not recorded. */
  at: string | null;
  done: boolean;
}

export interface ThreadMessage {
  id: string;
  role: string;
  content: string;
  at: string;
}

export interface AgentTask {
  /** Stable id: the Agentation note id when there is one, else the queue key. */
  id: string;
  kind: "queue" | "note" | "implement";
  /** The canvas card this task is about: an Implement request's design, or the placeholder a queue item fills. */
  designId?: string;
  /** The agent's latest answer in the thread (often the resolve summary). */
  summary?: string;
  title: string;
  /** What was asked for. */
  request: string;
  jobId?: string;
  index?: number;
  baseTitle?: string;
  status: TaskStatus;
  createdAt: string;
  updatedAt: string;
  steps: TaskStep[];
  /** Where the result lands on the canvas (queue tasks). */
  target?: { spaceId: string; spaceName: string; spacePath: string[]; designId?: string };
  /** The Agentation note mirroring the task: notes can only be added when there is one. */
  noteId?: string;
  noteStatus?: string;
  element?: string;
  thread: ThreadMessage[];
}

interface Annotation {
  id: string;
  comment?: string;
  element?: string;
  status?: string;
  createdAt?: string;
  updatedAt?: string;
  resolvedBy?: string;
  url?: string;
  thread?: { id: string; role: string; content: string; timestamp: number }[];
}

const g = globalThis as unknown as { __agentCapacity?: { at: number; value: AgentCapacity } };

/** Run the Rex CLI; resolves stdout, or undefined when Rex is not reachable. */
function rex(args: string[]): Promise<string | undefined> {
  return new Promise((resolve) =>
    execFile(REX, args, { timeout: 4000, maxBuffer: 8 * 1024 * 1024 }, (error, stdout) => resolve(error ? undefined : stdout)),
  );
}

/**
 * Rex's view of the agent sessions, cached for 3 s (the header polls every few seconds): the blocks labelled
 * proto-* in the Rex session REX_SESSION. Rex has no agent status, so a block counts as working while its
 * screen shows Claude Code's "esc to interrupt" hint.
 */
export async function agentCapacity(): Promise<AgentCapacity> {
  const cached = g.__agentCapacity;
  if (cached && Date.now() - cached.at < 3000) return cached.value;
  let value: AgentCapacity = { working: null, total: null, agents: [] };
  try {
    const out = await rex(["block", "ls", "-s", REX_SESSION, "--json"]);
    if (out) {
      const blocks = ((JSON.parse(out).blocks ?? []) as { block_id: string; label?: string }[]).filter((b) =>
        b.label?.startsWith("proto-"),
      );
      const agents = await Promise.all(
        blocks.map(async (b) => {
          const screen = await rex(["capture", "-b", b.block_id, "--trim"]);
          const tail = screen?.split("\n").slice(-12).join("\n") ?? "";
          const status = screen === undefined ? "unknown" : /esc to interrupt/.test(tail) ? "working" : "idle";
          return { name: b.label as string, status };
        }),
      );
      value = { working: agents.filter((a) => a.status === "working").length, total: agents.length, agents };
    }
  } catch {
    value = { working: null, total: null, agents: [] };
  }
  g.__agentCapacity = { at: Date.now(), value };
  return value;
}

async function getJson<T>(url: string): Promise<T | undefined> {
  try {
    const res = await fetch(url, { signal: AbortSignal.timeout(2500), cache: "no-store" });
    return res.ok ? ((await res.json()) as T) : undefined;
  } catch {
    return undefined;
  }
}

type StoredItem = Omit<QueueItem, "baseHtml" | "bases"> & { bases?: { id: string; title: string }[]; doneAt?: string };

/** The newest queue items, without the (large) base HTML. A done item's time is its file's last write. */
async function readQueue(): Promise<StoredItem[]> {
  let names: string[] = [];
  try {
    names = (await readdir(QUEUE_DIR)).filter((n) => n.endsWith(".json"));
  } catch {
    return [];
  }
  const items = await Promise.all(
    names.map(async (n): Promise<StoredItem | undefined> => {
      try {
        const file = path.join(QUEUE_DIR, n);
        const raw = JSON.parse(await readFile(file, "utf8")) as QueueItem;
        const { baseHtml: _html, bases, ...rest } = raw;
        void _html;
        const doneAt = raw.status !== "pending" ? (await stat(file)).mtime.toISOString() : undefined;
        return { ...rest, bases: bases?.map((b) => ({ id: b.id, title: b.title })), doneAt };
      } catch {
        return undefined;
      }
    }),
  );
  return items
    .filter((i): i is StoredItem => !!i)
    .sort((a, b) => b.createdAt.localeCompare(a.createdAt))
    .slice(0, MAX_QUEUE_TASKS);
}

function threadOf(a?: Annotation): ThreadMessage[] {
  return (a?.thread ?? []).map((m) => ({ id: m.id, role: m.role, content: m.content, at: new Date(m.timestamp).toISOString() }));
}

/** First line of an Agentation comment, without the "[prototypie …]" tag. */
function noteTitle(comment = "") {
  const line = comment.split("\n")[0].replace(/^\[prototypie [^\]]*\]\s*/, "").trim();
  return line.length > 90 ? `${line.slice(0, 88)}…` : line || "Notiz";
}

function queueTask(item: StoredItem, note?: Annotation): AgentTask {
  const thread = threadOf(note);
  const firstAgent = thread.find((m) => m.role === "agent");
  const cancelled = item.status === "cancelled" || note?.status === "dismissed";
  const resolved = note?.status === "resolved";
  const delivered = item.status === "done";
  const acknowledged = note?.status === "acknowledged";
  const status: TaskStatus = cancelled ? "cancelled" : resolved ? "resolved" : delivered ? "delivered" : acknowledged || firstAgent ? "working" : "waiting";
  const lastAgent = [...thread].reverse().find((m) => m.role === "agent");
  const steps: TaskStep[] = [
    { key: "received", label: "Eingang", at: item.createdAt, done: true },
    { key: "announced", label: "Agentation-Notiz", at: note?.createdAt ?? null, done: !!note },
    {
      key: "working",
      label: "In Arbeit",
      // Agentation records no time for "acknowledged"; an agent's first thread message is the earliest proof.
      at: firstAgent?.at ?? null,
      done: status !== "waiting" && status !== "cancelled",
    },
    { key: "delivered", label: "Geliefert", at: delivered ? (item.doneAt ?? null) : null, done: delivered || resolved },
    { key: "resolved", label: "Abgeschlossen", at: resolved ? (lastAgent?.at ?? note?.updatedAt ?? null) : null, done: resolved },
  ];
  if (cancelled) steps.push({ key: "cancelled", label: "Abgebrochen", at: item.doneAt ?? note?.updatedAt ?? null, done: true });
  const dir = item.direction?.title && !/^Direction \d+$/.test(item.direction.title) ? item.direction.title : undefined;
  const subject = item.baseTitle ?? (item.bases?.length ? item.bases.map((b) => b.title).join(" + ") : undefined);
  const ctx = item.context;
  return {
    id: note?.id ?? `${item.jobId}-${item.index}`,
    kind: "queue",
    title: dir ?? subject ?? noteTitle(item.instruction),
    request: item.instruction || item.direction?.brief || "",
    jobId: item.jobId,
    index: item.index,
    baseTitle: subject,
    status,
    createdAt: item.createdAt,
    updatedAt: note?.updatedAt ?? item.doneAt ?? item.createdAt,
    steps,
    target: ctx ? { spaceId: ctx.spaceId, spaceName: ctx.spaceName, spacePath: ctx.spacePath, designId: ctx.placeholderId } : undefined,
    noteId: note?.id,
    noteStatus: note?.status,
    designId: ctx?.placeholderId,
    summary: lastAgent?.content,
    thread,
  };
}

const IMPLEMENT = /^\[prototypie implement ([\w-]+)\]/;

function noteTask(a: Annotation): AgentTask {
  const thread = threadOf(a);
  const firstAgent = thread.find((m) => m.role === "agent");
  const lastAgent = [...thread].reverse().find((m) => m.role === "agent");
  const designId = (a.comment ?? "").match(IMPLEMENT)?.[1];
  const status: TaskStatus = a.status === "resolved" ? "resolved" : a.status === "dismissed" ? "cancelled" : a.status === "acknowledged" || firstAgent ? "working" : "waiting";
  const created = a.createdAt ?? new Date().toISOString();
  return {
    id: a.id,
    kind: designId ? "implement" : "note",
    designId,
    summary: lastAgent?.content,
    title: noteTitle(a.comment),
    request: (a.comment ?? "").replace(/^\[prototypie [^\]]*\]\s*/, ""),
    status,
    createdAt: created,
    updatedAt: a.updatedAt ?? created,
    steps: [
      { key: "received", label: designId ? "Gesendet" : "Notiz", at: created, done: true },
      { key: "working", label: "In Arbeit", at: firstAgent?.at ?? null, done: status === "working" || status === "resolved" },
      { key: "resolved", label: "Abgeschlossen", at: status === "resolved" ? (lastAgent?.at ?? a.updatedAt ?? null) : null, done: status === "resolved" },
      ...(status === "cancelled" ? [{ key: "cancelled" as const, label: "Verworfen", at: a.updatedAt ?? null, done: true }] : []),
    ],
    noteId: a.id,
    noteStatus: a.status,
    element: a.element,
    thread,
  };
}

const APP_URL = /^https?:\/\/(localhost|127\.0\.0\.1)(:\d+)?\//;
const gi = globalThis as unknown as { __implementNotes?: { at: number; value: Annotation[] } };

/**
 * Implement requests ("[prototypie implement <designId>]") in any state, also resolved ones (GET /pending only
 * has open notes). Agentation has no "all annotations" endpoint, so this reads the app's sessions, at most once a
 * minute; the open ones still come fresh from /pending on every poll.
 */
async function implementNotes(fresh = false): Promise<Annotation[]> {
  const cached = gi.__implementNotes;
  if (!fresh && cached && Date.now() - cached.at < 60_000) return cached.value;
  const sessions = await getJson<{ id: string; url?: string }[]>(`${AGENTATION}/sessions`);
  // Unreachable: keep what we had instead of caching "nothing" for a minute.
  if (!sessions) return cached?.value ?? [];
  const ours = sessions.filter((s) => s.url && APP_URL.test(s.url));
  const found: Annotation[] = [];
  for (let i = 0; i < ours.length; i += 12) {
    const batch = await Promise.all(ours.slice(i, i + 12).map((s) => getJson<{ annotations?: Annotation[] }>(`${AGENTATION}/sessions/${s.id}`)));
    for (const sess of batch) for (const a of sess?.annotations ?? []) if (IMPLEMENT.test(a.comment ?? "")) found.push(a);
  }
  gi.__implementNotes = { at: Date.now(), value: found };
  return found;
}

/**
 * Implement notes read now (not the minute-old cache), for /api/implement's duplicate check; undefined when
 * Agentation can't be read. Also refreshes the cache the panel uses.
 */
export async function currentImplementNotes(): Promise<Annotation[] | undefined> {
  if (!(await getJson<unknown[]>(`${AGENTATION}/sessions`))) return undefined;
  return implementNotes(true);
}

/** Drop the cached scan, so the panel lists a note that was just posted on its next poll. */
export function forgetImplementNotes() {
  gi.__implementNotes = undefined;
}

/** All tasks for the panel, newest activity first, plus the capacity. */
export async function listAgentTasks(): Promise<{ capacity: AgentCapacity; tasks: AgentTask[]; agentation: boolean }> {
  const [capacity, queue, pending, implemented] = await Promise.all([
    agentCapacity(),
    readQueue().then((items) => withContext(items as QueueItem[]) as Promise<StoredItem[]>),
    getJson<{ annotations: Annotation[] }>(`${AGENTATION}/pending`),
    implementNotes(),
  ]);
  const notes = await Promise.all(queue.map((item) => (item.annotationId ? getJson<Annotation>(`${AGENTATION}/annotations/${item.annotationId}`) : undefined)));
  const tasks = queue.map((item, i) => queueTask(item, notes[i]));
  const known = new Set(queue.map((i) => i.annotationId).filter(Boolean));
  // Open notes that aren't queue items: Implement requests and notes on the app (not other apps' sessions).
  // Fresh open notes first, so they win over the (up to a minute old) implement scan.
  for (const a of [...(pending?.annotations ?? []), ...implemented]) {
    if (known.has(a.id)) continue;
    if (a.url && !APP_URL.test(a.url)) continue;
    if ((a.comment ?? "").startsWith("[prototypie job ")) continue;
    known.add(a.id);
    tasks.push(noteTask(a));
  }
  tasks.sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
  return { capacity, tasks, agentation: pending !== undefined };
}

/** Add a note from the panel to the task's Agentation thread. Only notes the panel lists can be written to. */
export async function addTaskNote(noteId: string, text: string): Promise<{ ok: true; thread: ThreadMessage[] } | { ok: false; status: number; error: string }> {
  const { tasks } = await listAgentTasks();
  if (!tasks.some((t) => t.noteId === noteId)) return { ok: false, status: 404, error: "Unbekannte Aufgabe" };
  try {
    const res = await fetch(`${AGENTATION}/annotations/${encodeURIComponent(noteId)}/thread`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ role: "human", content: text }),
      signal: AbortSignal.timeout(4000),
    });
    if (!res.ok) return { ok: false, status: 502, error: `Agentation antwortet ${res.status}` };
    return { ok: true, thread: threadOf((await res.json()) as Annotation) };
  } catch {
    return { ok: false, status: 502, error: "Agentation nicht erreichbar" };
  }
}
