import { z } from "zod";
import { startEdit } from "@/lib/server/ai-edit";

export const maxDuration = 300;

const EditSchema = z.object({
  designId: z.string().regex(/^[\w-]{1,64}$/),
  instruction: z.string().trim().min(1).max(4000),
  focus: z.string().max(2000).optional(),
  annotations: z.array(z.object({ element: z.string().optional(), comment: z.string() })).max(50).optional(),
  agent: z.enum(["codex", "claude"]).optional(),
  podContext: z.array(z.any()).optional(),
  components: z.array(z.object({ id: z.string().max(64), title: z.string().max(400), html: z.string() })).max(20).optional(),
});

/**
 * In-place AI edit: POST /api/edit { designId, instruction, focus?, annotations?, agent?, podContext?, components? } -> { jobId }
 * The result overwrites the card's html in shared state; the previous content is kept in `versions` (undo).
 */
export async function POST(request: Request) {
  const parsed = EditSchema.safeParse(await request.json().catch(() => null));
  if (!parsed.success) return Response.json({ error: parsed.error.issues }, { status: 400 });
  const res = await startEdit(parsed.data);
  if ("error" in res) return Response.json({ error: res.error }, { status: res.status });
  return Response.json(res);
}
