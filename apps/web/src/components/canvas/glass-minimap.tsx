"use client";
import { useCallback, useEffect, useRef, useState } from "react";
import { MiniMap, Panel, useOnViewportChange, useStore } from "@xyflow/react";
import { cn } from "@/lib/utils";
import { GLASS } from "@/lib/glass";

const HIDE_AFTER = 1200; // ms after the last pan/zoom (or leaving the corner) before it fades out
const INTRO = 3500; // shown briefly on load so people learn it exists
const CORNER = { w: 230, h: 170 }; // px from the bottom-right that counts as "hovering the corner"

/**
 * Canvas minimap as a slim translucent glass panel that only appears while the canvas moves (pan/zoom)
 * or while the pointer is in the bottom-right corner, then fades out, so the canvas stays clean.
 * Shows the zoom level in its corner. Still pannable/zoomable like the stock minimap.
 */
export function GlassMiniMap() {
  const [on, setOn] = useState(true);
  const hovering = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const zoom = useStore((s) => s.transform[2]);

  const show = useCallback(() => {
    clearTimeout(timer.current);
    setOn(true);
  }, []);
  const hideSoon = useCallback((delay = HIDE_AFTER) => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      if (!hovering.current) setOn(false);
    }, delay);
  }, []);

  const enter = useCallback(() => {
    hovering.current = true;
    show();
  }, [show]);
  const leave = useCallback(() => {
    hovering.current = false;
    hideSoon();
  }, [hideSoon]);

  useEffect(() => {
    hideSoon(INTRO);
    return () => clearTimeout(timer.current);
  }, [hideSoon]);

  // Stable handlers: xyflow re-registers them in the store whenever they change, and this re-renders on every zoom.
  const end = useCallback(() => hideSoon(), [hideSoon]);
  useOnViewportChange({ onStart: show, onChange: show, onEnd: end });

  // Hot corner: the pointer near the bottom-right corner reveals the map. Checked by position rather than
  // with an overlay, so panning and zooming in that corner keep working.
  const domNode = useStore((s) => s.domNode);
  const inCorner = useRef(false);
  useEffect(() => {
    if (!domNode) return;
    const move = (e: PointerEvent) => {
      const r = domNode.getBoundingClientRect();
      const now =
        r.right - e.clientX < CORNER.w &&
        r.bottom - e.clientY < CORNER.h &&
        e.clientX >= r.left &&
        e.clientY >= r.top;
      if (now === inCorner.current) return;
      inCorner.current = now;
      if (now) enter();
      else leave();
    };
    const out = () => {
      if (inCorner.current) leave();
      inCorner.current = false;
    };
    domNode.addEventListener("pointermove", move);
    domNode.addEventListener("pointerleave", out);
    return () => {
      domNode.removeEventListener("pointermove", move);
      domNode.removeEventListener("pointerleave", out);
      inCorner.current = false;
      hovering.current = false;
    };
  }, [domNode, enter, leave]);

  return (
    <Panel
      position="bottom-right"
      className={cn(
        // md:!mr: clear of the floating inspector (INSPECTOR_OVERLAY + 16px).
        "max-md:!hidden !m-4 md:!mr-[388px] overflow-hidden rounded-[14px] p-1.5",
        GLASS,
        "transition-[opacity,transform] duration-300 motion-reduce:transition-none",
        on
          ? "translate-y-0 scale-100 opacity-100"
          : "pointer-events-none translate-y-1.5 scale-[0.98] opacity-0"
      )}
    >
      <MiniMap
        pannable
        zoomable
        ariaLabel="Canvas minimap"
        bgColor="transparent"
        nodeColor="color-mix(in oklab, var(--foreground) 22%, transparent)"
        nodeStrokeColor="transparent"
        nodeBorderRadius={40}
        maskColor="color-mix(in oklab, var(--background) 45%, transparent)"
        maskStrokeColor="color-mix(in oklab, var(--foreground) 55%, transparent)"
        maskStrokeWidth={2}
        style={{ width: 176, height: 116 }}
        // Sits inside the glass panel instead of positioning itself.
        className="!static !m-0 !bg-transparent"
      />
      <span className="pointer-events-none absolute bottom-2 left-2.5 text-[10.5px] tabular-nums text-muted-foreground">
        {Math.round(zoom * 100)}%
      </span>
    </Panel>
  );
}
