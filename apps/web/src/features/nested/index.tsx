"use client";
// Extension points for the "nested" feature. Core imports exactly these names - keep them exported.
import type { Node, NodeTypes } from "@xyflow/react";
import type { Design } from "@/lib/types";
import { useApp, type AppState } from "@/store";
import { PORTAL_H, PORTAL_W } from "./slice";
import { PortalNode } from "./portal-node";
import { NestedToolbarImpl, SpaceBreadcrumbImpl } from "./toolbar";

export { goToMainCanvas, newExploration } from "./toolbar";
export { ProjectSpaceDialog } from "./project-space";
import { DesignActionsImpl } from "./design-actions";
import { InspectorImpl } from "./inspector";
import { OverlayImpl } from "./overlay";
import { SpaceUrlSync } from "./url-sync";

const PORTAL_PREFIX = "portal:";

/** Extra React Flow node types this feature renders. */
export const nestedNodeTypes: NodeTypes = { portal: PortalNode };

/** Portals for every space nested directly inside `spaceId`. Pure function of state. */
export function nestedExtraNodes(state: AppState, spaceId: string): Node[] {
  return Object.values(state.spaces)
    .filter((sp) => sp.parentSpaceId === spaceId)
    .sort((a, b) => a.createdAt - b.createdAt)
    .map((sp, i) => ({
      id: PORTAL_PREFIX + sp.id,
      type: "portal",
      // Fallback column left of the origin for portals without a stored position.
      position: state.portals[sp.id] ?? { x: -PORTAL_W - 160, y: i * (PORTAL_H + 60) },
      data: { spaceId: sp.id },
      deletable: false,
    }));
}

/** Called when one of this feature's extra nodes is dragged. Return true if handled. */
export function nestedOnNodeMove(id: string, type: string | undefined, pos: { x: number; y: number }): boolean {
  if (type !== "portal" || !id.startsWith(PORTAL_PREFIX)) return false;
  useApp.getState().movePortal(id.slice(PORTAL_PREFIX.length), pos);
  return true;
}

/** Breadcrumb path and back up (left side of the header). */
export function SpaceBreadcrumb() {
  return <SpaceBreadcrumbImpl />;
}

/** New exploration button (right side of the header). */
export function NestedToolbar() {
  return <NestedToolbarImpl />;
}

/** Explore / Promote buttons and the "promoted from" badge on each design node. */
export function NestedDesignActions(props: { design: Design }) {
  return <DesignActionsImpl {...props} />;
}

/** Exploration stats and promote controls, plus a list of nested explorations. */
export function NestedInspector(props: { design: Design | null }) {
  return <InspectorImpl {...props} />;
}

/** Delete confirmation and per-space viewport handling. */
export function NestedOverlay() {
  return (
    <>
      <OverlayImpl />
      <SpaceUrlSync />
    </>
  );
}
