import { describeCanvas } from "@/lib/server/agent-canvas";

/** Agent skill: list spaces and their cards (ids, titles, positions). GET /api/agent/canvas */
export async function GET() {
  return Response.json({ spaces: await describeCanvas() });
}
