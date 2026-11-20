import { listPending } from "@/lib/server/agent-queue";

/** Pending work items for the Claude Code watcher: GET /api/agent-queue */
export async function GET() {
  return Response.json({ items: await listPending() });
}
