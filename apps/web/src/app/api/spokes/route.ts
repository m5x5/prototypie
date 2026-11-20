import { z } from "zod";
import { errorMessage, generateObject } from "@/lib/server/llm";

// Owned by the "spokes" feature: break a design (usually a full-page "block of clay") into explorable components.

export const maxDuration = 120;

const MAX_HTML = 150_000;

const ComponentsSchema = z.object({
  components: z.array(
    z.object({
      name: z.string(),
      description: z.string(),
      selector: z.string(),
    }),
  ),
});

export type SpokeComponent = z.infer<typeof ComponentsSchema>["components"][number];

export interface SpokesRequest {
  html: string;
  title?: string;
}

export type SpokesResponse = { components: SpokeComponent[] } | { error: string };

export async function POST(request: Request) {
  let body: SpokesRequest;
  try {
    body = (await request.json()) as SpokesRequest;
  } catch {
    return Response.json({ error: "Invalid JSON body." } satisfies SpokesResponse, { status: 400 });
  }
  if (!body.html?.trim()) {
    return Response.json({ error: "No page HTML to analyse." } satisfies SpokesResponse, { status: 400 });
  }

  try {
    const out = await generateObject({
      system:
        "You are a senior product designer. You look at a rough prototype (usually a full page, sometimes a single section or component) and break it into the individual components a designer would iterate on one at a time (hero, primary CTA, pricing card, nav, testimonial, form, footer...).",
      prompt: `Page${body.title ? ` ("${body.title}")` : ""}:\n<page_html>\n${body.html.slice(0, MAX_HTML)}\n</page_html>\n\nIdentify between 3 and 8 distinct, self-contained components on this page that are worth exploring in isolation, most impactful first. If it is a single small component, list its parts instead (2 or more; never invent parts that are not there). For each give:\n- name: 1-4 words\n- description: one sentence describing what it is and what it contains, specific enough to find it on the page\n- selector: a CSS selector or short locator for it in the HTML (e.g. "header nav", "section.pricing .card:first-child")`,
      schema: ComponentsSchema,
      description: "Submit the page's components.",
      reasoning: "low",
      signal: request.signal,
    });
    const components = out.components.slice(0, 8);
    if (!components.length) {
      return Response.json({ error: "The model found no components." } satisfies SpokesResponse, { status: 502 });
    }
    return Response.json({ components } satisfies SpokesResponse);
  } catch (err) {
    return Response.json({ error: errorMessage(err) } satisfies SpokesResponse, { status: 502 });
  }
}
