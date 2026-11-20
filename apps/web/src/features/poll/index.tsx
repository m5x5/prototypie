"use client";
// Extension points for the "poll" feature. Core imports exactly these names - keep them exported.
import type { Node, NodeTypes } from "@xyflow/react";
import { Check, Vote } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { Design } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useApp, type AppState } from "@/store";
import type { RefObject } from "react";
import { DecisionsDock, DecisionsMenuItem } from "./decisions-dock";
import { DecisionsPanel } from "./panel";

/** Extra React Flow node types this feature renders. */
export const pollNodeTypes: NodeTypes = {};

/** Extra (non-design) nodes to show on the canvas for a space. Must be a pure function of state. */
export function pollExtraNodes(_state: AppState, _spaceId: string): Node[] {
  return [];
}

/** Called when one of this feature's extra nodes is dragged. Return true if handled. */
export function pollOnNodeMove(_id: string, _type: string | undefined, _pos: { x: number; y: number }): boolean {
  return false;
}

/** The "Decisions" popover of the top toolbar; it opens from the More menu and anchors to its button. */
export function PollToolbar({ anchor }: { anchor: RefObject<HTMLElement | null> }) {
  return <DecisionsDock anchor={anchor} />;
}

/** Items this feature adds to the header's More menu. */
export function PollMenuItems() {
  return <DecisionsMenuItem />;
}

/** Action buttons rendered on each design node's footer. */
export function PollDesignActions({ design }: { design: Design }) {
  const inPoll = useApp((s) => s.pollCandidates.includes(design.id));
  const toggle = useApp((s) => s.togglePollCandidate);
  return (
    <Button
      size="sm"
      variant={inPoll ? "secondary" : "ghost"}
      aria-pressed={inPoll}
      title={inPoll ? "Remove from the stakeholder poll" : "Add to the stakeholder poll"}
      disabled={design.status !== "ready" && !inPoll}
      onClick={() => toggle(design.id)}
      className={cn(inPoll && "text-emerald-700 dark:text-emerald-400")}
    >
      {inPoll ? <Check className="size-3.5" /> : <Vote className="size-3.5" />}
      {inPoll ? "In poll" : "Poll"}
    </Button>
  );
}

/** Section rendered inside the right-hand inspector for the selected design (design may be null). */
export function PollInspector(_props: { design: Design | null }) {
  return null;
}

/** Full-screen overlays / dialogs / side panels owned by this feature. */
export function PollOverlay() {
  return <DecisionsPanel />;
}
