import { nanoid } from "nanoid";
import type { VariationEvent } from "@/lib/types";

/**
 * Server-side generation jobs. A job keeps running when the browser disconnects
 * (reload, closed tab); clients replay its events from any offset and follow it live.
 * Kept on globalThis so dev-server hot reloads don't drop running jobs.
 */

interface Job {
  id: string;
  events: VariationEvent[];
  done: boolean;
  listeners: Set<() => void>;
  abort: AbortController;
  createdAt: number;
}

const g = globalThis as unknown as { __prototypieJobs?: Map<string, Job> };
const jobs = (g.__prototypieJobs ??= new Map());

const TTL = 60 * 60 * 1000; // keep finished jobs replayable for an hour

function sweep() {
  const now = Date.now();
  for (const [id, j] of jobs) if (j.done && now - j.createdAt > TTL) jobs.delete(id);
}

/** Start `run` as a detached job. `emit` records an event; the job ends when `run` settles. */
export function startJob(
  run: (emit: (e: VariationEvent) => void, signal: AbortSignal, jobId: string) => Promise<void>,
): string {
  sweep();
  const job: Job = { id: nanoid(10), events: [], done: false, listeners: new Set(), abort: new AbortController(), createdAt: Date.now() };
  jobs.set(job.id, job);
  const notify = () => job.listeners.forEach((l) => l());
  const emit = (e: VariationEvent) => {
    if (job.done) return;
    job.events.push(e);
    notify();
  };
  run(emit, job.abort.signal, job.id)
    .catch((err) => emit({ type: "error", index: null, message: err instanceof Error ? err.message : String(err) }))
    .finally(() => {
      emit({ type: "done" });
      job.done = true;
      notify();
    });
  return job.id;
}

/** Whether the job is still known (running, or finished and still within its replay TTL). */
export function jobExists(id: string): boolean {
  sweep();
  return jobs.has(id);
}

export function cancelJob(id: string) {
  jobs.get(id)?.abort.abort();
}

/** NDJSON stream of a job's events from `from` onwards; follows live until the job is done. */
export function jobStream(id: string, from = 0): Response {
  const job = jobs.get(id);
  if (!job) return Response.json({ error: "Unknown or expired job" }, { status: 404 });
  const encoder = new TextEncoder();
  let cursor = from;
  let listener: (() => void) | undefined;
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      const flush = () => {
        while (cursor < job.events.length) {
          controller.enqueue(encoder.encode(JSON.stringify(job.events[cursor++]) + "\n"));
        }
        if (job.done) {
          if (listener) job.listeners.delete(listener);
          controller.close();
        }
      };
      // First line tells the client which job this is, so it can resume after a reload.
      controller.enqueue(encoder.encode(JSON.stringify({ type: "job", id: job.id }) + "\n"));
      listener = () => {
        try {
          flush();
        } catch {
          // Client went away; the job itself keeps running.
          if (listener) job.listeners.delete(listener);
        }
      };
      job.listeners.add(listener);
      flush();
    },
    cancel() {
      if (listener) job.listeners.delete(listener);
    },
  });
  return new Response(stream, {
    headers: { "Content-Type": "application/x-ndjson; charset=utf-8", "Cache-Control": "no-cache", "X-Job-Id": job.id },
  });
}
