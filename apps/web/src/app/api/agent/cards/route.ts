import { CanvasError, CardsBody, createCards } from "@/lib/server/agent-canvas";

/**
 * Agent skill: add finished cards to the canvas.
 * POST /api/agent/cards { spaceId?, nearId?, cards: [{ title, html, rationale?, viewport?, viewportHeight?, kind?, parentId?, component? }] }
 * Cards go in a row right of `nearId` (else below the space's content). Returns { spaceId, ids }.
 */
export async function POST(request: Request) {
  const parsed = CardsBody.safeParse(await request.json().catch(() => null));
  if (!parsed.success) return Response.json({ error: "Invalid body", issues: parsed.error.issues }, { status: 400 });
  try {
    return Response.json(await createCards(parsed.data));
  } catch (err) {
    if (err instanceof CanvasError) return Response.json({ error: err.message }, { status: err.status });
    throw err;
  }
}
