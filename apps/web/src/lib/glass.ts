/**
 * Glass surface for UI floating over the canvas (minimap, inspector panel): translucent fill, blur + saturate so
 * the canvas reads through, a subtle 1px border (foreground/10: white/10 in dark, black/10 in light), soft shadow.
 */
export const GLASS =
  "border border-foreground/10 bg-background/50 shadow-[0_10px_30px_-12px_rgb(0_0_0/0.6)] backdrop-blur-[14px] backdrop-saturate-[1.3]";

/**
 * Denser glass for text-heavy panels (inspector): canvas-coloured fill at 80% (#000 in dark, page bg in light) and a
 * stronger blur, so content passing underneath is only a soft hint and the text stays crisp. Needs --canvas-bg
 * (set on the app shell).
 */
export const GLASS_PANEL =
  "border border-foreground/10 bg-(--canvas-bg)/80 shadow-[0_10px_30px_-12px_rgb(0_0_0/0.6)] backdrop-blur-[20px] backdrop-saturate-[1.3]";
