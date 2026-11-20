import { CanvasError, createExploration, ExplorationBody } from "@/lib/server/agent-canvas";

/**
 * Agent skill: start an exploration (nested space) with its portal card, in one atomic write.
 * POST /api/agent/explorations { name, parentSpaceId?, seedId?, cards?: [...] }
 * `seedId` copies that design in as the starting card; `cards` are created inside. Returns { spaceId, portal, seedId, cardIds }.
 */
export async function POST(request: Request) {
  const parsed = ExplorationBody.safeParse(await request.json().catch(() => null));
  if (!parsed.success) return Response.json({ error: "Invalid body", issues: parsed.error.issues }, { status: 400 });
  try {
    return Response.json(await createExploration(parsed.data));
  } catch (err) {
    if (err instanceof CanvasError) return Response.json({ error: err.message }, { status: err.status });
    throw err;
  }
}
