import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { linkedDesignIds, type PromotedLinks } from "@/lib/implement-links";
import { notifyAgent } from "@/lib/server/agent-queue";
import { currentImplementNotes, forgetImplementNotes } from "@/lib/server/agent-tasks";
import { dedupedImplement } from "@/lib/server/implement-dedupe";
import { readState } from "@/lib/server/state-store";
import type { Design } from "@/lib/types";

type Body = { designId?: unknown; title?: unknown; rationale?: unknown; html?: unknown; force?: unknown };

/**
 * Hand a design to the coding agent to implement in the product:
 * POST /api/implement { designId, title?, rationale?, html? }.
 * The client sends the design it shows (its edits may not have synced yet); the server's copy is the fallback.
 * Writes the design to data/implement/<id>.html and posts an Agentation note
 * "[prototypie implement <id>] ..." for the watching Claude Code session.
 * One request per design: if the design, its promoted source or a promoted copy already has an open note, nothing is
 * posted ({ ok, duplicate: true, annotationId }); after a resolved one only `force: true` posts again (else 409 with
 * { done: true, annotationId }).
 */
export async function POST(request: Request) {
  const body = (await request.json().catch(() => ({}))) as Body;
  const { designId } = body;
  if (typeof designId !== "string" || !/^[\w-]+$/.test(designId)) {
    return Response.json({ error: "designId required" }, { status: 400 });
  }

  const { records } = await readState();
  const links: PromotedLinks = {};
  for (const [key, rec] of Object.entries(records)) {
    const value = rec.value as { designId?: unknown } | undefined;
    if (key.startsWith("promotedFrom/") && !rec.deleted && typeof value?.designId === "string") {
      links[key.slice("promotedFrom/".length)] = { designId: value.designId };
    }
  }

  let design: Pick<Design, "title" | "rationale" | "html"> | undefined;
  if (typeof body.html === "string" && body.html && typeof body.title === "string") {
    design = { title: body.title, rationale: typeof body.rationale === "string" ? body.rationale : "", html: body.html };
  } else {
    const rec = records[`designs/${designId}`];
    const stored = rec && !rec.deleted ? (rec.value as Design | undefined) : undefined;
    if (!stored) return Response.json({ error: "Unknown design" }, { status: 404 });
    if (stored.status !== "ready" || !stored.html) return Response.json({ error: "Design isn't ready" }, { status: 409 });
    design = stored;
  }

  // Reliable local guard: every sent design carries implementRequestedAt in the shared state. If the design or
  // any promoted source/copy already has it, don't post again (the Agentation scan can miss notes when some of
  // its many session fetches fail).
  if (body.force !== true) {
    const already = linkedDesignIds(links, designId).find((id) => {
      const rec = records[`designs/${id}`];
      return !!rec && !rec.deleted && !!(rec.value as Design | undefined)?.implementRequestedAt;
    });
    if (already) {
      return Response.json(
        { error: "This design (or a linked copy) was already sent to the agent. Send again with force: true.", done: true, designId: already },
        { status: 409 },
      );
    }
  }

  const file = path.join("data", "implement", `${designId}.html`);
  const saved = design;
  let saveError: string | undefined;
  const outcome = await dedupedImplement({
    ids: linkedDesignIds(links, designId),
    force: body.force === true,
    loadNotes: currentImplementNotes,
    notify: async () => {
      // Only written when a note is posted, so a duplicate click doesn't overwrite the HTML the agent is reading.
      try {
        await mkdir(path.dirname(path.join(process.cwd(), file)), { recursive: true });
        await writeFile(path.join(process.cwd(), file), saved.html);
      } catch (err) {
        saveError = err instanceof Error ? err.message : String(err);
        return undefined;
      }
      return notifyAgent({
        comment:
          `[prototypie implement ${designId}] Implement the design "${saved.title}" in the product (this codebase). ` +
          `${saved.rationale ? `Why: ${saved.rationale} ` : ""}The design's HTML is in ${file}.`,
        element: `design ${designId}: ${saved.title}`,
        elementPath: file,
      });
    },
  });

  if (outcome.kind === "duplicate") return Response.json({ ok: true, duplicate: true, annotationId: outcome.annotationId });
  if (outcome.kind === "done") {
    return Response.json(
      { error: "This design was already implemented. Send again with force: true.", done: true, annotationId: outcome.annotationId },
      { status: 409 },
    );
  }
  if (saveError) return Response.json({ error: `Couldn't save the design: ${saveError}` }, { status: 500 });
  if (outcome.kind === "unreachable") {
    return Response.json(
      { error: `No coding agent reachable (is agentation-mcp running on :4747?). The design was saved to ${file}.`, file },
      { status: 503 },
    );
  }
  forgetImplementNotes();
  return Response.json({ ok: true, annotationId: outcome.annotationId, file });
}
