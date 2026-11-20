/**
 * Layout-Finder model and the one renderer behind both the dialog's mini previews and the card it creates:
 * `sceneHtml` draws a layout (matrix, timeline with bands, swimlanes, cards with links) for two shapes and one
 * relation as absolutely positioned HTML (+ one SVG for links). Pure strings, no iframes, all text escaped.
 */

/** How a catalog entry is known: read live, behind login (401), only linked to, or from an app cache with a date. */
export type Access = "live" | "401" | "ref" | "cache";

export interface LfItem {
  id: string;
  label: string;
  /** Epoch ms; only for items whose shape really has a time field (ical:due, event start/end). */
  start?: number;
  end?: number;
}

export interface LfShape {
  id: string;
  name: string;
  /** Type(s) as in the catalog, e.g. "ical:Vtodo · schema:Action". */
  type: string;
  access: Access;
  color: string;
  items: LfItem[];
  /** Pod + path, shown as provenance. */
  source: string;
}

export interface LfRelation {
  a: string;
  b: string;
  predicate: string;
  access: Access;
  /** Set when the edge is only implied through a shared target (e.g. both point at the same course). */
  via?: string;
  /** Item pairs [a item id, b item id] the edge connects. */
  pairs: [string, string][];
  /** Where the edge is documented. */
  evidence?: string;
}

export type LayoutId = "matrix" | "timeline" | "lanes" | "cards";

export const LAYOUTS: { id: LayoutId; name: string }[] = [
  { id: "matrix", name: "Matrix" },
  { id: "timeline", name: "Timeline mit Bändern" },
  { id: "lanes", name: "Swimlanes" },
  { id: "cards", name: "Karten mit Verweisen" },
];

export const ACCESS_LABEL: Record<Access, string> = {
  live: "live · 200",
  cache: "App-Cache",
  ref: "nur Verweis",
  "401": "401",
};

export const esc = (s: string) =>
  String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

export const isTimed = (s: LfShape) => s.items.some((x) => x.start != null);

/** Does relation `r` connect item `ai` of shape A with item `bi` of shape B (in either stored direction)? */
export function related(r: LfRelation | undefined, A: string, ai: string, bi: string): boolean {
  if (!r) return false;
  return r.pairs.some(([x, y]) => (r.a === A ? x === ai && y === bi : x === bi && y === ai));
}

/** Why a layout can't be drawn for this pair, or null when it can. */
export function unavailable(id: LayoutId, a: LfShape, b: LfShape, rel: LfRelation | undefined): string | null {
  if (!rel) return "Keine belegte Beziehung";
  if (id === "timeline" && !isTimed(a) && !isTimed(b)) return "braucht Zeit · ical:due / Termin";
  if (!a.items.length || !b.items.length) return "Keine belegten Einträge";
  return null;
}

const TZ = "Europe/Berlin";
function timeLabel(ms: number, withDay: boolean): string {
  const d = new Date(ms);
  const t = d.toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit", timeZone: TZ });
  if (!withDay) return t;
  return `${d.toLocaleDateString("de-DE", { day: "2-digit", month: "2-digit", timeZone: TZ })} ${t}`;
}
const dayKey = (ms: number) => new Date(ms).toLocaleDateString("de-DE", { timeZone: TZ });

/** Scoped CSS for `.lfm` scenes; colors come from the app's theme tokens, so it works in the dialog and in cards. */
export const SCENE_CSS = `
.lfm{position:relative;overflow:hidden;font-size:var(--lfm-fs,9.5px);line-height:1.2}
.lfm .lab{position:absolute;color:var(--muted-foreground);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.lfm .it{position:absolute;padding:.2em .5em;border-radius:4px;background:var(--card);border:1px solid var(--border);border-left:3px solid var(--c);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;color:var(--foreground)}
.lfm .it.ref{border-style:dashed;border-left-style:solid}
.lfm .band{position:absolute;left:0;right:0;border-top:1px dashed var(--border)}
.lfm .lane{position:absolute;left:4px;right:4px;border-radius:6px;background:color-mix(in oklab,var(--c) 10%,var(--card));border:1px solid var(--border)}
.lfm .cell{position:absolute;border:1px solid var(--border);background:var(--card)}
.lfm .dot{position:absolute;width:10px;height:10px;border-radius:50%;background:var(--c);translate:-50% -50%}
.lfm .axis{position:absolute;left:8px;right:8px;height:1px;background:var(--ring)}
.lfm .tick{position:absolute;width:1px;height:5px;background:var(--ring)}
.lfm svg{position:absolute;inset:0;width:100%;height:100%;overflow:visible}
.lfm svg line{stroke:var(--ring);stroke-width:1.2}
.lfm svg text{fill:var(--muted-foreground);font:calc(var(--lfm-fs,9.5px) - 1px) ui-monospace,Menlo,monospace}
`;

