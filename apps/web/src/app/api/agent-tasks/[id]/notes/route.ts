import { z } from "zod";
import { addTaskNote } from "@/lib/server/agent-tasks";

const NoteSchema = z.object({ text: z.string().trim().min(1).max(2000) });

/** Add a note to a task's Agentation thread (role "human"): POST /api/agent-tasks/<noteId>/notes { text } */
export async function POST(request: Request, ctx: RouteContext<"/api/agent-tasks/[id]/notes">) {
  const { id } = await ctx.params;
  const parsed = NoteSchema.safeParse(await request.json().catch(() => null));
  if (!parsed.success) return Response.json({ error: "Bitte einen Text eingeben (höchstens 2000 Zeichen)." }, { status: 400 });
  const result = await addTaskNote(id, parsed.data.text);
  if (!result.ok) return Response.json({ error: result.error }, { status: result.status });
  return Response.json({ ok: true, thread: result.thread });
}
