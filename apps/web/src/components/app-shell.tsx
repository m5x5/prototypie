"use client";
import { useEffect, useRef, useState } from "react";
import { ReactFlowProvider } from "@xyflow/react";
import { Layers, Maximize, Minimize } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Canvas } from "@/components/canvas/canvas";
import { Inspector } from "@/components/inspector";
import { FullscreenDialog } from "@/components/fullscreen-dialog";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Toaster } from "@/components/ui/sonner";
import { goToMainCanvas, NestedOverlay, SpaceBreadcrumb } from "@/features/nested";
import { CreateMenu } from "@/components/create-menu";
import { PollMenuItems, PollOverlay, PollToolbar } from "@/features/poll";
import { SpokesOverlay, SpokesToolbar } from "@/features/spokes";
import { AgentationBridge } from "@/features/agentation/bridge";
import { ImportMenu } from "@/features/imports/import-menu";
import { CompareDialog } from "@/features/compare/compare-dialog";
import { AgentCapacityButton } from "@/features/agent-tasks/header-capacity";

/** Toggles browser fullscreen for the whole app. */
/** Keyboard Lock API (Chromium): while fullscreen, a locked Escape doesn't exit (holding Esc still does). */
type KeyboardLock = { lock?: (keys?: string[]) => Promise<void>; unlock?: () => void };
const keyboardLock = () => (navigator as Navigator & { keyboard?: KeyboardLock }).keyboard;

/** Toggles browser fullscreen for the whole app. Escape stays available to the app instead of exiting. */
function FullscreenToggle() {
  const [full, setFull] = useState(false);
  useEffect(() => {
    const sync = () => {
      const on = !!document.fullscreenElement;
      setFull(on);
      if (on) void keyboardLock()?.lock?.(["Escape"]).catch(() => {});
      else keyboardLock()?.unlock?.();
    };
    document.addEventListener("fullscreenchange", sync);
    return () => document.removeEventListener("fullscreenchange", sync);
  }, []);
  const label = full ? "Exit fullscreen" : "Fullscreen";
  return (
    <Button
      size="icon-sm"
      variant="ghost"
      className="max-md:hidden"
      aria-label={label}
      title={label}
      onClick={() => void (full ? document.exitFullscreen() : document.documentElement.requestFullscreen()).catch(() => {})}
    >
      {full ? <Minimize /> : <Maximize />}
    </Button>
  );
}

/**
 * From md up: a soft backdrop behind a header group (::before, behind the group's content, click-through). The group
 * stretches to the header's full 48px height (content stays centred), so the backdrop spans y 0–48 whatever the
 * controls' height, runs out to the window edge and 4rem towards the centre, and fades out at the bottom
 * (solid to 40%, like the old full-width fade) and towards the centre, so there's no hard box.
 */
const FADE_BASE =
  "md:relative md:isolate md:self-stretch md:before:pointer-events-none md:before:absolute md:before:inset-y-0 md:before:-z-10 md:before:bg-(--canvas-bg) md:before:mask-b-from-40%";
const FADE_LEFT = "md:before:-left-4 md:before:-right-16 md:before:mask-r-from-[calc(100%-4rem)]";
const FADE_RIGHT = "md:before:-right-4 md:before:-left-16 md:before:mask-l-from-[calc(100%-4rem)]";

/** Phones show one pane at a time (the canvas or the side panel); from md up both sit side by side. */
type Pane = "canvas" | "panel";

function PaneSwitch({ pane, onChange }: { pane: Pane; onChange: (p: Pane) => void }) {
  const tab = (id: Pane, label: string) => (
    <button
      type="button"
      role="tab"
      aria-selected={pane === id}
      onClick={() => onChange(id)}
      className={cn(
        "h-10 min-w-24 rounded-full px-4 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring motion-reduce:transition-none",
        pane === id ? "bg-foreground text-background" : "text-muted-foreground hover:text-foreground",
      )}
    >
      {label}
    </button>
  );
  return (
    // Centred, so the Agentation button (bottom right) and the canvas zoom controls (bottom left) stay free.
    <div className="pointer-events-none fixed inset-x-0 bottom-[max(0.75rem,env(safe-area-inset-bottom))] z-40 flex justify-center md:hidden">
      <div role="tablist" aria-label="View" className="pointer-events-auto flex gap-1 rounded-full border bg-background/90 p-1 shadow-lg backdrop-blur">
        {tab("canvas", "Canvas")}
        {tab("panel", "Panel")}
      </div>
    </div>
  );
}

