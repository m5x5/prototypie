"use client";
import { useEffect, useRef, useState } from "react";
import { useReactFlow, type Viewport } from "@xyflow/react";
import { Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useApp } from "@/store";
import { descendantSpaceIds, isProject } from "./slice";
import { selectSubtreeDesigns } from "./portal-node";
import { useNestedUI } from "./nav";
import { framePadding } from "@/lib/canvas-insets";

export function OverlayImpl() {
  useSpaceViewports();
  return <DeletePortalDialog />;
}

/** Remember the viewport per space, restore it on return, and frame a design when navigation asks for it. */
function useSpaceViewports() {
  const rf = useReactFlow();
  const active = useApp((s) => s.activeSpaceId);
  const tick = useNestedUI((s) => s.nav?.tick ?? 0);
  const shown = useRef(active);
  const handledTick = useRef(0);
  const saved = useRef(new Map<string, Viewport>());

  useEffect(() => {
    const from = shown.current;
    const switched = from !== active;
    const nav = useNestedUI.getState().nav;
    const freshNav = !!nav && nav.tick !== handledTick.current && nav.spaceId === active;
    if (!switched && !freshNav) return;
    if (switched) {
      saved.current.set(from, rf.getViewport());
      shown.current = active;
    }
    if (nav) handledTick.current = nav.tick;
    const focusId = freshNav ? nav.designId : undefined;

    // Wait for the canvas to adopt the new space's nodes and measure them.
    let r2 = 0;
    const r1 = requestAnimationFrame(() => {
      r2 = requestAnimationFrame(() => {
        if (focusId && rf.getNode(focusId)) {
          void rf.fitView({ nodes: [{ id: focusId }], padding: framePadding(0.6), maxZoom: 1, duration: 350 });
          return;
        }
        if (!switched) return;
        const vp = saved.current.get(active);
        if (vp) void rf.setViewport(vp);
        else void rf.fitView({ padding: framePadding(0.2), maxZoom: 1 });
      });
    });
    return () => {
      cancelAnimationFrame(r1);
      cancelAnimationFrame(r2);
    };
  }, [active, tick, rf]);
}

function DeletePortalDialog() {
  const id = useNestedUI((s) => s.confirmDeleteId);
  const askDelete = useNestedUI((s) => s.askDelete);
  const space = useApp((s) => (id ? s.spaces[id] : undefined));
  const designCount = useApp((s) => (id ? selectSubtreeDesigns(s, id).length : 0));
  const nestedCount = useApp((s) => (id ? descendantSpaceIds(s, id).length : 0));
  const deleteSpace = useApp((s) => s.deleteSpace);

  // Keep showing the last target while the dialog animates out after the space is gone.
  const live = space ? { name: space.name, designCount, nestedCount, what: isProject(space) ? "project" : "exploration" } : null;
  const [shown, setShown] = useState(live);
  if (live && (live.name !== shown?.name || live.designCount !== shown.designCount || live.nestedCount !== shown.nestedCount || live.what !== shown.what))
    setShown(live);
  const info = live ?? shown;
  const parts = info ? [`${info.designCount} ${info.designCount === 1 ? "design" : "designs"}`] : [];
  if (info?.nestedCount) parts.push(`${info.nestedCount} nested ${info.nestedCount === 1 ? "space" : "spaces"}`);

  return (
    <Dialog open={!!space} onOpenChange={(o) => !o && askDelete(null)}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Delete “{info?.name}”?</DialogTitle>
          <DialogDescription>
            This removes the {info?.what ?? "exploration"} with its {parts.join(" and ")}.
            {info?.what === "project" ? "" : " Designs already promoted to this canvas stay."} This can’t be undone.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button
            variant="destructive"
            onClick={() => {
              if (id) deleteSpace(id);
              askDelete(null);
            }}
          >
            <Trash2 /> Delete {info?.what ?? "exploration"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
