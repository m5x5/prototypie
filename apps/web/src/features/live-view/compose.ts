import type { Design } from "@/lib/types";

/**
 * Live views: a card whose html places other cards by reference instead of copying them.
 *
 *   <prototypie-slot data-design="<design id>" style="..."></prototypie-slot>
 *
 * At render time each slot shows the CURRENT html of that design's main variant: the newest card marked as main
 * (`Design.mainAt`) among the design and its variations (parentId descendants), else the design itself. In-place
 * edits change a card's html, so a live view always shows the latest version of every component.
 *
 * A slot becomes a div keeping its own attributes (class/style lay it out) with the component in an iframe, so the
 * component keeps its own styles and scripts. The iframe grows to its content's height; `data-fit="fill"` makes it
 * fill the slot instead (give the slot a height), `data-height="N"` fixes it to N px.
 */

const SLOT_RE = /<prototypie-slot\b([^>]*)>(?:\s*<\/prototypie-slot>)?/gi;
const ID_RE = /\bdata-design\s*=\s*["']?([\w-]+)/i;
const MAX_DEPTH = 3;

export const hasSlots = (html: string | undefined): boolean => !!html && /<prototypie-slot\b/i.test(html);

/** Ids referenced by the slots in `html`, in order. */
export function slotIds(html: string): string[] {
  const ids: string[] = [];
  for (const m of html.matchAll(SLOT_RE)) {
    const id = ID_RE.exec(m[1])?.[1];
    if (id) ids.push(id);
  }
  return ids;
}

/** The card a slot for `id` shows: the newest main-marked card among `id` and its variations, else `id` itself. */
export function mainVariant(designs: Record<string, Design>, id: string): Design | undefined {
  const root = designs[id];
  if (!root) return undefined;
  const children = childIndex(designs);
  let best: Design | undefined;
  const seen = new Set<string>();
  const stack = [id];
  while (stack.length) {
    const cur = stack.pop()!;
    if (seen.has(cur)) continue;
    seen.add(cur);
    const d = designs[cur];
    // A live view derived from the component (e.g. created from it) is not one of its variants: skip it and its branch.
    if (cur !== id && hasSlots(d?.html)) continue;
    if (d?.mainAt && d.status === "ready" && (!best || d.mainAt > (best.mainAt ?? 0))) best = d;
    for (const c of children.get(cur) ?? []) stack.push(c);
  }
  return best ?? root;
}

const childCache = new WeakMap<Record<string, Design>, Map<string, string[]>>();
function childIndex(designs: Record<string, Design>): Map<string, string[]> {
  let m = childCache.get(designs);
  if (m) return m;
  m = new Map();
  for (const d of Object.values(designs)) {
    if (!d.parentId) continue;
    const list = m.get(d.parentId);
    if (list) list.push(d.id);
    else m.set(d.parentId, [d.id]);
  }
  childCache.set(designs, m);
  return m;
}

/** Ids of every design that is a slot target somewhere, or a variation of one (so it can be made main). */
export function slotLineage(designs: Record<string, Design>): Set<string> {
  const cached = lineageCache.get(designs);
  if (cached) return cached;
  const targets = new Set<string>();
  for (const d of Object.values(designs)) if (hasSlots(d.html)) for (const id of slotIds(d.html)) targets.add(id);
  const children = childIndex(designs);
  const out = new Set<string>();
  const stack = [...targets];
  while (stack.length) {
    const cur = stack.pop()!;
    if (out.has(cur) || (!targets.has(cur) && hasSlots(designs[cur]?.html))) continue;
    out.add(cur);
    for (const c of children.get(cur) ?? []) stack.push(c);
  }
  lineageCache.set(designs, out);
  return out;
}
const lineageCache = new WeakMap<Record<string, Design>, Set<string>>();

// < and > too: an unescaped srcdoc would contain the component's own </body>, which the outer document is searched for.
const attr = (s: string) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

const SIZER = `<style data-prototypie-live>.pt-slot>iframe{display:block;width:100%;border:0;background:transparent}.pt-slot[data-fit=fill]>iframe{height:100%}</style>
<script data-prototypie-live>(()=>{const fit=f=>{try{const d=f.contentDocument;if(!d||!d.body)return;const h=Math.max(d.documentElement.scrollHeight,d.body.scrollHeight,...[...d.body.children].map(c=>c.getBoundingClientRect().top+c.scrollHeight));if(h>0)f.style.height=Math.ceil(h)+'px'}catch(e){}};
const hook=f=>{const s=f.parentElement;if(!s||s.dataset.fit==='fill'||s.dataset.height)return;const go=()=>{fit(f);try{new ResizeObserver(()=>fit(f)).observe(f.contentDocument.body)}catch(e){}};f.addEventListener('load',go);if(f.contentDocument&&f.contentDocument.readyState==='complete')go()};
document.querySelectorAll('.pt-slot>iframe').forEach(hook)})()</script>`;

/**
 * Replaces every slot in `html` with the current html of its main variant (recursively, a few levels deep).
 * `wrap` can adjust each component's document (e.g. freeze animations in thumbnails).
 */
export function composeLive(
  html: string,
  designs: Record<string, Design>,
  wrap?: (componentHtml: string) => string,
  depth = 0,
): string {
  if (!hasSlots(html)) return html;
  const out = html.replace(SLOT_RE, (_m, attrs: string) => {
    const id = ID_RE.exec(attrs)?.[1];
    const d = id ? mainVariant(designs, id) : undefined;
    const fixed = /\bdata-height\s*=\s*["']?(\d+)/i.exec(attrs)?.[1];
    let inner = d?.status === "ready" && d.html ? d.html : "";
    if (inner && depth < MAX_DEPTH) inner = composeLive(inner, designs, wrap, depth + 1);
    if (inner && wrap) inner = wrap(inner);
    const frame = inner
      ? `<iframe title="${attr(d!.title)}" srcdoc="${attr(inner)}"${fixed ? ` style="height:${fixed}px"` : ""}></iframe>`
      : "";
    const cls = /\bclass\s*=\s*"([^"]*)"/i.exec(attrs);
    const rest = cls ? attrs.replace(cls[0], "") : attrs;
    return `<div class="pt-slot${cls ? ` ${cls[1]}` : ""}"${rest}>${frame}</div>`;
  });
  const i = out.toLowerCase().lastIndexOf("</body>");
  return i >= 0 ? out.slice(0, i) + SIZER + out.slice(i) : out + SIZER;
}

