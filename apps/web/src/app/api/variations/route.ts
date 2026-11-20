import { z } from "zod";
import type { VariationEvent, VariationRequest } from "@/lib/types";
import { errorMessage, generateObject } from "@/lib/server/llm";
import { jobStream, startJob } from "@/lib/server/jobs";
import { enqueueAndWait, saveScreenshot } from "@/lib/server/agent-queue";
import { describePodContext } from "@/lib/server/pod-catalog";
import { cleanComponents, describeComponents } from "@/lib/server/components";
import { describeGuidelines, guidelinesFor, type Guidelines } from "@/lib/server/guidelines";

export const maxDuration = 300;

const PlanSchema = z.object({
  directions: z.array(z.object({ title: z.string(), brief: z.string() })),
});

const VariationSchema = z.object({
  title: z.string(),
  rationale: z.string(),
  html: z.string(),
});

/** Card titles live inside a space (= the project), so they never repeat it. */
const TITLE_RULE = `Titles are short and space-scoped: 1-4 words describing the design (e.g. "Werkzeugleiste", "Kompakte Sidebar"). Never include the project, product or space name, and never words like "Komponente", "Component", "Variant" or "Direction".`;

const DESIGN_SYSTEM = `You are a senior product designer who prototypes in code.
You produce self-contained HTML documents: inline <style>, optional inline <script> for interactivity, no external JS.
Google Fonts <link> tags are allowed. Use real-feeling copy, not lorem ipsum. Images: use CSS gradients, SVG or emoji, never external image URLs.
Icons: inline SVG. If Lucide icons are asked for, inline the real Lucide SVG markup (24x24 viewBox, fill="none", stroke="currentColor", stroke-width="2", round caps and joins).
The document is rendered in a sandboxed iframe. Make it responsive and polished; interactive states (hover, focus, toggles, tabs) are encouraged.
When a base design is given, keep its product, content and visual language unless the instruction says otherwise.
Hard rules: design only the component the instruction talks about and focus on it; don't add or redesign anything else.
No explanatory text in the document (no headings, captions, labels, spec lists, legends or annotations about the design); put reasoning in "rationale".
Each output is ONE variant: never show several options, states or versions side by side in one document.
${TITLE_RULE}`;

function describeTask(req: VariationRequest) {
  const parts: string[] = [];
  if (req.bases?.length) {
    parts.push(
      `Combine these ${req.bases.length} designs into ONE new design. Take the strongest ideas from each (layout, hierarchy, components, interactions, visual language) and merge them into a coherent whole; don't place them side by side. Keep their shared product and content.\n\n` +
        req.bases.map((b, i) => `Design ${i + 1}${b.title ? ` ("${b.title}")` : ""}:\n<base_html>\n${b.html}\n</base_html>`).join("\n\n"),
    );
  } else if (req.baseHtml) {
    parts.push(`Base design${req.baseTitle ? ` ("${req.baseTitle}")` : ""}:\n<base_html>\n${req.baseHtml}\n</base_html>`);
  }
  if (req.focus) {
    parts.push(
      req.scope === "component"
        ? `Focus ONLY on this component/region: ${req.focus}. Each output document must contain just that component, centred on a neutral backdrop that matches the base design's styling, so it can be judged in isolation.`
        : `Change ONLY this component/region: ${req.focus}. Return the full page, but everything outside it must stay identical to the base HTML (same markup, styles, copy and order; byte-for-byte where possible). Don't restyle, reflow or re-theme the rest of the page. Directions differ only in how they treat this region.`,
    );
  }
  if (req.annotations?.length) {
    parts.push(
      `Feedback pinned on the base design with Agentation (element -> note). Address every note by changing only the annotated elements (and their children); anything not annotated stays exactly as in the base design. Keep supporting CSS/JS changes minimal and scoped to those elements:\n${req.annotations
        .map((a, i) => `${i + 1}. ${a.element ? `[${a.element}] ` : ""}${a.comment}`)
        .join("\n")}`,
    );
  }
  const pod = describePodContext(req.podContext);
  if (pod) parts.push(pod);
  const components = describeComponents(req.components);
  if (components) parts.push(components);
  parts.push(`Instruction from the designer: ${req.instruction || "Explore meaningfully different directions."}`);
  return parts.join("\n\n");
}

