"use client";
import { Crown } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { Design } from "@/lib/types";
import { useApp } from "@/store";
import { NODE_H } from "@/store/core";
import { defaultLiveHtml, slotLineage } from "./compose";

/**
 * "Main" toggle on cards that a live view shows (a slot target or one of its variations): marks this card as the
 * variant live views show for that component. The newest mark wins, so no other card needs unmarking.
 */
export function MainVariantToggle({ design }: { design: Design }) {
  const inLiveView = useApp((s) => slotLineage(s.designs).has(design.id));
  const updateDesign = useApp((s) => s.updateDesign);
  if (!inLiveView && !design.mainAt) return null;
  const on = !!design.mainAt;
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            size="sm"
            variant="ghost"
            aria-pressed={on}
            className={cn(on && "text-primary")}
            disabled={design.status !== "ready"}
            onClick={() => updateDesign(design.id, { mainAt: on ? undefined : Date.now() })}
          />
        }
      >
        <Crown className={cn("size-3.5", on && "fill-current")} /> Main
      </TooltipTrigger>
      <TooltipContent>{on ? "Live views show this variant. Click to unset." : "Show this variant in live views"}</TooltipContent>
    </Tooltip>
  );
}

/**
 * New live view of the given cards: one ready card whose slots reference them (stacked, in order), placed centred
 * below them. Edit its layout like any card (AI edit keeps the slots); it always shows each component's main variant.
 */
export function createLiveView(ids: string[]): string | undefined {
  const s = useApp.getState();
  const ready = ids.map((id) => s.designs[id]).filter((d): d is Design => d?.status === "ready");
  if (ready.length < 1) return undefined;
  const first = ready[0];
  const viewport = Math.max(...ready.map((d) => d.viewport ?? 1280));
  const x = ready.reduce((sum, d) => sum + d.position.x, 0) / ready.length;
  const y = Math.max(...ready.map((d) => d.position.y)) + NODE_H + 220;
  return s.addDesign({
    spaceId: first.spaceId,
    parentId: first.id,
    kind: "page",
    title: "Live view",
    rationale: `Shows the current main variant of: ${ready.map((d) => d.title).join(", ")}.`,
    html: defaultLiveHtml(
      ready.map((d) => d.id),
      viewport,
    ),
    status: "ready",
    position: { x, y },
    viewport,
    // Stored like a combine so the canvas draws an edge from every component (see features/multi/mix.tsx).
    request: {
      instruction: "Live view",
      count: 1,
      bases: ready.map((d) => ({ id: d.id, title: d.title, html: "" })),
      baseTitle: ready.map((d) => d.title).join(" + "),
    },
  });
}
