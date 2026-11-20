"use client";
import { useApp } from "@/store";
import { NODE_H, NODE_W, SIBLING_GAP } from "@/store/core";

/**
 * One way to generate from source cards, used by the selection bar ("Combine") and by draft cards (drag a handle
 * onto empty canvas): one ready source = variations of it; two or more = ONE request carrying all of them as
 * `bases`, so the agent combines them. Results land in a row at `origin` (default: centred below the sources),
 * edged from the first source (the canvas also draws edges from the other bases).
 * Returns false when there's nothing ready to generate from.
 */
export function generateFromSources(opts: {
  sourceIds: string[];
  instruction: string;
  count: number;
  origin?: { x: number; y: number };
}): boolean {
  const s = useApp.getState();
  const ready = opts.sourceIds.map((id) => s.designs[id]).filter((d) => d?.status === "ready");
  if (!ready.length) return false;
  const count = Math.max(1, Math.round(opts.count));
  const text = opts.instruction.trim();
  const first = ready[0];
  const width = count * NODE_W + (count - 1) * SIBLING_GAP;
  const origin =
    opts.origin ??
    (() => {
      const centre = ready.reduce((sum, d) => sum + d.position.x, 0) / ready.length + NODE_W / 2;
      const bottom = Math.max(...ready.map((d) => d.position.y));
      return { x: centre - width / 2, y: bottom + NODE_H + 220 };
    })();
  if (ready.length === 1) {
    void s.generate({
      spaceId: first.spaceId,
      parentId: first.id,
      kind: "variation",
      origin,
      label: `${(text || "Variation").slice(0, 40)}: ${first.title}`,
      request: {
        instruction: text || "Explore a meaningfully different direction.",
        count,
        baseHtml: first.html,
        baseTitle: first.title,
        agent: s.agentChoice,
      },
    });
    return true;
  }
  void s.generate({
    spaceId: first.spaceId,
    parentId: first.id,
    kind: "variation",
    origin,
    label: `Mix: ${ready.map((d) => d.title).join(" + ")}`.slice(0, 60),
    request: {
      instruction: text || "Combine them into one design.",
      count,
      bases: ready.map((d) => ({ id: d.id, title: d.title, html: d.html })),
      baseTitle: ready.map((d) => d.title).join(" + "),
      agent: s.agentChoice,
    },
  });
  return true;
}
