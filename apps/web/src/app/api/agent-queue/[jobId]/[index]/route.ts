import { z } from "zod";
import { inferViewport, inferViewportHeight, submitResult } from "@/lib/server/agent-queue";

const ResultSchema = z.object({
  title: z.string().min(1),
  rationale: z.string(),
  html: z.string().min(1),
  /** Width the design is laid out at (e.g. 1240 for a tablet). Default: <meta name="viewport" content="width=N"> in html, else the card's current frame. */
  viewport: z.number().int().min(200).max(4000).optional(),
  /** Height of a fixed-size screen (e.g. 960 for a 640×960 phone). Default: height=M in the viewport meta, else free height. */
  viewportHeight: z.number().int().min(200).max(4000).optional(),
});

/**
 * Deliver a finished design for one queued direction: POST /api/agent-queue/<jobId>/<index> { title, rationale, html, viewport?, viewportHeight? }
 * 200 placed · 404 nobody waits (done, cancelled, expired) · 409 its placeholder card was deleted (item cancelled).
 */
export async function POST(request: Request, ctx: RouteContext<"/api/agent-queue/[jobId]/[index]">) {
  const { jobId, index } = await ctx.params;
  const parsed = ResultSchema.safeParse(await request.json().catch(() => null));
  if (!parsed.success) return Response.json({ error: parsed.error.issues }, { status: 400 });
  const viewport = parsed.data.viewport ?? inferViewport(parsed.data.html);
  const viewportHeight = parsed.data.viewportHeight ?? inferViewportHeight(parsed.data.html);
  const ok = await submitResult(jobId, Number(index), {
    ...parsed.data,
    ...(viewport ? { viewport } : {}),
    ...(viewportHeight ? { viewportHeight } : {}),
  });
  if (ok === "gone")
    return Response.json(
      {
        error:
          "The placeholder card for this item was deleted in the app, so the item is cancelled and nothing was placed. If the design is still wanted, create a card with POST /api/agent/cards.",
      },
      { status: 409 },
    );
  if (!ok) return Response.json({ error: "No job is waiting for this item (done, cancelled or expired)" }, { status: 404 });
  return Response.json({ ok: true });
}
