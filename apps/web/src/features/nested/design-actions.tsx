"use client";
import { ArrowUpFromLine, BookMarked, CornerLeftDown, Telescope } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { ImplementButton } from "@/features/agent-tasks/implement-button";
import { AiEditActions } from "@/features/ai-edit";
import { MainVariantToggle } from "@/features/live-view/actions";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useApp } from "@/store";
import { useUI } from "@/store/ui";
import type { Design } from "@/lib/types";
import { canPromoteTo, findPromotedCopy } from "./slice";
import { enterSpace, promote, showPromotedCopy } from "./nav";

export function DesignActionsImpl({ design }: { design: Design }) {
  const parentSpaceId = useApp((s) => s.spaces[design.spaceId]?.parentSpaceId);
  const parentName = useApp((s) => (parentSpaceId ? s.spaces[parentSpaceId]?.name : undefined));
  const promotedCopy = useApp((s) => findPromotedCopy(s, design.id));
  const ref = useApp((s) => s.promotedFrom[design.id]);
  const sourceSpace = useApp((s) => (ref ? s.spaces[ref.spaceId] : undefined));
  const createExploration = useApp((s) => s.createExploration);
  const setGuidelines = useApp((s) => s.setGuidelines);
  const spaceName = useApp((s) => s.spaces[design.spaceId]?.name ?? "this space");
  const focusPrompt = useUI((s) => s.focusPrompt);
  const ready = design.status === "ready";

  // A promoted design (or its promoted copy) is a decision: offer to build it for real.
  const canImplement = Boolean(promotedCopy || ref) && ready;

  const explore = () => {
    const { spaceId, seedId } = createExploration({ parentSpaceId: design.spaceId, seedDesignId: design.id });
    enterSpace(spaceId, seedId);
    // Land with the prompt focused so the next round of exploration is one keystroke away.
    if (seedId) focusPrompt(seedId);
  };

  return (
    <>
      <Tooltip>
        <TooltipTrigger render={<Button size="sm" variant="ghost" disabled={!ready} onClick={explore} />}>
          <Telescope className="size-3.5" /> Explore
        </TooltipTrigger>
        <TooltipContent>Explore in a nested space</TooltipContent>
      </Tooltip>

      {canPromoteTo(parentSpaceId) &&
        (promotedCopy ? (
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  size="sm"
                  variant="ghost"
                  className="text-primary"
                  onClick={() => showPromotedCopy(design.id)}
                />
              }
            >
              <ArrowUpFromLine className="size-3.5" /> Promoted
            </TooltipTrigger>
            <TooltipContent>Show the copy in {parentName}</TooltipContent>
          </Tooltip>
        ) : (
          <Tooltip>
            <TooltipTrigger
              render={<Button size="sm" variant="ghost" disabled={!ready} onClick={() => promote([design.id])} />}
            >
              <ArrowUpFromLine className="size-3.5" /> Promote
            </TooltipTrigger>
            <TooltipContent>Copy up to {parentName}</TooltipContent>
          </Tooltip>
        ))}

      {canImplement && <ImplementButton design={design} />}

      <AiEditActions design={design} />

      <MainVariantToggle design={design} />

      {design.guidelinesAt ? (
        <Tooltip>
          <TooltipTrigger
            render={
              <Badge
                variant="outline"
                className="cursor-pointer gap-1 border-primary/40 text-primary"
                render={<button type="button" onClick={() => setGuidelines(design.id, false)} />}
              />
            }
          >
            <BookMarked /> Guidelines
          </TooltipTrigger>
          <TooltipContent>Design guidelines of {spaceName}: new designs here follow them. Click to unset.</TooltipContent>
        </Tooltip>
      ) : (
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label="Set as design guidelines"
                disabled={!ready}
                onClick={() => setGuidelines(design.id, true)}
              />
            }
          >
            <BookMarked className="size-3.5" />
          </TooltipTrigger>
          <TooltipContent>Set as the design guidelines of {spaceName}</TooltipContent>
        </Tooltip>
      )}

      {ref && (
        <Badge
          variant="outline"
          className="ml-auto max-w-full cursor-pointer gap-1 text-muted-foreground hover:text-foreground"
          title={sourceSpace ? `Jump into ${sourceSpace.name}` : "Source space was deleted"}
          render={
            <button
              type="button"
              disabled={!sourceSpace}
              onClick={() => sourceSpace && enterSpace(sourceSpace.id, ref.designId)}
            />
          }
        >
          <CornerLeftDown />
          <span className="truncate">
            from {sourceSpace?.name ?? ref.spaceName}
            {!sourceSpace && " (deleted)"}
          </span>
        </Badge>
      )}
    </>
  );
}
