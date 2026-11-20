import { listProjects, matchLocalPath, PROJECTS_TTL } from "@/lib/server/projects";

/**
 * The projects for "+ → Space": GET /api/projects -> { available, projects: {id, name, repo, localPath, keywords}[] }.
 * GET /api/projects?match=<space name> -> { localPath? }: the local folder that space name stands for.
 */
export async function GET(request: Request) {
  const match = new URL(request.url).searchParams.get("match");
  if (match != null) return Response.json({ localPath: await matchLocalPath(match) }, { headers: { "Cache-Control": "no-store" } });
  const projects = await listProjects();
  return Response.json(
    { available: projects !== undefined, file: PROJECTS_TTL, projects: projects ?? [] },
    { headers: { "Cache-Control": "no-store" } },
  );
}
