"use client";
import { Columns3, Maximize2, Star } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { create } from "zustand";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { DesignPreview } from "@/components/design-preview";
import { cn } from "@/lib/utils";
import { useApp } from "@/store";
import { useUI } from "@/store/ui";
import type { Design } from "@/lib/types";

/** Which design's results are being compared (transient). */
const useCompare = create<{ id: string | null; open: (id: string | null) => void }>()((set) => ({
  id: null,
  open: (id) => set({ id }),
}));

/** Ready variations made from a design, oldest first. */
function useResults(id: string | null) {
  return useApp(
    useShallow((s) =>
      id
        ? Object.values(s.designs)
            .filter((d) => d.parentId === id && d.status === "ready")
            .sort((a, b) => a.createdAt - b.createdAt)
        : [],
    ),
  );
}

/** "Compare" action on a design node: shown once the design has results. */
export function CompareButton({ design }: { design: Design }) {
  // A plain count keeps this cheap: every node on the canvas renders one of these.
  const count = useApp((s) => {
    let n = 0;
    for (const id in s.designs) if (s.designs[id].parentId === design.id && s.designs[id].status === "ready") n++;
    return n;
  });
  const open = useCompare((s) => s.open);
  if (!count) return null;
  return (
    <Button size="sm" variant="ghost" title="View this design and its variations side by side" onClick={() => open(design.id)}>
      <Columns3 className="size-3.5" /> Compare ({count})
    </Button>
  );
}

/** Side-by-side view of a design and all its variations, each scrollable and interactive. */
export function CompareDialog() {
  const id = useCompare((s) => s.id);
  const close = useCompare((s) => s.open);
  const base = useApp((s) => (id ? s.designs[id] : undefined));
  const results = useResults(id);
  const updateDesign = useApp((s) => s.updateDesign);
  const openFullscreen = useUI((s) => s.openFullscreen);
  const all = base ? [base, ...results] : [];

  return (
    <Dialog open={!!base} onOpenChange={(o) => !o && close(null)}>
      <DialogContent className="inset-0 top-0 left-0 flex h-dvh w-screen max-w-none translate-x-0 translate-y-0 flex-col gap-3 rounded-none bg-background ring-0 sm:max-w-none max-md:pb-[max(1rem,env(safe-area-inset-bottom))] max-md:[&>[data-slot=dialog-close]]:size-10">
        <DialogHeader>
          <DialogTitle>{base?.title}: side by side</DialogTitle>
        </DialogHeader>
        <div className="flex min-h-0 flex-1 gap-3 overflow-x-auto pb-2">
          {all.map((d, i) => (
            <CompareColumn
              key={d.id}
              design={d}
              original={i === 0}
              wide={all.length <= 2}
              onStar={() => updateDesign(d.id, { starred: !d.starred })}
              onOpen={() => {
                close(null);
                openFullscreen(d.id);
              }}
            />
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}

/** One design in the compare row, at a fixed column width: no post-load fitting, so it never resizes after it appears. */
function CompareColumn({
  design: d,
  original,
  wide,
  onStar,
  onOpen,
}: {
  design: Design;
  original: boolean;
  wide: boolean;
  onStar: () => void;
  onOpen: () => void;
}) {
  const phone = (d.viewport ?? 1280) < 600;
  return (
    <div
      className={cn(
        "flex min-h-0 shrink-0 flex-col gap-2",
        phone ? "w-[400px] max-md:w-[85vw]" : wide ? "w-[calc(50%-6px)] min-w-[480px] max-md:w-[85vw] max-md:min-w-0" : "w-[560px] max-md:w-[85vw]",
      )}
    >
      <div className="flex items-center gap-1">
        <span className="truncate text-sm font-medium" title={d.title}>
          {original ? "Original · " : ""}
          {d.title}
        </span>
        <div className="ml-auto flex shrink-0 items-center">
          <Button size="icon-sm" variant="ghost" className="max-md:size-10" aria-label={d.starred ? "Unstar" : "Star"} onClick={onStar}>
            <Star className={cn("size-4", d.starred && "fill-amber-400 text-amber-400")} />
          </Button>
          <Button size="icon-sm" variant="ghost" className="max-md:size-10" aria-label="Open fullscreen" onClick={onOpen}>
            <Maximize2 className="size-4" />
          </Button>
        </div>
      </div>
      {/* No frame around the designs. */}
      <DesignPreview
        html={d.html}
        viewport={d.viewport ?? 1280}
        interactive
        className="min-h-0 flex-1 bg-transparent"
      />
    </div>
  );
}
