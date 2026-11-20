import { listAgentTasks } from "@/lib/server/agent-tasks";

/** Agent capacity (Rex) and the agents' tasks with steps and Agentation thread: GET /api/agent-tasks */
export async function GET() {
  return Response.json(await listAgentTasks(), { headers: { "Cache-Control": "no-store" } });
}
