/**
 * A promoted copy and its source are the same decision: an Implement request for one counts for the other.
 * Shared by /api/implement (dedupe) and the client (Implement button, Inspector progress).
 */

/** Promoted copy id -> the design it was copied from (the `promotedFrom` map, or its `promotedFrom/<id>` records). */
export type PromotedLinks = Record<string, { designId: string } | undefined>;

/** `designId` plus every design linked to it through promotion, in both directions and across several levels. */
export function linkedDesignIds(links: PromotedLinks, designId: string): string[] {
  const out = new Set([designId]);
  const stack = [designId];
  while (stack.length) {
    const cur = stack.pop()!;
    const source = links[cur]?.designId;
    if (source && !out.has(source)) {
      out.add(source);
      stack.push(source);
    }
    for (const [copyId, ref] of Object.entries(links)) {
      if (ref?.designId === cur && !out.has(copyId)) {
        out.add(copyId);
        stack.push(copyId);
      }
    }
  }
  return [...out];
}
