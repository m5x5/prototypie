import { cancelForDeletedDesigns } from "@/lib/server/agent-queue";
import { applyChanges, isValidPath, readState, type Change } from "@/lib/server/state-store";

/**
 * Shared canvas state, record by record.
 * GET /api/state?since=N&epoch=E -> { epoch, version, records: { "<key>/<id>": { rev, value? , deleted? } } }
 *   Only records changed after version N (all records when N=0 or E is not the server's epoch).
 */
export async function GET(request: Request) {
  const url = new URL(request.url);
  const since = Number(url.searchParams.get("since") ?? 0) || 0;
  const epoch = url.searchParams.get("epoch") ?? undefined;
  return Response.json(await readState(since, epoch), { headers: { "Cache-Control": "no-store" } });
}

/**
 * PUT /api/state { clientId?, changes: [{ path, value } | { path, deleted: true }] } -> { epoch, version }
 * All changes in one request share one version; last write per record wins. Every accepted change is
 * appended to data/state-log.ndjson (ts, rev, clientId, path, op, before, title, reason, user agent).
 */
export async function PUT(request: Request) {
  let body: { clientId?: unknown; reason?: unknown; changes?: unknown };
  try {
    body = await request.json();
  } catch {
    return Response.json({ error: "Body must be JSON." }, { status: 400 });
  }
  if (!Array.isArray(body.changes)) return Response.json({ error: "`changes` must be an array." }, { status: 400 });
  const changes: Change[] = [];
  for (const c of body.changes as Change[]) {
    if (!c || !isValidPath(c.path)) return Response.json({ error: `Invalid record path (not a shared store key): ${c?.path}` }, { status: 400 });
    if (!c.deleted && c.value === undefined) return Response.json({ error: `Missing value for ${c.path}` }, { status: 400 });
    changes.push({ path: c.path, value: c.deleted ? undefined : c.value, deleted: !!c.deleted });
  }
  const str = (v: unknown) => (typeof v === "string" ? v.slice(0, 200) : undefined);
  const result = await applyChanges(changes, {
    clientId: str(body.clientId),
    reason: str(body.reason),
    userAgent: request.headers.get("user-agent") ?? undefined,
  });
  // A deleted placeholder card cancels the queue item that would have filled it, so no agent works for nothing.
  const deletedDesigns = changes.filter((c) => c.deleted && c.path.startsWith("designs/")).map((c) => c.path.slice(8));
  if (deletedDesigns.length) await cancelForDeletedDesigns(deletedDesigns).catch(() => {});
  return Response.json(result);
}