export function AppShell() {
  const [pane, setPane] = useState<Pane>("canvas");
  const moreRef = useRef<HTMLButtonElement>(null);
  return (
    // Wait before the first tooltip (instant ones flood the cards); neighbours still switch instantly.
    <TooltipProvider delay={600}>
      <ReactFlowProvider>
        {/* --canvas-bg: the one canvas colour (page bg in light, pitch black in dark); the canvas and the header fade use it. */}
        <div className="flex h-dvh flex-col [--canvas-bg:var(--background)] dark:[--canvas-bg:#000]">
          {/* No border or solid fill. From md up the canvas runs under the header (main's -mt-12), the middle is fully
              transparent and only the left and right groups get a soft --canvas-bg backdrop (FADE_*), fading down and
              towards the centre; only the controls catch clicks. Phones keep one full-width fade (header stays in flow). */}
          <header className="relative z-30 flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 max-md:bg-linear-to-b max-md:from-(--canvas-bg) max-md:from-40% max-md:to-transparent px-3 py-1.5 md:pointer-events-none md:h-12 md:flex-nowrap md:px-4 md:py-0 md:*:pointer-events-auto">
            {/* Logo + path share a tighter gap than the header's controls, so the › sits centred between them. */}
            <div className={cn("flex min-w-0 flex-1 items-center gap-1 md:flex-none", FADE_BASE, FADE_LEFT)}>
              <button
                type="button"
                aria-label="Main canvas"
                title="Main canvas"
                onClick={goToMainCanvas}
                className="flex h-10 cursor-pointer items-center gap-[9px] rounded-full pr-1 text-sm font-semibold transition-opacity hover:opacity-80 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none motion-reduce:transition-none"
              >
                <span className="grid size-[30px] shrink-0 place-items-center rounded-full bg-linear-145 from-white to-[#ededed] shadow-[inset_0_0_0_1px_var(--border)] dark:from-[#2a2a2a] dark:to-[#151515]">
                  <Layers className="size-[15px]" />
                </span>
                <span className="max-md:hidden">Prototypie</span>
              </button>
              {/* May shrink (names truncate) so a long space path never runs under the header's right-hand controls. */}
              <div className="flex min-w-0 flex-1 md:flex-none">
                <SpaceBreadcrumb />
              </div>
            </div>
            {/* On a phone the controls form a second, horizontally scrollable row of their own. */}
            <div className={cn("flex shrink-0 items-center gap-2 max-md:order-last max-md:w-full max-md:overflow-x-auto max-md:pb-1 max-md:[scrollbar-width:none] md:ml-auto", FADE_BASE, FADE_RIGHT)}>
              <AgentCapacityButton />
              <CreateMenu />
              <SpokesToolbar />
              <FullscreenToggle />
              <ImportMenu triggerRef={moreRef} items={<PollMenuItems />} />
              <PollToolbar anchor={moreRef} />
            </div>
          </header>
          {/* From md up the inspector floats over the canvas (absolute, see Inspector), so main spans the full width. */}
          <div className="relative flex min-h-0 flex-1">
            <main className={cn("relative min-w-0 flex-1 md:-mt-12", pane !== "canvas" && "max-md:hidden")}>
              <Canvas />
            </main>
            <div className={cn("min-h-0 min-w-0 flex-1 md:contents", pane !== "panel" && "max-md:hidden")}>
              <Inspector />
            </div>
          </div>
        </div>
        <PaneSwitch pane={pane} onChange={setPane} />
        <FullscreenDialog />
        <CompareDialog />
        <NestedOverlay />
        <SpokesOverlay />
        <PollOverlay />
        {/* From md up toasts sit left of the floating inspector (340px panel + 16px insets + 16px gap). */}
        <Toaster className="md:[--toast-offset-right:388px]" offset={{ right: "var(--toast-offset-right, 24px)" }} />
        <AgentationBridge />
      </ReactFlowProvider>
    </TooltipProvider>
  );
}
