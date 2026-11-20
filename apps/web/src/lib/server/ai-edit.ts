import { z } from "zod";
import { MAX_VERSIONS, type EditableDesign, type EditRequest } from "@/lib/ai-edit";
import type { Variation } from "@/lib/types";
import { enqueueAndWait } from "./agent-queue";
import { describeGuidelines, guidelinesFor } from "./guidelines";
import { cancelJob, startJob } from "./jobs";
import { errorMessage, generateObject } from "./llm";
import { describePodContext } from "./pod-catalog";
import { cleanComponents, describeComponents } from "./components";
import { applyChanges, readState } from "./state-store";

/**
 * In-place AI edit of one card (see src/lib/ai-edit.ts). Runs as a detached job; the result overwrites the card's
 * html in shared state (every open browser updates live) and the old content is kept as a version to undo to.
 */

const EDIT_SYSTEM = `You are a senior product designer who edits HTML prototypes in place.
You get one self-contained HTML document and an instruction. Return the SAME document with only the requested change
applied: keep every other element, style, script, copy and the order exactly as it is (byte-for-byte where possible).
Don't restyle, re-theme, reflow or "improve" anything that wasn't asked for. Keep it self-contained: inline <style>,
optional inline <script>, Google Fonts <link> allowed, no external JS or image URLs.
No explanatory text in the document; put a one-or-two sentence summary of what changed in "rationale".`;

const EditSchema = z.object({ rationale: z.string(), html: z.string() });

const META = { clientId: "server:ai-edit" };

async function current(designId: string): Promise<EditableDesign | undefined> {
  const rec = (await readState()).records[`designs/${designId}`];
  return rec && !rec.deleted ? (rec.value as EditableDesign | undefined) : undefined;
}

async function write(d: EditableDesign, reason: string) {
  await applyChanges([{ path: `designs/${d.id}`, value: d }], { ...META, reason });
}

function describeEdit(d: EditableDesign, req: EditRequest) {
  const parts = [`Design to edit ("${d.title}"):\n<html_document>\n${d.html}\n</html_document>`];
  if (req.focus) parts.push(`Change ONLY this element/region: ${req.focus}. Everything outside it stays identical.`);
  if (req.annotations?.length) {
    parts.push(
      `Notes pinned on the design (element -> note). Address each by changing only the annotated elements:\n${req.annotations
        .map((a, i) => `${i + 1}. ${a.element ? `[${a.element}] ` : ""}${a.comment}`)
        .join("\n")}`,
    );
  }
  const pod = describePodContext(req.podContext);
  if (pod) parts.push(pod);
  const components = describeComponents(req.components);
  if (components) parts.push(components);
  parts.push(`Edit instruction: ${req.instruction}`);
  return parts.join("\n\n");
}

/** Start an edit. Returns the job id, or an error with an HTTP status. */
export async function startEdit(raw: EditRequest): Promise<{ jobId: string } | { error: string; status: number }> {
  const req: EditRequest = { ...raw, components: cleanComponents(raw.components) };
  const d = await current(req.designId);
  if (!d) return { error: "No such design", status: 404 };
  if (d.status !== "ready" || !d.html) return { error: "Only a finished design can be edited", status: 409 };
  if (d.editing) return { error: "This design is already being edited", status: 409 };
  const agent = req.agent ?? "codex";
  const guidelines = await guidelinesFor(d.spaceId);

  const jobId = startJob(async (send, signal, jobId) => {
    send({ type: "plan", directions: [{ title: d.title, brief: req.instruction }] });
    try {
      let v: Variation;
      if (agent === "claude") {
        const item = {
          jobId,
          index: 0,
          mode: "edit" as const,
          targetDesignId: d.id,
          instruction: req.instruction,
          focus: req.focus,
          scope: "page" as const,
          annotations: req.annotations?.length ? req.annotations : undefined,
          direction: { title: `Edit in place: ${d.title}`, brief: req.instruction },
          baseTitle: d.title,
          baseHtml: d.html,
          podContext: req.podContext?.length ? req.podContext : undefined,
          components: req.components,
          guidelines: guidelines ? { cardId: guidelines.cardId, title: guidelines.title, text: guidelines.text } : undefined,
        };
        v = await enqueueAndWait(item, signal);
      } else {
        const out = await generateObject({
          system: guidelines ? `${EDIT_SYSTEM}\n\n${describeGuidelines(guidelines)}` : EDIT_SYSTEM,
          schema: EditSchema,
          description: "Submit the edited document.",
          reasoning: "medium",
          signal,
          prompt: describeEdit(d, req),
        });
        v = { title: d.title, ...out };
      }
      await finish(d.id, jobId, v, req.instruction);
      send({ type: "variation", index: 0, variation: v });
    } catch (err) {
      const message = signal.aborted ? "Cancelled" : errorMessage(err);
      await fail(d.id, jobId, signal.aborted ? undefined : message);
      send({ type: "error", index: 0, message });
    }
  });

  const editing = { jobId, instruction: req.instruction, agent, startedAt: Date.now() };
  await write({ ...d, editing, editError: undefined }, `ai edit start ${jobId}`);
  return { jobId };
}

/** Replace the card's content with the edit result; the old content becomes the newest version. */
async function finish(designId: string, jobId: string, v: Variation, instruction: string) {
  const d = await current(designId);
  if (!d || d.editing?.jobId !== jobId) return; // deleted, cancelled or superseded meanwhile
  const prev = {
    html: d.html,
    title: d.title,
    rationale: d.rationale,
    ...(d.viewport ? { viewport: d.viewport } : {}),
    ...(d.viewportHeight ? { viewportHeight: d.viewportHeight } : {}),
    replacedAt: Date.now(),
    instruction,
  };
  await write(
    {
      ...d,
      html: v.html,
      rationale: v.rationale || d.rationale,
      ...(v.viewport ? { viewport: v.viewport } : {}),
      ...(v.viewportHeight ? { viewportHeight: v.viewportHeight } : {}),
      versions: [...(d.versions ?? []), prev].slice(-MAX_VERSIONS),
      editing: undefined,
      editError: undefined,
    },
    `ai edit done ${jobId}`,
  );
}

async function fail(designId: string, jobId: string, message: string | undefined) {
  const d = await current(designId);
  if (!d || d.editing?.jobId !== jobId) return;
  await write({ ...d, editing: undefined, editError: message }, `ai edit failed ${jobId}`);
}

/** Stop a running edit and clear its busy state (also clears a stale flag left by a dev server restart). */
export async function cancelEdit(designId: string): Promise<boolean> {
  const d = await current(designId);
  if (!d?.editing) return false;
  cancelJob(d.editing.jobId);
  await write({ ...d, editing: undefined }, `ai edit cancelled ${d.editing.jobId}`);
  return true;
}
