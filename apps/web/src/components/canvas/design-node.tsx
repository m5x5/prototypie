"use client";
import { memo } from "react";
import { Handle, Position, useStoreApi, type Node, type NodeProps } from "@xyflow/react";
import { AlertTriangle, Loader2, Maximize2, NotebookPen, RotateCcw, Sparkles, Star, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DesignPreview } from "@/components/design-preview";
import { CompareButton } from "@/features/compare/compare-dialog";
import { NODE_W } from "@/store/core";
import { useApp } from "@/store";
import { useUI } from "@/store/ui";
import { cn } from "@/lib/utils";
import { NestedDesignActions } from "@/features/nested";
import { PollDesignActions } from "@/features/poll";
import { SpokesDesignActions } from "@/features/spokes";
import { NotesBadge } from "@/features/agentation/notes";
import { PodProvenance } from "@/features/pod-mention/mention";

export type DesignNodeType = Node<{ designId: string }, "design">;

function DesignNodeImpl({ data, selected }: NodeProps<DesignNodeType>) {
  const design = useApp((s) => s.designs[data.designId]);
  const updateDesign = useApp((s) => s.updateDesign);
  const removeDesign = useApp((s) => s.removeDesign);
  const retryDesign = useApp((s) => s.retryDesign);
  const openFullscreenOne = useUI((s) => s.openFullscreen);
  const rfStore = useStoreApi();
  // Opening a card that is part of a multi-selection steps through the whole selection, starting at this card.
  const openFullscreen = (id: string) => {
    const sel = rfStore.getState().nodes.filter((n) => n.selected && n.type === "design").map((n) => n.id);
    openFullscreenOne(id, sel.length > 1 && sel.includes(id) ? sel : undefined);
  };
  const focusPrompt = useUI((s) => s.focusPrompt);
  if (!design) return null;
  const vw = design.viewport ?? 1280;
  const vh = design.viewportHeight;
  // A fixed-size screen keeps its own ratio: portrait at the phone height, landscape at full width.
  const previewSize = vh
    ? vh > vw
      ? "mx-auto h-[420px] w-auto"
      : "w-full"
    : vw < 600
      ? "mx-auto aspect-[9/17] h-[420px] w-auto"
      : "aspect-[4/3] w-full";

  return (
    // The title sits outside, above the card, as a label on the canvas; the card itself is the elevated surface.
    <div style={{ width: NODE_W }} className="text-card-foreground">
      <div className="flex h-7 items-center gap-1.5 px-0.5 pb-1 max-md:h-11">
        <span
          className={cn(
            "truncate text-xs text-muted-foreground transition-colors max-md:text-sm",
            selected && "font-medium text-foreground",
          )}
          title={design.title}
        >
          {design.title}
        </span>
        {design.kind !== "variation" && (
          <Badge variant="secondary" className="h-4 shrink-0 px-1.5 text-[10px] capitalize">
            {design.kind}
          </Badge>
        )}
        <NotesBadge designId={design.id} />
        {design.notes?.trim() && (
          <span className="shrink-0 text-muted-foreground" title={design.notes} aria-label="Has a note">
            <NotebookPen className="size-3.5" />
          </span>
        )}
        <div className="ml-auto flex shrink-0 items-center text-muted-foreground">
          <Button
            size="icon-xs"
            className="max-md:size-10"
            variant="ghost"
            aria-label="Open fullscreen"
            title="Open fullscreen"
            disabled={design.status !== "ready"}
            onClick={() => openFullscreen(design.id)}
          >
            <Maximize2 className="size-3.5" />
          </Button>
          <Button
            size="icon-xs"
            className="max-md:size-10"
            variant="ghost"
            aria-label={design.starred ? "Unstar" : "Star"}
            onClick={() => updateDesign(design.id, { starred: !design.starred })}
          >
            <Star className={cn("size-3.5", design.starred && "fill-amber-400 text-amber-400")} />
          </Button>
          <Button size="icon-xs"
            className="max-md:size-10" variant="ghost" aria-label="Delete" onClick={() => removeDesign(design.id)}>
            <Trash2 className="size-3.5" />
          </Button>
        </div>
      </div>

      <div
        className={cn(
          // Elevated surface: 12px radius, hairline border, one soft deep shadow (stronger in dark mode).
          "relative rounded-[12px] border bg-card shadow-[0_8px_30px_rgb(0_0_0/0.10)] transition-[box-shadow,outline-color] dark:shadow-[0_8px_30px_rgb(0_0_0/0.4)]",
          "outline-2 outline-offset-2 outline-transparent",
          selected && "shadow-[0_12px_36px_rgb(0_0_0/0.16)] outline-primary/60 dark:shadow-[0_12px_36px_rgb(0_0_0/0.5)]",
        )}
      >
        <Handle type="target" position={Position.Top} className="!bg-muted-foreground max-md:!size-4" />
        {/* Rounded itself instead of clipping the surface, so the handles on its edges stay whole. */}
        <div className="overflow-hidden rounded-t-[11px]">
          {design.status === "ready" ? (
            <button
              type="button"
              // A portrait fixed-size screen sits centred at its own ratio; everything else runs edge to edge.
              className={cn(
                "group relative block overflow-hidden",
                vh && vh > vw ? "mx-auto w-fit" : "w-full",
              )}
              data-fullscreen-source={design.id}
              onDoubleClick={() => openFullscreen(design.id)}
            >
              <DesignPreview
                html={design.html}
                viewport={design.viewport}
                screenHeight={vh}
                className={previewSize}
                growToContent
              />
            </button>
          ) : design.status === "generating" ? (
            <div className="flex aspect-[4/3] w-full flex-col items-center justify-center gap-2 bg-muted/40 text-muted-foreground">
              <Loader2 className="size-5 animate-spin" />
              <span className="text-xs">Designing…</span>
            </div>
          ) : (
            <div className="flex aspect-[4/3] w-full flex-col items-center justify-center gap-2 bg-destructive/5 p-4 text-center text-destructive">
              <AlertTriangle className="size-5" />
              <span className="text-xs">{design.error ?? "Generation failed"}</span>
              <Button size="sm" variant="outline" className="nodrag mt-1 max-md:h-10" onClick={() => void retryDesign(design.id)}>
                <RotateCcw /> Retry
              </Button>
            </div>
          )}
          {/* The rationale lives in the inspector and the fullscreen dialog; the card stays visual. */}
        </div>

        {!!design.request?.podContext?.length && <PodProvenance context={design.request.podContext} />}
        <div className="nodrag flex flex-wrap items-center gap-1 border-t px-2 py-2 max-md:[&_button]:min-h-10 max-md:[&_button]:min-w-10">
          <Button
            size="sm"
            variant="secondary"
            disabled={design.status !== "ready"}
            onClick={() => focusPrompt(design.id)}
          >
            <Sparkles className="size-3.5" /> Vary
          </Button>
          <CompareButton design={design} />
          <NestedDesignActions design={design} />
          <SpokesDesignActions design={design} />
          <PollDesignActions design={design} />
        </div>
        <Handle type="source" position={Position.Bottom} className="!bg-muted-foreground max-md:!size-5" />
      </div>
    </div>
  );
}

export const DesignNode = memo(DesignNodeImpl);
