import { cancelEdit } from "@/lib/server/ai-edit";

/** Cancel a running in-place edit of this design (also clears a stale busy flag). */
export async function DELETE(_request: Request, ctx: RouteContext<"/api/edit/[designId]">) {
  const { designId } = await ctx.params;
  const ok = await cancelEdit(designId);
  return new Response(null, { status: ok ? 204 : 404 });
}
