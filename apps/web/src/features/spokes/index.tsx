"use client";
// Extension points for the "spokes" feature. Core imports exactly these names - keep them exported.
import type { Node, NodeTypes } from "@xyflow/react";
import { Boxes, Merge } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import type { Design } from "@/lib/types";
import { useApp, type AppState } from "@/store";
import { useDraft } from "@/store/use-draft";
import { ClayDialog, SpokesDialog } from "./dialogs";
import { LanesPanel } from "./lanes";
import { belowOrigins, focusOf, hubOf, launchLane, useSpokesUI } from "./lib";
import { ReviewDialog } from "./review";
import { ReviewDock } from "./review-dock";
import { canCreateIn } from "@/features/nested/root-guard";

export { VoiceButton } from "./voice";

/** Extra React Flow node types this feature renders. */
export const spokesNodeTypes: NodeTypes = {};

/** Extra (non-design) nodes to show on the canvas for a space. Must be a pure function of state. */
export function spokesExtraNodes(_state: AppState, _spaceId: string): Node[] {
  return [];
}

/** Called when one of this feature's extra nodes is dragged. Return true if handled. */
export function spokesOnNodeMove(_id: string, _type: string | undefined, _pos: { x: number; y: number }): boolean {
  return false;
}

/** Open the "New page" dialog (the header's "+ → Page"). */
export function openNewPage() {
  if (!canCreateIn()) return;
  useSpokesUI.getState().setClayOpen(true);
}

/** Buttons rendered in the top toolbar ("New page" lives in the header's "+" create menu). */
export function SpokesToolbar() {
  return <ReviewDock />;
}

/** Action buttons rendered on each design node's footer. */
export function SpokesDesignActions({ design }: { design: Design }) {
  const setSpokesFor = useSpokesUI((s) => s.setSpokesFor);
  const hub = useApp((s) => (design.kind === "spoke" ? hubOf(s, design.id) : undefined));
  // Every design can be broken into spokes (pages, variations, components, imports). A spoke with a hub shows
  // Merge in this slot instead; a spoke whose hub is gone can be broken up further.
  if (design.kind !== "spoke" || !hub) {
    return (
      <Button size="sm" variant="ghost" disabled={design.status !== "ready"} onClick={() => setSpokesFor(design.id)}>
        <Boxes className="size-3.5" /> Spokes
      </Button>
    );
  }
  return (
    <Button
      size="sm"
      variant="ghost"
      disabled={design.status !== "ready" || hub.status !== "ready"}
      title={`Merge this spoke back into "${hub.title}"`}
      onClick={() => mergeIntoHub(design.id)}
    >
      <Merge className="size-3.5" /> Merge
    </Button>
  );
}

/** Ask the agent to integrate a refined spoke into its hub (count 1, child of the hub, same kind). */
export function mergeIntoHub(spokeId: string, note?: string) {
  const s = useApp.getState();
  const spoke = s.designs[spokeId];
  const hub = hubOf(s, spokeId);
  if (!spoke || !hub) return;
  const focus = focusOf(s, spokeId) ?? spoke.title;
  const [origin] = belowOrigins(s, [hub], 1);
  void launchLane(
    {
      spaceId: hub.spaceId,
      parentId: hub.id,
      // The merged version is a new version of the hub, so it keeps the hub's kind (page, variation, ...).
      kind: hub.kind,
      origin,
      label: `Merge · ${spoke.title}`,
      request: {
        baseHtml: hub.html,
        baseTitle: hub.title,
        count: 1,
        focus,
        scope: "page",
        instruction: `Integrate the refined component below into this design, replacing its existing version of it (${focus}). Keep its design, copy and interactions, but adapt spacing, widths and tokens so it sits naturally in the design. Leave the rest of the design as it is.${note?.trim() ? `\n\nDesigner's note on this merge (takes priority, e.g. merge only part of it): ${note.trim()}` : ""}\n\n<refined_component title="${spoke.title.replace(/"/g, "'")}">\n${spoke.html}\n</refined_component>`,
      },
    },
    { type: "merge", name: `Merge · ${spoke.title}`, focus, hubId: hub.id },
  );
  toast(`Merging "${spoke.title}" into ${hub.title}`, { description: "A new version appears below the hub." });
}

/** Section rendered inside the right-hand inspector for the selected design (design may be null). */
export function SpokesInspector({ design }: { design: Design | null }) {
  const setSpokesFor = useSpokesUI((s) => s.setSpokesFor);
  const focus = useApp((s) => (design?.kind === "spoke" ? focusOf(s, design.id) : undefined));
  const hub = useApp((s) => (design?.kind === "spoke" ? hubOf(s, design.id) : undefined));
  const spokeCount = useApp((s) =>
    design && design.kind !== "spoke" ? Object.values(s.designs).filter((d) => d.kind === "spoke" && d.parentId === design.id).length : 0,
  );

  return (
    <div className="space-y-3">
      {/* "Start from a block of clay" is hidden on request; the clay dialog stays mounted for later reuse. */}
      {design && design.kind !== "spoke" && (
        <div className="space-y-2 rounded-lg border bg-muted/30 p-3">
          <div className="flex items-center gap-2 text-xs font-medium">
            <Boxes className="size-3.5" /> {design.kind === "page" ? "Hub page" : "Hub"}
            {spokeCount > 0 && <span className="font-normal text-muted-foreground">· {spokeCount} spoke designs</span>}
          </div>
          <p className="text-xs text-muted-foreground">
            Break it into components and explore each one in its own parallel lane.
          </p>
          <Button
            size="sm"
            variant="secondary"
            className="w-full"
            disabled={design.status !== "ready"}
            onClick={() => setSpokesFor(design.id)}
          >
            <Boxes /> Break into spokes
          </Button>
        </div>
      )}
      {design?.kind === "spoke" && (
        <div className="space-y-2 rounded-lg border bg-muted/30 p-3">
          <div className="flex items-center gap-2 text-xs font-medium">
            <Boxes className="size-3.5" /> Spoke
          </div>
          {focus && <p className="text-xs text-muted-foreground">{focus}</p>}
          {hub && (
            <MergeBack
              key={design.id}
              designId={design.id}
              disabled={design.status !== "ready" || hub.status !== "ready"}
              hubTitle={hub.title}
              onMerge={(note) => mergeIntoHub(design.id, note)}
            />
          )}
        </div>
      )}
      <LanesPanel />
    </div>
  );
}

/** Merge button with an optional note, e.g. "only merge the steps". */
function MergeBack({
  designId,
  disabled,
  hubTitle,
  onMerge,
}: {
  designId: string;
  disabled: boolean;
  hubTitle: string;
  onMerge: (note: string) => void;
}) {
  // Local `drafts` key per spoke: the note survives a reload and is dropped once merged.
  const [note, setNote] = useDraft(`merge:${designId}`);
  const merge = () => {
    onMerge(note);
    setNote("");
  };
  return (
    <div className="space-y-1.5">
      <Textarea
        rows={2}
        value={note}
        placeholder="Optional note, e.g. only merge the steps"
        className="min-h-0 text-xs"
        onChange={(e) => setNote(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !disabled) merge();
        }}
      />
      <Button size="sm" variant="secondary" className={cn("w-full")} disabled={disabled} onClick={merge}>
        <Merge /> Merge back into “{hubTitle}”
      </Button>
    </div>
  );
}

/** Full-screen overlays / dialogs / side panels owned by this feature. */
export function SpokesOverlay() {
  return (
    <>
      <ClayDialog />
      <SpokesDialog />
      <ReviewDialog />
    </>
  );
}
