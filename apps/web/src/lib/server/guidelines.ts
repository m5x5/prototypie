import type { Design, Space } from "@/lib/types";
import { readState } from "./state-store";

/** A space's design guidelines: one card marked with `guidelinesAt`, inherited by nested spaces without their own. */
export interface Guidelines {
  cardId: string;
  title: string;
  /** Spaces from the one being designed in up to the one that owns the guidelines (names). */
  fromSpace: string;
  text: string;
}

const MAX_TEXT = 12000;

/** The guidelines that apply in `spaceId`: its own card, else the nearest ancestor's (up to the main canvas). */
export async function guidelinesFor(spaceId: string | undefined): Promise<Guidelines | undefined> {
  if (!spaceId) return undefined;
  let records: Awaited<ReturnType<typeof readState>>["records"];
  try {
    records = (await readState()).records;
  } catch {
    return undefined;
  }
  const live = <T>(prefix: string) =>
    Object.entries(records)
      .filter(([p, r]) => p.startsWith(prefix) && !r.deleted && r.value !== undefined)
      .map(([, r]) => r.value as T);
  const spaces = new Map(live<Space>("spaces/").map((s) => [s.id, s]));
  // Newest mark wins if two browsers set one at the same time.
  const bySpace = new Map<string, Design>();
  for (const d of live<Design>("designs/")) {
    if (!d.guidelinesAt || d.status !== "ready" || !d.html) continue;
    const cur = bySpace.get(d.spaceId);
    if (!cur || d.guidelinesAt > (cur.guidelinesAt ?? 0)) bySpace.set(d.spaceId, d);
  }
  const seen = new Set<string>();
  for (let cur: string | undefined = spaceId; cur && !seen.has(cur); ) {
    seen.add(cur);
    const card = bySpace.get(cur);
    if (card) {
      return {
        cardId: card.id,
        title: card.title,
        fromSpace: cur === "root" ? "Main canvas" : (spaces.get(cur)?.name ?? cur),
        text: htmlToText(card.html).slice(0, MAX_TEXT),
      };
    }
    cur = spaces.get(cur)?.parentSpaceId ?? (cur === "root" ? undefined : "root");
  }
  return undefined;
}

/** Readable text of an HTML document: no head/style/script, block elements on their own lines, entities decoded. */
export function htmlToText(html: string): string {
  return html
    .replace(/<(head|style|script|noscript|svg)\b[\s\S]*?<\/\1>/gi, " ")
    .replace(/<!--[\s\S]*?-->/g, " ")
    .replace(/<(br|hr)\b[^>]*>/gi, "\n")
    .replace(/<\/?(p|div|section|article|header|footer|main|aside|nav|h[1-6]|li|ul|ol|dl|dt|dd|tr|table|blockquote|pre|figure|figcaption)\b[^>]*>/gi, "\n")
    .replace(/<(td|th)\b[^>]*>/gi, " | ")
    .replace(/<[^>]+>/g, " ")
    .replace(/&nbsp;/g, " ")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;|&apos;/g, "'")
    .replace(/&#(\d+);/g, (_, n) => String.fromCodePoint(Number(n)))
    .replace(/&#x([0-9a-f]+);/gi, (_, n) => String.fromCodePoint(parseInt(n, 16)))
    .replace(/&amp;/g, "&")
    .replace(/[ \t\f\v\r]+/g, " ")
    .replace(/ *\n */g, "\n")
    .replace(/\n{2,}/g, "\n")
    .trim();
}

/** The guidelines as a hard constraint for the LLM's system prompt. */
export function describeGuidelines(g: Guidelines | undefined): string {
  if (!g) return "";
  return `Design guidelines for this space ("${g.title}", set in ${g.fromSpace}). They frame every design made here: follow them strictly; they override the base design and your own taste wherever they conflict (only an explicit instruction from the designer may override them):
<design_guidelines>
${g.text}
</design_guidelines>`;
}
