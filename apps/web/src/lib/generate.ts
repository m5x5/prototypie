import type { VariationEvent, VariationRequest } from "@/lib/types";

/** Calls /api/variations and invokes onEvent for every NDJSON event as it streams in. */
export async function streamVariations(
  req: VariationRequest,
  onEvent: (e: VariationEvent) => void,
  signal?: AbortSignal,
): Promise<void> {
  const res = await fetch("/api/variations", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
    signal,
  });
  return readEvents(res, onEvent);
}

/** Reattach to a server job (e.g. after a reload) and replay its events from the start. */
export async function resumeVariations(jobId: string, onEvent: (e: VariationEvent) => void, signal?: AbortSignal) {
  const res = await fetch(`/api/variations/${encodeURIComponent(jobId)}`, { signal });
  if (res.status === 404) {
    onEvent({ type: "error", index: null, message: "The server no longer has this job (dev server restarted?)" });
    onEvent({ type: "done" });
    return;
  }
  return readEvents(res, onEvent);
}

/** Does the server still have this job? `undefined` when it couldn't be asked (offline, server down). */
export async function serverJobExists(jobId: string): Promise<boolean | undefined> {
  try {
    const res = await fetch(`/api/variations/${encodeURIComponent(jobId)}`, { method: "HEAD" });
    if (res.status === 404) return false;
    return res.ok ? true : undefined;
  } catch {
    return undefined;
  }
}

/** Ask the server to stop a job. Stopping the fetch alone no longer stops generation. */
export function cancelServerJob(jobId: string) {
  void fetch(`/api/variations/${encodeURIComponent(jobId)}`, { method: "DELETE" }).catch(() => {});
}

async function readEvents(res: Response, onEvent: (e: VariationEvent) => void) {
  if (!res.ok || !res.body) {
    onEvent({ type: "error", index: null, message: `Request failed (${res.status})` });
    onEvent({ type: "done" });
    return;
  }
  const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = "";
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += value;
    let nl: number;
    while ((nl = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, nl).trim();
      buffer = buffer.slice(nl + 1);
      if (line) onEvent(JSON.parse(line) as VariationEvent);
    }
  }
}
