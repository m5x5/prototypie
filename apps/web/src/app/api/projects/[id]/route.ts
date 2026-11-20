import { notifyAgent } from "@/lib/server/agent-queue";
import { getProject } from "@/lib/server/projects";

/** One project with the first paragraph of its local README: GET /api/projects/<id>. */
export async function GET(_request: Request, ctx: RouteContext<"/api/projects/[id]">) {
  const project = await getProject((await ctx.params).id);
  if (!project) return Response.json({ error: "Unknown project" }, { status: 404 });
  return Response.json(project, { headers: { "Cache-Control": "no-store" } });
}

/**
 * Ask an agent to enrich a new project space: POST /api/projects/<id> { spaceId } posts the Agentation note
 * "[prototypie project-info <spaceId>] …". The note text is built here from the TTL, not from the request.
 */
export async function POST(request: Request, ctx: RouteContext<"/api/projects/[id]">) {
  const { spaceId } = (await request.json().catch(() => ({}))) as { spaceId?: unknown };
  if (typeof spaceId !== "string" || !/^[\w-]+$/.test(spaceId)) return Response.json({ error: "spaceId required" }, { status: 400 });
  const project = await getProject((await ctx.params).id);
  if (!project) return Response.json({ error: "Unknown project" }, { status: 404 });
  const repo = project.localPath ?? project.repo ?? "kein Repo";
  const annotationId = await notifyAgent({
    comment:
      `[prototypie project-info ${spaceId}] Projekt ${project.name} (${repo}): Projekt-Info recherchieren und Karten anlegen. ` +
      `Space ${spaceId} hat schon eine Karte „Projekt-Info“ (Name, Repo, Keywords${project.readme ? ", README-Absatz" : ""}). ` +
      `Mit einem project-info-Skill, falls vorhanden; Karten über POST /api/agent/cards (scripts/agent-queue.md). Keine Geheimnisse übernehmen.`,
    element: `space ${spaceId}: ${project.name}`,
    elementPath: project.localPath ?? project.repo ?? "private-projects.ttl",
  });
  if (!annotationId) return Response.json({ error: "No coding agent reachable (is agentation-mcp running on :4747?)" }, { status: 503 });
  return Response.json({ ok: true, annotationId });
}
