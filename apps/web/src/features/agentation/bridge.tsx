"use client";
import { useEffect, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Agentation, type Annotation } from "agentation";
import { create } from "zustand";
import { useApp } from "@/store";
import { INSPECTOR_OVERLAY, MINIMAP_TOP } from "@/lib/canvas-insets";

/** Agent Sync server (agentation-mcp). Override with NEXT_PUBLIC_AGENTATION_ENDPOINT, or set it to "" for local-only. */
const ENDPOINT = process.env.NEXT_PUBLIC_AGENTATION_ENDPOINT ?? "http://localhost:4747";

/** Iframe id <-> design id. DesignPreview gives every design iframe this id so notes can be mapped back. */
export const frameIdFor = (designId: string) => `design-${designId}`;
const designIdFromFrame = (frameId?: string) => (frameId?.startsWith("design-") ? frameId.slice(7) : undefined);

/**
 * Moves the toolbar (its <agentation-toolbar> host; a light-DOM rule beats the library's :host bottom/right) off the
 * inspector and minimap from md up. Below md the inspector is a full-screen pane and the minimap is hidden.
 */
const TOOLBAR_CLASS = "md:right-(--agentation-right) md:bottom-(--agentation-bottom)";

/** Where Agentation portals its UI: a modal's popup while one is open (focus trap), else document.body. */
export const useAgentationHost = create<{ container: HTMLElement | null; setContainer: (el: HTMLElement | null) => void }>()(
  (set) => ({ container: null, setContainer: (container) => set({ container }) }),
);

function toNote(a: Annotation) {
  // Innermost design frame wins (a design could in theory nest another).
  const designId = [...(a.frame?.path ?? [])].reverse().map((p) => designIdFromFrame(p.id)).find(Boolean);
  if (!designId) return null;
  return {
    id: a.id,
    designId,
    comment: a.comment,
    element: a.element,
    elementPath: a.elementPath,
    selectedText: a.selectedText,
    timestamp: a.timestamp,
  };
}

/**
 * The Agentation toolbar for the whole app. Notes made on the app chrome go to the
 * coding agent (via agentation-mcp); notes made inside a design iframe are ALSO kept
 * per design, so "Apply notes" can turn them into the next round of variations.
 */
export function AgentationBridge() {
  const container = useAgentationHost((s) => s.container);
  const root = useRef<Root | null>(null);

  // Rendered in its own React root, outside Next's StrictMode. Under StrictMode, agentation 3.1.2's
  // useFeedbackPortal creates two portal hosts, so the toolbar mounts, then remounts into the second
  // host while the first instance's session init is still running: every unsynced (offline) note was
  // POSTed twice, and each load opened two sessions and two SSE streams.
  useEffect(() => {
    if (process.env.NODE_ENV !== "development") return;
    const el = document.createElement("div");
    el.dataset.agentationRoot = "";
    el.style.display = "contents";
    // From md up the toolbar sits right above the glass minimap, right-aligned with it (TOOLBAR_CLASS reads these).
    // On <html>: the toolbar host is portalled (body or a modal), not into `el`.
    const html = document.documentElement.style;
    html.setProperty("--agentation-right", `${INSPECTOR_OVERLAY + 16}px`);
    html.setProperty("--agentation-bottom", `${MINIMAP_TOP + 12}px`);
    document.body.appendChild(el);
    const r = createRoot(el);
    root.current = r;
    return () => {
      root.current = null;
      // Unmounting a root synchronously from inside a commit warns; defer it.
      setTimeout(() => {
        r.unmount();
        el.remove();
      });
    };
  }, []);

  // Declared after the root effect, so on mount (and StrictMode's re-run) it sees the live root.
  useEffect(() => {
    if (!root.current) return;
    const sync = (a: Annotation) => {
      const n = toNote(a);
      if (n) useApp.getState().upsertNote(n);
    };
    root.current.render(
      <Agentation
        appName="Prototypie"
        className={TOOLBAR_CLASS}
        endpoint={ENDPOINT || undefined}
        portalContainer={container}
        onAnnotationAdd={sync}
        onAnnotationUpdate={sync}
        onAnnotationDelete={(a) => useApp.getState().removeNote(a.id)}
        onAnnotationsClear={(list) => list.forEach((a) => useApp.getState().removeNote(a.id))}
      />,
    );
  }, [container]);

  return null;
}
