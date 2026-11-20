import { z } from "zod";
import { errorMessage, generateObject, LLMError } from "@/lib/server/llm";
import { DecisionsRequestSchema, issueMessage, type DecisionsResponse, type SuggestedDecision } from "@/features/poll/schema";

export const maxDuration = 120;

const TOTAL_HTML_BUDGET = 90_000; // characters of HTML across all candidates

const OutputSchema = z.object({
  decisions: z.array(
    z.object({
      question: z.string(),
      why: z.string(),
      options: z.array(z.object({ designId: z.string(), label: z.string() })),
    }),
  ),
});

const SYSTEM = `You are a design lead helping a product designer prepare a decision review for stakeholders.
The designer has explored many competing HTML prototypes. Stakeholders should not wade through the messy exploration;
they should answer a few crisp, consequential questions by comparing concrete options side by side.

Your job: look at the candidate designs and surface the 1-4 core decisions the designer is actually grappling with
(e.g. "How prominent should pricing be?", "Guided wizard or single long form?", "Playful or corporate tone?").
For each decision:
- question: a short, plain-language question a non-designer can answer (max ~12 words).
- why: one sentence on why this choice matters for users or the business.
- options: 2-4 candidate designs that best embody the competing answers, each with a 2-6 word label that names the
  answer it represents (e.g. "Pricing up front", "Pricing after value"), not the design's title.
Only use designId values from the provided candidates. A design may appear in several decisions if it is a clear
example of different answers, but prefer distinct designs. Order decisions from most to least consequential.
Skip trivial differences (colour tweaks, copy typos). If the candidates only really differ along one axis, return one decision.`;

/** Strip what the model doesn't need (scripts, data URIs, whitespace) and cap the length. */
function condense(html: string, max: number) {
  const s = html
    .replace(/<script[\s\S]*?<\/script>/gi, "<script>…</script>")
    .replace(/<svg[\s\S]*?<\/svg>/gi, "<svg>…</svg>")
    .replace(/data:[^"')\s]{80,}/g, "data:…")
    .replace(/<!--[\s\S]*?-->/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return s.length > max ? `${s.slice(0, max)} …[truncated]` : s;
}

export async function POST(request: Request) {
  let json: unknown;
  try {
    json = await request.json();
  } catch {
    return Response.json({ error: "Body must be JSON." }, { status: 400 });
  }
  const parsed = DecisionsRequestSchema.safeParse(json);
  if (!parsed.success) {
    return Response.json({ error: issueMessage(parsed.error, "Invalid request.") }, { status: 400 });
  }
  const { designs, context } = parsed.data;
  const perDesign = Math.max(2_000, Math.min(16_000, Math.floor(TOTAL_HTML_BUDGET / designs.length)));

  const candidates = designs
    .map(
      (d) =>
        `<candidate designId="${d.id}">\n<title>${d.title}</title>\n<rationale>${d.rationale}</rationale>\n<html>${condense(d.html, perDesign)}</html>\n</candidate>`,
    )
    .join("\n\n");

  try {
    const output = await generateObject({
      system: SYSTEM,
      schema: OutputSchema,
      description: "Submit the core decisions for the stakeholder poll.",
      reasoning: "medium",
      signal: request.signal,
      prompt: `${candidates}\n\n${context ? `Note from the designer: ${context}\n\n` : ""}Surface the core decisions for the stakeholder poll.`,
    });

    // Keep only real candidate ids, dedupe options, drop decisions that no longer compare anything.
    const known = new Set(designs.map((d) => d.id));
    const decisions: SuggestedDecision[] = output.decisions
      .map((d) => {
        const seen = new Set<string>();
        const options = d.options
          .filter((o) => known.has(o.designId) && !seen.has(o.designId) && seen.add(o.designId))
          .slice(0, 4)
          .map((o) => ({ designId: o.designId, label: o.label.trim() || "Option" }));
        return { question: d.question.trim(), why: d.why.trim(), options };
      })
      .filter((d) => d.question && d.options.length >= 2)
      .slice(0, 4);
    if (!decisions.length) {
      return Response.json({ error: "The agent couldn't find a clear decision between these designs." }, { status: 422 });
    }
    const body: DecisionsResponse = { decisions };
    return Response.json(body);
  } catch (err) {
    if (request.signal.aborted) return Response.json({ error: "Request cancelled." }, { status: 499 });
    // LLMError = not logged in / model problem: a configuration issue the designer can act on.
    const status = err instanceof LLMError ? 503 : 500;
    return Response.json({ error: errorMessage(err) }, { status });
  }
}