/**
 * Inner HTML of a scene `w`×`h` px; `k` scales the fixed sizes (1 = dialog preview, ~3 = card).
 * Only items that the relation really connects get linked/placed; nothing is invented.
 */
export function sceneHtml(
  id: LayoutId,
  a: LfShape,
  b: LfShape,
  r: LfRelation | undefined,
  w: number,
  h: number,
  k = 1,
): string {
  const it = (s: LfShape, x: number, y: number, width: number, label: string, ref = false) =>
    `<span class="it${ref ? " ref" : ""}" style="--c:${s.color};left:${x}px;top:${y}px;width:${width}px" title="${esc(label)}">${esc(label)}</span>`;
  const lab = (x: number, y: number, text: string, width?: number) =>
    `<span class="lab" style="left:${x}px;top:${y}px${width ? `;width:${width}px` : ""}">${esc(text)}</span>`;

  if (id === "matrix") {
    let out = "";
    const head = 70 * k;
    const cw = Math.min(70 * k, (w - head) / Math.max(1, b.items.length));
    const top = 24 * k;
    const rh = Math.min(28 * k, (h - top) / Math.max(1, a.items.length));
    b.items.forEach((x, j) => (out += lab(head + j * cw, 6 * k, x.label, cw - 4)));
    a.items.forEach((y, i) => {
      out += lab(6 * k, top + i * rh + 6 * k, y.label, head - 10 * k);
      b.items.forEach((x, j) => {
        out += `<span class="cell" style="left:${head + j * cw}px;top:${top + i * rh}px;width:${cw - 2}px;height:${rh - 2}px"></span>`;
        if (related(r, a.id, y.id, x.id))
          out += `<span class="dot" style="--c:${a.color};left:${head + j * cw + cw / 2}px;top:${top + i * rh + rh / 2}px"></span>`;
      });
    });
    return out;
  }

  if (id === "timeline") {
    const both = isTimed(a) && isTimed(b);
    const timedShape = isTimed(a) ? a : b;
    const other = timedShape === a ? b : a;
    const all = [...(isTimed(a) ? a.items : []), ...(isTimed(b) ? b.items : [])].filter((x) => x.start != null);
    if (!all.length) return "";
    const stamps = all.flatMap((x) => [x.start!, x.end].filter((v): v is number => v != null));
    const pad = 20 * 60_000;
    const from = Math.min(...stamps) - pad;
    const to = Math.max(...stamps) + pad;
    const withDay = new Set(stamps.map(dayKey)).size > 1;
    const X = (m: number) => 10 * k + ((m - from) / (to - from)) * (w - 20 * k);
    let out = `<span class="axis" style="top:${h - 22 * k}px"></span>`;
    [...new Set(stamps)]
      .sort((p, q) => p - q)
      .forEach((m, i, arr) => {
        out += `<span class="tick" style="left:${X(m)}px;top:${h - 24 * k}px"></span>`;
        if (i === 0 || X(m) - X(arr[i - 1]) > (withDay ? 52 : 30) * k) out += lab(X(m) - 12 * k, h - 16 * k, timeLabel(m, withDay));
      });
    const block = (s: LfShape, x: LfItem, y: number) =>
      it(s, X(x.start!), y, Math.max(52 * k, x.end ? X(x.end) - X(x.start!) : 56 * k), `${timeLabel(x.start!, false)} ${x.label}`);
    if (both) {
      [a, b].forEach((s, row) => {
        const y = (6 + row * 42) * k;
        out += `<span class="band" style="top:${y + 34 * k}px"></span>${lab(6 * k, y, s.name)}`;
        s.items.filter((x) => x.start != null).forEach((x) => (out += block(s, x, y + 13 * k)));
      });
      return out;
    }
    other.items.forEach((band, i) => {
      const y = (8 + i * 40) * k;
      out += `<span class="band" style="top:${y + 30 * k}px"></span>${lab(6 * k, y, `${other.name}: ${band.label}`)}`;
      timedShape.items
        .filter((x) => x.start != null && related(r, a.id, timedShape === a ? x.id : band.id, timedShape === a ? band.id : x.id))
        .forEach((x) => (out += block(timedShape, x, y + 12 * k)));
    });
    return out;
  }

  if (id === "lanes") {
    let out = "";
    const lh = (h - 8 * k) / Math.max(1, b.items.length);
    b.items.forEach((lane, i) => {
      const y = 4 * k + i * lh;
      out += `<span class="lane" style="--c:${b.color};top:${y}px;height:${lh - 4 * k}px"></span>${lab(10 * k, y + 4 * k, `${b.name} · ${lane.label}`)}`;
      a.items
        .filter((x) => related(r, a.id, x.id, lane.id))
        .forEach((x, j) => (out += it(a, 12 * k + j * 92 * k, y + 20 * k, 86 * k, x.label)));
    });
    return out;
  }

  // cards with links
  const cardW = Math.min(82 * k, (w - 28 * k) * 0.34);
  const ax = 14 * k;
  const bx = w - cardW - 14 * k;
  const step = 34 * k;
  let svg = "";
  a.items.forEach((x, i) =>
    b.items.forEach((y, j) => {
      if (!related(r, a.id, x.id, y.id)) return;
      const y1 = 30 * k + i * step;
      const y2 = 30 * k + j * step;
      svg += `<line x1="${ax + cardW}" y1="${y1}" x2="${bx}" y2="${y2}"/><text text-anchor="middle" x="${(ax + cardW + bx) / 2}" y="${(y1 + y2) / 2 - 4 * k}">${esc(r!.predicate.split(" ")[0])}</text>`;
    }),
  );
  let out = `<svg aria-hidden="true">${svg}</svg>`;
  a.items.forEach((x, i) => (out += it(a, ax, 20 * k + i * step, cardW, x.label, a.access === "ref")));
  b.items.forEach((y, j) => (out += it(b, bx, 20 * k + j * step, cardW, y.label, b.access === "ref")));
  return out;
}

