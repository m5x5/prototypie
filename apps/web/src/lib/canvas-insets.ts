import type { FitViewOptions } from "@xyflow/react";

/** From md up the inspector floats over the canvas's right edge: 340px panel + 16px inset + 16px gap. */
export const INSPECTOR_OVERLAY = 372;
/**
 * From md up the glass minimap sits at the canvas's bottom-right, left of the inspector (16px margin; 176x116 map +
 * 6px padding + 1px border = 130px tall). Floating dev UI (the Agentation toolbar) stacks 12px above it.
 */
export const MINIMAP_TOP = 16 + 130;
/** The header (h-12) floats over the canvas's top edge from md up. */
const HEADER_OVERLAY = 48;

/**
 * fitView padding that frames content in the canvas area left free by the floating header and inspector.
 * `p` is the usual React Flow fraction (0.1 = 10% each side); below md the canvas isn't overlaid, so it's `p` as is.
 */
export function framePadding(p: number): FitViewOptions["padding"] {
  if (typeof window === "undefined" || !window.matchMedia("(min-width: 48rem)").matches) return p;
  const x = Math.round(p * (window.innerWidth - INSPECTOR_OVERLAY));
  const y = Math.round(p * (window.innerHeight - HEADER_OVERLAY));
  return { top: `${y + HEADER_OVERLAY}px`, bottom: `${y}px`, left: `${x}px`, right: `${x + INSPECTOR_OVERLAY}px` };
}