export async function POST(request: Request) {
  const raw = (await request.json()) as VariationRequest;
  const body: VariationRequest = { ...raw, components: cleanComponents(raw.components) };
  const count = Math.min(Math.max(Math.round(body.count || 3), 1), 8);
  const task = describeTask(body);
  const guidelines = await guidelinesFor(body.spaceId);
  // The space's guidelines are a system-level constraint for every call of this job.
  const system = guidelines ? `${DESIGN_SYSTEM}\n\n${describeGuidelines(guidelines)}` : DESIGN_SYSTEM;

  // Runs as a detached server-side job: a page reload doesn't stop it, the client just reconnects.
  const id = startJob(async (send, signal, jobId) => {
    if (body.importFrom) {
      if (body.agent === "claude") return runImportViaClaude(body, body.importFrom, send, signal, jobId, guidelines);
      // Recreating an existing element means reading its source code: only the Claude agent can do that.
      const direction = { title: body.importFrom.element.trim().slice(0, 200), brief: body.importFrom.source };
      send({ type: "plan", directions: [direction] });
      send({ type: "error", index: 0, message: "Importing from the app needs the Claude agent (switch the inspector's agent to Claude)." });
      return;
    }
    if (body.agent === "claude") return runViaClaude(body, count, send, signal, jobId, guidelines);
    // 1. Plan N distinct directions (fast), so the parallel generations don't converge.
    //    A retry already knows its direction and skips this.
    const plan = body.direction ? { directions: [body.direction] } : await generateObject({
      system,
      schema: PlanSchema,
      description: "Submit the design directions.",
      reasoning: "low",
      signal,
      prompt:
        count === 1
          ? `${task}\n\nPropose exactly 1 design direction: a short title (1-4 words, no project/space name, no "Komponente"/"Component") and a 1-3 sentence brief.`
          : `${task}\n\nPropose exactly ${count} clearly distinct design directions (layout, hierarchy, tone, interaction - not just colour swaps). For each give a short title (1-4 words, no project/space name, no "Komponente"/"Component") and a 1-3 sentence brief.`,
    });
    const directions = plan.directions.slice(0, count);
    send({ type: "plan", directions });

    // 2. Generate every direction in parallel; each result is emitted as soon as it lands.
    await Promise.all(
      directions.map(async (dir, index) => {
        try {
          const v = await generateObject({
            system,
            schema: VariationSchema,
            description: "Submit the finished design.",
            reasoning: "medium",
            signal,
            prompt: `${task}\n\nBuild this direction:\nTitle: ${dir.title}\nBrief: ${dir.brief}\n\nSubmit the title, a one-or-two sentence rationale aimed at a stakeholder, and the complete HTML document.`,
          });
          send({ type: "variation", index, variation: v });
        } catch (err) {
          send({ type: "error", index, message: errorMessage(err) });
        }
      }),
    );
  });
  return jobStream(id);
}

/**
 * "claude" agent: no LLM call here. Each direction becomes a work item in data/agent-queue/ that a Claude Code
 * session picks up (scripts/agent-queue.md) and answers via POST /api/agent-queue/<jobId>/<index>.
 */
async function runViaClaude(
  body: VariationRequest,
  count: number,
  send: (e: VariationEvent) => void,
  signal: AbortSignal,
  jobId: string,
  guidelines: Guidelines | undefined,
) {
  const directions = body.direction
    ? [body.direction]
    : Array.from({ length: count }, (_, i) => ({
        title: `Direction ${i + 1}`,
        brief: body.instruction || "Explore a meaningfully different direction.",
      }));
  send({ type: "plan", directions });
  await Promise.all(
    directions.map(async (direction, index) => {
      try {
        const variation = await enqueueAndWait(
          {
            jobId,
            index,
            instruction: body.instruction,
            focus: body.focus,
            scope: body.scope,
            annotations: body.annotations,
            direction,
            baseTitle: body.baseTitle,
            baseHtml: body.bases?.length ? undefined : body.baseHtml,
            bases: body.bases?.length ? body.bases : undefined,
            podContext: body.podContext?.length ? body.podContext : undefined,
            components: body.components,
            guidelines: guidelines ? { cardId: guidelines.cardId, title: guidelines.title, text: guidelines.text } : undefined,
          },
          signal,
        );
        send({ type: "variation", index, variation });
      } catch (err) {
        send({ type: "error", index, message: errorMessage(err) });
      }
    }),
  );
}

/**
 * "+ → Import from app…" with the Claude agent: one queue item in import mode. The agent reads the app's source
 * (read-only) and recreates the existing element faithfully as a component-scope card (scripts/agent-queue.md).
 */
async function runImportViaClaude(
  body: VariationRequest,
  from: NonNullable<VariationRequest["importFrom"]>,
  send: (e: VariationEvent) => void,
  signal: AbortSignal,
  jobId: string,
  guidelines: Guidelines | undefined,
) {
  const element = from.element.trim().slice(0, 200);
  const source = from.source.trim().slice(0, 2000);
  const direction = { title: element, brief: `Recreate "${element}" exactly as it is in ${source}.` };
  send({ type: "plan", directions: [direction] });
  try {
    const shot = from.screenshot ? await saveScreenshot(jobId, from.screenshot) : undefined;
    const variation = await enqueueAndWait(
      {
        jobId,
        index: 0,
        mode: "import",
        element,
        importSource: source,
        importScreenshot: shot,
        instruction:
          body.instruction ||
          `Import the existing "${element}" from the app at ${source}: recreate it faithfully from its source code (structure, tokens, real copy, real sizes and viewport). Don't redesign or improve it.`,
        focus: element,
        scope: "component",
        direction,
        // The space's guidelines ride along as context (scripts/agent-queue.md "Import items"): the import still copies
        // what the app looks like today; they only fill values the source leaves open.
        guidelines: guidelines ? { cardId: guidelines.cardId, title: guidelines.title, text: guidelines.text } : undefined,
      },
      signal,
    );
    send({ type: "variation", index: 0, variation });
  } catch (err) {
    send({ type: "error", index: 0, message: errorMessage(err) });
  }
}
