"use client";
import { flushSync } from "react-dom";
import { create } from "zustand";
import { useApp } from "./index";

/** Transient (non-persisted) UI state shared across features. */
interface UIState {
  fullscreenId: string | null;
  /** The designs to step through in fullscreen (←/→), in canvas order; null = just the one. */
  fullscreenSet: string[] | null;
  /** Opens one design; with `set` (2+ ids, e.g. the canvas selection) the dialog steps through those. */
  openFullscreen: (id: string | null, set?: string[]) => void;
  /** Incremented to ask the inspector to focus its prompt box. */
  promptFocusTick: number;
  /** Which composer target the last focusPrompt asked for ("edit" = in-place AI edit); null keeps the current one. */
  promptTarget: "edit" | null;
  focusPrompt: (designId: string, target?: "edit") => void;
}

export const useUI = create<UIState>()((set, get) => ({
  fullscreenId: null,
  fullscreenSet: null,
  openFullscreen: (id, ids) => {
    const order = id && ids ? canvasOrder(ids) : [];
    const apply = () =>
      set({ fullscreenId: id, fullscreenSet: id && order.length > 1 && order.includes(id) ? order : null });
    const prev = get().fullscreenId;
    // Opening and closing morph the card's preview into the dialog and back; stepping through a set doesn't.
    if (!prev === !id) return apply();
    morphFullscreen((id ?? prev)!, !!id, apply);
  },
  promptFocusTick: 0,
  promptTarget: null,
  focusPrompt: (designId, target) => {
    useApp.getState().select(designId);
    set((s) => ({ promptFocusTick: s.promptFocusTick + 1, promptTarget: target ?? null }));
  },
}));

const MORPH_NAME = "fullscreen-design";
const MORPH_MS = 320;
const MORPH_EASE = "cubic-bezier(0.2, 0.8, 0.2, 1)";
/** How long the open waits (screen held on the old frame) for the fullscreen iframe to load before animating. */
const IFRAME_WAIT_MS = 250;

type VT = { ready: Promise<void>; finished: Promise<void> };

/** Resolves when the fullscreen preview's iframe exists and has loaded its design, or after `ms`. */
async function iframeLoaded(el: HTMLElement | null, ms: number) {
  const end = performance.now() + ms;
  // The preview mounts its iframe in an effect; timers keep running while the view transition holds rendering.
  let frame = el?.querySelector("iframe");
  while (!frame && performance.now() < end) {
    await new Promise((r) => setTimeout(r, 8));
    frame = el?.querySelector("iframe");
  }
  if (!frame) return;
  const d = frame.contentDocument;
  if (d?.readyState === "complete" && d.URL === "about:srcdoc" && d.body?.childElementCount) return;
  await new Promise<void>((done) => {
    const t = setTimeout(done, Math.max(0, end - performance.now()));
    frame.addEventListener("load", () => (clearTimeout(t), done()), { once: true });
  });
}

/**
 * Runs `apply` (open or close the fullscreen dialog) inside a view transition, with the clicked card's preview
 * (`data-fullscreen-source`) and the dialog's preview (`data-fullscreen-target`) sharing one view-transition-name.
 * The name is on only one of them per snapshot. Without the API, the card or reduced motion: just `apply`.
 *
 * Smoothness: the morph animates only the group's transform (compositor), not the UA's width/height keyframes
 * (main thread, which is busy loading the iframe); the open holds the old frame until the fullscreen iframe has
 * loaded, so the animation runs on finished snapshots; the dialog's own animations and backdrop blur are off.
 */
function morphFullscreen(designId: string, opening: boolean, apply: () => void) {
  const doc = document as Document & { startViewTransition?: (cb: () => void | Promise<void>) => VT };
  const card = document.querySelector<HTMLElement>(`[data-fullscreen-source="${CSS.escape(designId)}"]`);
  if (!doc.startViewTransition || !card || matchMedia("(prefers-reduced-motion: reduce)").matches) return apply();
  const target = () => document.querySelector<HTMLElement>("[data-fullscreen-target]");
  const root = document.documentElement;
  // Turns off the dialog's own fade/zoom and the backdrop blur while the morph runs (globals.css).
  root.dataset.fullscreenMorph = "";
  const from = opening ? card : target();
  const fromRect = from?.getBoundingClientRect();
  if (from) from.style.viewTransitionName = MORPH_NAME;
  const t = doc.startViewTransition(async () => {
    if (from) from.style.viewTransitionName = "";
    flushSync(apply);
    if (opening) {
      // The portal can take a tick to mount; timers still run while rendering is paused.
      for (let i = 0; i < 20 && !target(); i++) await new Promise((r) => setTimeout(r, 10));
      const to = target();
      if (to) to.style.viewTransitionName = MORPH_NAME;
      // This dialog instance never plays its own enter animation: once the morph flag on <html> goes away, the
      // fade/zoom-in classes would apply again and replay it right after the morph (the "jolt" at the end).
      // Inline, so it holds for the element's lifetime; Base UI unmounts it on close.
      const popup = document.querySelector<HTMLElement>("[data-fullscreen-dialog]");
      const overlays = popup?.parentElement?.querySelectorAll<HTMLElement>("[data-slot=dialog-overlay]") ?? [];
      for (const el of [popup, ...overlays]) if (el) Object.assign(el.style, { animation: "none", transition: "none" });
      await iframeLoaded(to, IFRAME_WAIT_MS);
    } else {
      // Base UI unmounts the closed dialog only after its exit animation, so hide it for the new snapshot.
      const popup = document.querySelector<HTMLElement>("[data-fullscreen-dialog]");
      popup?.parentElement?.querySelectorAll<HTMLElement>("[data-slot=dialog-overlay]").forEach((el) => (el.style.visibility = "hidden"));
      if (popup) popup.style.visibility = "hidden";
      card.style.viewTransitionName = MORPH_NAME;
    }
  });
  t.ready
    .then(() => {
      // Replace the UA's width/height morph by a transform-only one between the two on-screen boxes.
      const to = (opening ? target() : card)?.getBoundingClientRect();
      const group = `::view-transition-group(${MORPH_NAME})`;
      const cs = getComputedStyle(root, group);
      const w = parseFloat(cs.width);
      const h = parseFloat(cs.height);
      if (!fromRect || !to || !w || !h) return;
      const box = (r: DOMRect) => `translate(${r.left}px, ${r.top}px) scale(${r.width / w}, ${r.height / h})`;
      const a = root.animate(
        { transform: [box(fromRect), box(to)], transformOrigin: ["0 0", "0 0"] },
        { duration: MORPH_MS, easing: MORPH_EASE, fill: "both", pseudoElement: group },
      );
      void t.finished.finally(() => a.cancel());
    })
    .catch(() => {});
  t.finished.finally(() => {
    card.style.viewTransitionName = "";
    const to = target();
    if (to) to.style.viewTransitionName = "";
    delete root.dataset.fullscreenMorph;
  });
}

/** Ready designs among `ids`, in reading order on the canvas: rows top to bottom, then left to right. */
export function canvasOrder(ids: string[]): string[] {
  const designs = useApp.getState().designs;
  const ds = [...new Set(ids)].map((id) => designs[id]).filter((d) => d?.status === "ready");
  // Cards whose tops are within ROW px of each other count as one row.
  const ROW = 160;
  return ds
    .sort((a, b) => (Math.abs(a.position.y - b.position.y) < ROW ? a.position.x - b.position.x : a.position.y - b.position.y))
    .map((d) => d.id);
}
