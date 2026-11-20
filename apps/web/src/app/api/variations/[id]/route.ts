import { cancelJob, jobExists, jobStream } from "@/lib/server/jobs";

/** Replay + follow a generation job: GET /api/variations/<id>?from=<eventIndex> */
export async function GET(request: Request, ctx: RouteContext<"/api/variations/[id]">) {
  const { id } = await ctx.params;
  const from = Number(new URL(request.url).searchParams.get("from") ?? 0) || 0;
  return jobStream(id, from);
}

/** Existence probe without opening the event stream: 200 if the job is known, 404 if not. */
export async function HEAD(_request: Request, ctx: RouteContext<"/api/variations/[id]">) {
  const { id } = await ctx.params;
  return new Response(null, { status: jobExists(id) ? 200 : 404 });
}

/** Cancel a running job. */
export async function DELETE(_request: Request, ctx: RouteContext<"/api/variations/[id]">) {
  const { id } = await ctx.params;
  cancelJob(id);
  return new Response(null, { status: 204 });
}
