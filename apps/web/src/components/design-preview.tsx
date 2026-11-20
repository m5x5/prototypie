"use client";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { frameIdFor } from "@/features/agentation/bridge";
import { composeLiveCached, hasSlots } from "@/features/live-view/compose";
import { useApp } from "@/store";

/**
 * Renders generated HTML in a sandboxed iframe (scripts + same-origin, so Agentation can pick inside),
 * scaled down to fit its container. `viewport` is the virtual width the design is laid out at.
 *
 * Thumbnails (non-interactive) are kept cheap, because a canvas can hold dozens of them:
 * - the iframe mounts only after it has been in view for a moment (panning past doesn't load it),
 *   and then stays mounted (no reload churn when it scrolls out and back; the canvas keeps nodes
 *   mounted for this, see canvas.tsx);
 * - CSS animations/transitions are frozen, and ~1s after load its JS timers and rAF loops are
 *   stopped, so a thumbnail becomes a static picture instead of a running page;
 * - tool chrome inside designs (e.g. DialKit panels) is hidden, and phone-sized screens drop their
 *   page backdrop so the screen sits directly on the card.
 */

const MOUNT_DELAY = 180; // ms in view before a thumbnail loads
const QUIESCE_AFTER = 1200; // ms after load before its scripts are stopped

// Animations are frozen at their END, not frame 0: a negative delay seeks a paused animation past its finish, so
// entrance animations (e.g. `@keyframes drop{from{opacity:0}}`) show their final state instead of staying invisible.
// Infinite loops just freeze at some mid-phase.
function previewCss(phone: boolean) {
  return `<style data-prototypie-preview>
*,*::before,*::after{animation-delay:-60s!important;animation-play-state:paused!important;transition:none!important;scroll-behavior:auto!important}
.dialkit-root,.dialkit-panel{display:none!important}
${phone ? "html,body{background:transparent!important}" : ""}
</style>`;
}

function forPreview(html: string, phone: boolean) {
  const css = previewCss(phone);
  const i = html.search(/<\/head>/i);
  return i >= 0 ? html.slice(0, i) + css + html.slice(i) : css + html;
}

/** Stop a thumbnail's ongoing JS work: timers, intervals and animation-frame loops. */
function quiesce(frame: HTMLIFrameElement) {
  try {
    const w = frame.contentWindow as (Window & typeof globalThis) | null;
    if (!w) return;
    const top = w.setTimeout(() => {}, 0);
    for (let id = 0; id <= top; id++) {
      w.clearTimeout(id);
      w.clearInterval(id);
    }
    w.requestAnimationFrame = () => 0;
    w.setInterval = (() => 0) as unknown as typeof w.setInterval;
    w.document.getAnimations?.().forEach((a) => a.pause());
    w.document.querySelectorAll("video,audio").forEach((m) => (m as HTMLMediaElement).pause());
  } catch {
    // Cross-origin or already gone: nothing to do.
  }
}

/** A crop also remembers the viewport height it was measured at (vh), so the frame can keep that height. */
type Crop = { x: number; y: number; w: number; h: number; vh: number };

/**
 * Where a thumbnail's actual content is, when it only fills a narrow part of the viewport (e.g. a 340px
 * sidebar or a component centred on a backdrop at 1280px). Full-width wrappers/backdrops are ignored.
 * Returns null when the content already spans most of the width. When it crops, the backdrop's pattern
 * (e.g. a dotted canvas grid) is dropped so a simple card reads as just the card.
 */
function contentCrop(frame: HTMLIFrameElement, viewport: number, maxShare = 0.6, pad = 24): Crop | null {
  try {
    const d = frame.contentDocument;
    if (!d?.body) return null;
    let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
    for (const el of d.body.querySelectorAll("*")) {
      const r = el.getBoundingClientRect();
      if (r.width <= 0 || r.height <= 0) continue;
      // Wide media is the design itself, not an empty layout wrapper. Ignoring an SVG/canvas while
      // measuring its smaller children would crop a full scene down to a few details.
      if (r.width >= viewport * 0.9 && !el.matches("img,svg,canvas,video")) continue;
      if (r.right <= 0 || r.left >= viewport) continue;
      x0 = Math.min(x0, Math.max(0, r.left));
      x1 = Math.max(x1, Math.min(viewport, r.right));
      y0 = Math.min(y0, Math.max(0, r.top));
      y1 = Math.max(y1, r.bottom);
    }
    if (!(x1 > x0) || x1 - x0 > viewport * maxShare) return null;
    // Content inside a scroll container (e.g. a week grid with overflow:auto) reports its full scroll
    // height, but only the document's own height is ever visible: clamp, or the card grows needlessly tall.
    y1 = Math.min(y1, d.documentElement.scrollHeight);
    const x = Math.max(0, x0 - pad);
    const y = Math.max(0, y0 - pad);
    for (const el of [d.documentElement, d.body]) el.style.setProperty("background-image", "none", "important");
    const vh = frame.contentWindow?.innerHeight || d.documentElement.clientHeight;
    return { x, y, w: Math.min(viewport - x, x1 - x0 + pad * 2), h: y1 + pad - y, vh };
  } catch {
    return null;
  }
}

