import { getPodCatalog } from "@/lib/server/pod-catalog";

/** GET /api/pod-catalog[?refresh=1]: pods, containers, shapes and known resources with their access status. */
export async function GET(request: Request) {
  const refresh = new URL(request.url).searchParams.get("refresh") === "1";
  try {
    return Response.json(await getPodCatalog({ refresh }));
  } catch (err) {
    return Response.json({ error: err instanceof Error ? err.message : String(err) }, { status: 500 });
  }
}
