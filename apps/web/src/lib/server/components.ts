import type { ComponentRef } from "@/lib/types";

/** Most #-mentioned components passed on per request, and the longest html kept for each. */
export const MAX_COMPONENTS = 12;
const MAX_HTML = 120_000;

/** Keep only well-formed refs with html, at most MAX_COMPONENTS, each html capped. */
export function cleanComponents(list: ComponentRef[] | undefined): ComponentRef[] | undefined {
  const out = (list ?? [])
    .filter((c) => c && typeof c.id === "string" && typeof c.title === "string" && typeof c.html === "string" && c.html.trim())
    .slice(0, MAX_COMPONENTS)
    .map((c) => ({ id: c.id, title: c.title.slice(0, 200), html: c.html.slice(0, MAX_HTML) }));
  return out.length ? out : undefined;
}

/** Prompt section for #-mentioned components: reuse them exactly instead of redesigning them. */
export function describeComponents(list: ComponentRef[] | undefined): string | undefined {
  if (!list?.length) return undefined;
  return (
    `Reference components the designer #-mentioned. Wherever the design needs one of these, reuse it EXACTLY as given ` +
    `(same markup, styles, tokens, icons, copy and sizes); only adapt what placement requires (width, outer spacing). ` +
    `Don't redesign, restyle or re-theme them, and don't show them on their own as a reference sheet:\n\n` +
    list.map((c, i) => `Component ${i + 1} ("${c.title}", card ${c.id}):\n<component_html>\n${c.html}\n</component_html>`).join("\n\n")
  );
}