/**
 * Same as composeLive, but reuses the previous result while none of the referenced component cards changed
 * (the store changes on every drag; composing re-escapes every component's html).
 */
const memo = new Map<string, { parts: (string | undefined)[]; out: string }>();
export function composeLiveCached(
  key: string,
  html: string,
  designs: Record<string, Design>,
  wrap?: (componentHtml: string) => string,
): string {
  if (!hasSlots(html)) return html;
  const parts = collectParts(html, designs, 0);
  const prev = memo.get(key);
  if (prev && prev.parts.length === parts.length && prev.parts.every((p, i) => p === parts[i])) return prev.out;
  const out = composeLive(html, designs, wrap);
  memo.set(key, { parts, out });
  if (memo.size > 200) memo.delete(memo.keys().next().value!);
  return out;
}

function collectParts(html: string, designs: Record<string, Design>, depth: number): (string | undefined)[] {
  const parts: (string | undefined)[] = [html];
  for (const id of slotIds(html)) {
    const d = mainVariant(designs, id);
    const h = d?.status === "ready" ? d.html : undefined;
    parts.push(h);
    if (h && depth < MAX_DEPTH && hasSlots(h)) parts.push(...collectParts(h, designs, depth + 1));
  }
  return parts;
}

/** Starting html for a new live view: the components stacked full width, in the given order. */
export function defaultLiveHtml(ids: string[], viewport: number): string {
  const slots = ids.map((id) => `<prototypie-slot data-design="${id}"></prototypie-slot>`).join("\n");
  return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=${viewport}"><style>html,body{margin:0;background:#fff}main{display:flex;flex-direction:column}</style></head><body><main>
${slots}
</main></body></html>`;
}