/** Self-contained card document for the chosen layout, with provenance under the scene. */
export function layoutCardHtml(opts: {
  title: string;
  layout: LayoutId;
  a: LfShape;
  b: LfShape;
  rel: LfRelation;
}): string {
  const { title, layout, a, b, rel } = opts;
  const W = 1120;
  const H = 520;
  const k = 2.6;
  const scene = sceneHtml(layout, a, b, rel, W, H, k);
  const name = LAYOUTS.find((l) => l.id === layout)?.name ?? layout;
  const row = (label: string, value: string) => `<div class="row"><dt>${esc(label)}</dt><dd>${value}</dd></div>`;
  const src = (s: LfShape) => `<code>${esc(s.source)}</code> <span class="tag">${esc(ACCESS_LABEL[s.access])}</span>`;
  return `<!doctype html><html lang="de"><head><meta charset="utf-8"><title>${esc(title)}</title>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Google+Sans+Flex:wght@400;500;600;700&display=swap">
<style>
:root{--background:#fff;--foreground:#18181b;--muted-foreground:#71717a;--border:#e4e4e7;--card:#fff;--ring:#a1a1aa;--muted:#f4f4f5}
@media (prefers-color-scheme:dark){:root{--background:#0a0a0a;--foreground:#fafafa;--muted-foreground:#a1a1aa;--border:#27272a;--card:#141414;--ring:#52525b;--muted:#18181b}}
*{box-sizing:border-box;margin:0}
body{background:var(--background);color:var(--foreground);font:15px/1.5 "Google Sans Flex",system-ui,sans-serif;padding:40px}
.k{font-size:12px;font-weight:600;letter-spacing:.08em;text-transform:uppercase;color:var(--muted-foreground)}
h1{font-size:30px;font-weight:650;margin:6px 0 20px;overflow-wrap:anywhere}
.stage{width:${W}px;max-width:100%;height:${H}px;border:1px solid var(--border);border-radius:14px;background:var(--muted);--lfm-fs:15px}
dl{display:grid;gap:8px;margin-top:22px;max-width:${W}px}
.row{display:grid;grid-template-columns:150px 1fr;gap:12px}
dt{color:var(--muted-foreground)}
dd{overflow-wrap:anywhere}
code{font:13px ui-monospace,SFMono-Regular,Menlo,monospace}
.tag{display:inline-block;margin-left:6px;padding:0 8px;border-radius:999px;background:var(--muted);border:1px solid var(--border);font-size:12px}
${SCENE_CSS}
</style></head><body>
<div class="k">${esc(name)}</div>
<h1>${esc(title)}</h1>
<div class="stage lfm" role="img" aria-label="${esc(`${name}: ${a.name} und ${b.name} über ${rel.predicate}`)}">${scene}</div>
<dl>
${row(a.name, src(a))}
${row(b.name, src(b))}
${row("Beziehung", `<code>${esc(rel.predicate)}</code>${rel.via ? ` über ${esc(rel.via)}` : ""} <span class="tag">${esc(ACCESS_LABEL[rel.access])}</span>`)}
${rel.evidence ? row("Beleg", esc(rel.evidence)) : ""}
</dl>
</body></html>`;
}