export function DesignPreview({
  html,
  viewport = 1280,
  className,
  interactive = false,
  designId,
  style,
  fitContent,
  onFit,
  growToContent = false,
  screenHeight,
  fullscreenTarget = false,
}: {
  html: string;
  viewport?: number;
  className?: string;
  /** When false, pointer events are blocked so the canvas can be panned over the preview. */
  interactive?: boolean;
  /** Tags the iframe so Agentation notes made inside it map back to this design. */
  designId?: string;
  style?: React.CSSProperties;
  /** Marks the fullscreen dialog's preview as the end point of the card-to-fullscreen view transition. */
  fullscreenTarget?: boolean;
  /** Zoom to the content when it only fills a narrow part of the page (default: thumbnails only). */
  fitContent?: boolean;
  /** Called with the content's width (px, at most 1:1) once a fitted preview has measured it. */
  onFit?: (width: number | null) => void;
  /** Thumbnail of a narrow, tall element (e.g. a sidebar): make the box tall enough to show all of it. */
  growToContent?: boolean;
  /** Fixed screen height (e.g. 960 for a 640×960 phone): the box keeps viewport:screenHeight, no content cropping or growing. */
  screenHeight?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [{ width, height }, setSize] = useState({ width: 0, height: 0 });
  const [mounted, setMounted] = useState(interactive);
  const [loaded, setLoaded] = useState(false);
  // Thumbnails zoom in on narrow content so it's actually visible on the card.
  const [crop, setCrop] = useState<Crop | null>(null);

  // The fullscreen preview measures itself right away: its open runs inside a view transition, which holds rendering
  // (and with it ResizeObserver) until the iframe has loaded, and the iframe only mounts once the size is known.
  useLayoutEffect(() => {
    const el = ref.current;
    if (interactive && el) setSize({ width: el.clientWidth, height: el.clientHeight });
  }, [interactive]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setSize({ width: e.contentRect.width, height: e.contentRect.height }));
    ro.observe(el);
    if (interactive || mounted) return () => ro.disconnect();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const mount = () => {
      clearInterval(poll);
      setMounted(true);
      io.disconnect(); // mounted for good
    };
    const io = new IntersectionObserver(
      ([e]) => {
        clearTimeout(timer);
        if (e.isIntersecting) timer = setTimeout(mount, MOUNT_DELAY);
      },
      { rootMargin: "100px" },
    );
    io.observe(el);
    // Backstop for space switches / navigating back: the canvas re-adopts nodes and restores its viewport
    // a few frames after mount, and the observer doesn't always report that move (cards stayed empty until
    // the user zoomed). Check the real position a few times while things settle.
    let checks = 0;
    // Declared after mount(); safe because mount only runs from async callbacks (IO / timers / this interval).
    const poll = setInterval(() => {
      if (++checks > 12) return clearInterval(poll);
      const r = el.getBoundingClientRect();
      const m = 100;
      if (r.width > 0 && r.right > -m && r.bottom > -m && r.left < innerWidth + m && r.top < innerHeight + m) mount();
    }, 250);
    return () => {
      clearTimeout(timer);
      clearInterval(poll);
      ro.disconnect();
      io.disconnect();
    };
  }, [interactive, mounted]);

  const phone = viewport < 600;
  // Live views place other cards by reference: resolve their slots to each component's current main variant.
  const live = hasSlots(html);
  const resolved = useApp((s) =>
    live
      ? composeLiveCached(`${designId ?? html}:${interactive ? 1 : 0}`, html, s.designs, interactive ? undefined : (h) => forPreview(h, false))
      : html,
  );
  const doc = useMemo(() => (interactive ? resolved : forPreview(resolved, phone)), [resolved, interactive, phone]);
  const fitOn = !screenHeight && (fitContent ?? !interactive);
  // An explicit fitContent also trims phone-width pages and anything not filling ~the whole width.
  const fit = fitOn && (!phone || fitContent) && crop ? crop : null;
  // Layout scale of the iframe; it keeps the same layout height when cropped, so measured positions stay valid.
  // A fixed-size screen always lays out at exactly viewport × screenHeight and scales to fit the box, so a border
  // on the box (fullscreen) can't stretch it to e.g. 640×961.
  const base = width ? (screenHeight && height ? Math.min(width / viewport, height / screenHeight) : width / viewport) : 0;
  let scale = base;
  let tx = 0;
  let ty = 0;
  if (fit && width) {
    const byWidth = width / fit.w;
    const whole = height ? Math.min(byWidth, height / fit.h) : byWidth;
    // Show the whole element when that keeps it reasonably large (a card); otherwise fill the width from the top (a tall sidebar).
    // Never blow content up past its real size.
    // Interactive fitted previews fill their width (top-aligned) so they read at full size.
    scale = Math.min(!interactive && whole >= byWidth * 0.5 ? whole : byWidth, Math.max(base, 1));
    tx = (width / scale - fit.w) / 2 - fit.x;
    ty = scale === whole && height ? (height / scale - fit.h) / 2 - fit.y : -fit.y;
  }
  // Decided from the content's shape only (not the box's current height), so it can't oscillate.
  const tall = growToContent && !interactive && fit && fit.h / fit.w > 1.5 ? fit : null;
  return (
    <div
      ref={ref}
      data-fullscreen-target={fullscreenTarget || undefined}
      // Paint containment: panning the canvas never has to look inside the scaled iframe's subtree.
      style={{
        contain: "layout paint",
        ...(tall ? { aspectRatio: `${tall.w} / ${Math.min(tall.h, tall.w * 3)}` } : null),
        ...(screenHeight ? { aspectRatio: `${viewport} / ${screenHeight}` } : null),
        ...style,
      }}
      // Load state, first in the class list so an Agentation note on a blank card records it
      // ("thumb-waiting" = never mounted, "thumb-loading" = iframe not loaded, "thumb-ready" = loaded but blank).
      className={cn(
        interactive ? null : `thumb-${!mounted || !base ? "waiting" : loaded ? "ready" : "loading"}`,
        "relative overflow-hidden",
        interactive || !phone ? "bg-muted" : "bg-transparent",
        className,
      )}
    >
      {base > 0 && mounted && (
        <iframe
          title="Design preview"
          id={designId ? frameIdFor(designId) : undefined}
          // allow-same-origin lets the Agentation toolbar pick elements inside the design (and lets us
          // quiesce thumbnails). Designs are generated locally for this tool; it is a dev prototyping app.
          sandbox="allow-scripts allow-same-origin allow-forms"
          srcDoc={doc}
          onLoad={interactive && !fitOn ? undefined : (e) => {
            const frame = e.currentTarget;
            setLoaded(true);
            setTimeout(() => {
              if (fitOn && (!phone || fitContent)) {
                const c = contentCrop(frame, viewport, fitContent ? 0.95 : 0.6, fitContent ? 0 : 24);
                setCrop(c);
                onFit?.(c ? c.w : null);
              }
              if (!interactive) quiesce(frame);
            }, QUIESCE_AFTER);
          }}
          className={cn("absolute left-0 top-0 origin-top-left border-0", !interactive && "pointer-events-none")}
          style={{
            width: viewport,
            // Once a thumbnail is cropped, freeze its viewport at the height it was measured at. Otherwise a card
            // that grows to fit tall content also stretches 100vh layouts, pushing bottom bars out of the crop.
            height: screenHeight ? screenHeight : fit && !interactive && fit.vh ? fit.vh : `${100 / base}%`,
            transform: fit ? `scale(${scale}) translate(${tx}px, ${ty}px)` : `scale(${scale})`,
            // Phone thumbnails are see-through. Chrome paints an opaque (white) backdrop behind an iframe whose
            // document's color-scheme differs from the embedding element's (dark app vs. "normal" design), so match it.
            colorScheme: !interactive && phone ? "normal" : undefined,
          }}
        />
      )}
    </div>
  );
}
