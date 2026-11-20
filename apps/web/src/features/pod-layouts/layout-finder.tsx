"use client";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import { ArrowLeftRight, Layers, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { useApp } from "@/store";
import { NODE_W } from "@/store/core";
import { enterSpace } from "@/features/nested/nav";
import { canCreateIn } from "@/features/nested/root-guard";
import { loadLayoutModel, type LayoutModel } from "./model";
import {
  ACCESS_LABEL,
  LAYOUTS,
  SCENE_CSS,
  isTimed,
  layoutCardHtml,
  sceneHtml,
  unavailable,
  type Access,
  type LayoutId,
  type LfRelation,
  type LfShape,
} from "./scene";

/**
 * "+ → Layout finden…": pick two shapes from the pod catalog (GET /api/pod-catalog) and one documented relation
 * between them; four mini previews (matrix, timeline with bands, swimlanes, cards with links) are drawn from the
 * catalog's real entries. "Übernehmen" creates a new exploration with one card, or one card in the current space,
 * with the provenance in the rationale. Pairs without a documented relation get an honest note plus partners that
 * do have one.
 */

const TAG: Record<Access, string> = {
  live: "bg-emerald-500/12 text-emerald-700 dark:text-emerald-400",
  cache: "bg-amber-500/14 text-amber-700 dark:text-amber-400",
  ref: "bg-sky-500/12 text-sky-700 dark:text-sky-400",
  "401": "bg-red-500/12 text-red-700 dark:text-red-400",
};

function Tag({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <span className={cn("inline-flex h-5 items-center rounded-full px-1.5 text-[11px] font-medium whitespace-nowrap", className)}>
      {children}
    </span>
  );
}
const AccessTag = ({ access }: { access: Access }) => <Tag className={TAG[access]}>{ACCESS_LABEL[access]}</Tag>;

/** Arrow keys move between the enabled radios of a group (roving tabindex), selecting as they go. */
function onRadioKeys(e: KeyboardEvent<HTMLElement>, choose: (el: HTMLElement) => void) {
  if (!["ArrowRight", "ArrowLeft", "ArrowDown", "ArrowUp"].includes(e.key)) return;
  const items = [...e.currentTarget.querySelectorAll<HTMLElement>("[role=radio]")].filter(
    (b) => b.getAttribute("aria-disabled") !== "true",
  );
  if (!items.length) return;
  e.preventDefault();
  const i = items.indexOf(document.activeElement as HTMLElement);
  const fwd = e.key === "ArrowRight" || e.key === "ArrowDown";
  choose(items[(i + (fwd ? 1 : items.length - 1)) % items.length]);
}

/** Focus a radio now (keyed buttons survive the re-render) and again after React re-rendered the group. */
function focusLater(root: HTMLElement | null, selector: string) {
  root?.querySelector<HTMLElement>(selector)?.focus();
  requestAnimationFrame(() => root?.querySelector<HTMLElement>(selector)?.focus());
}

function relationsOf(model: LayoutModel, A: string, B: string): LfRelation[] {
  return model.relations.filter((r) => (r.a === A && r.b === B) || (r.a === B && r.b === A));
}

function rationaleFor(a: LfShape, b: LfShape, r: LfRelation, layout: string, generatedAt: string) {
  return [
    `Layout „${layout}“ aus dem Pod-Katalog (GET /api/pod-catalog, Stand ${generatedAt}).`,
    `${a.name}: ${a.source} (${ACCESS_LABEL[a.access]}, ${a.items.length} ${a.items.length === 1 ? "Eintrag" : "Einträge"}).`,
    `${b.name}: ${b.source} (${ACCESS_LABEL[b.access]}, ${b.items.length} ${b.items.length === 1 ? "Eintrag" : "Einträge"}).`,
    `Beziehung ${r.predicate}${r.via ? ` über ${r.via}` : ""} (${ACCESS_LABEL[r.access]})${r.evidence ? `, Beleg: ${r.evidence}` : ""}.`,
    "Nur belegte Einträge und Kanten; nichts ergänzt.",
  ].join(" ");
}

function apply(opts: { name: string; asSpace: boolean; layout: LayoutId; a: LfShape; b: LfShape; rel: LfRelation; generatedAt: string }) {
  const { name, asSpace, layout, a, b, rel, generatedAt } = opts;
  const layoutName = LAYOUTS.find((l) => l.id === layout)!.name;
  const card = {
    kind: "page" as const,
    title: name,
    rationale: rationaleFor(a, b, rel, layoutName, generatedAt),
    html: layoutCardHtml({ title: name, layout, a, b, rel }),
    status: "ready" as const,
    instruction: `Layout-Finder: ${a.name} × ${b.name} · ${rel.predicate} · ${layoutName}`,
  };
  const s = useApp.getState();
  if (!canCreateIn(s.activeSpaceId)) return;
  if (asSpace) {
    const { spaceId } = s.createExploration({ parentSpaceId: s.activeSpaceId, name });
    const id = useApp.getState().addDesign({ ...card, spaceId, position: { x: 0, y: 0 } });
    enterSpace(spaceId, id);
    toast.success(`Space „${name}“ angelegt`);
    return;
  }
  // Right of the right-most card in this space (or at the origin of an empty one).
  const here = Object.values(s.designs).filter((d) => d.spaceId === s.activeSpaceId);
  const last = here.reduce<(typeof here)[number] | undefined>((m, d) => (!m || d.position.x > m.position.x ? d : m), undefined);
  const position = last ? { x: last.position.x + NODE_W + 80, y: last.position.y } : { x: 0, y: 0 };
  const id = s.addDesign({ ...card, spaceId: s.activeSpaceId, position });
  s.select(id);
  toast.success(`Karte „${name}“ angelegt`);
}

export function LayoutFinderDialog({
  open,
  onOpenChange,
  returnFocus,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  returnFocus?: RefObject<HTMLElement | null>;
}) {
  const [model, setModel] = useState<LayoutModel | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [A, setA] = useState<string | null>(null);
  const [B, setB] = useState<string | null>(null);
  const [R, setR] = useState(0);
  const [L, setL] = useState<LayoutId>("lanes");
  const [asSpace, setAsSpace] = useState(true);
  const [name, setName] = useState<string | null>(null);
  const body = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    let live = true;
    loadLayoutModel()
      .then((m) => {
        if (!live) return;
        setError(null);
        setModel(m);
        // Start on the first documented relation, so the dialog opens with something to compare.
        const first = m.relations[0];
        setA((cur) => cur ?? first?.a ?? m.shapes[0]?.id ?? null);
        setB((cur) => cur ?? first?.b ?? m.shapes[1]?.id ?? null);
      })
      .catch((e) => live && setError(e instanceof Error ? e.message : String(e)));
    return () => {
      live = false;
    };
  }, [open]);

  const shape = (id: string | null) => model?.shapes.find((s) => s.id === id);
  const a = shape(A);
  const b = shape(B);
  const rels = useMemo(() => (model && A && B ? relationsOf(model, A, B) : []), [model, A, B]);
  const rel = rels[Math.min(R, rels.length - 1)];
  const layout: LayoutId =
    a && b && unavailable(L, a, b, rel) ? (LAYOUTS.find((l) => !unavailable(l.id, a, b, rel))?.id ?? L) : L;
  const ok = !!(a && b && rel && !unavailable(layout, a, b, rel));
  const layoutName = LAYOUTS.find((l) => l.id === layout)!.name;
  const defaultName = a && b ? `${a.name} × ${b.name}${ok ? ` · ${layoutName}` : ""}` : "";
  const finalName = (name ?? defaultName).trim() || defaultName;

  const partners = useMemo(() => {
    if (!model || !A) return [];
    const ids = model.relations.filter((r) => r.a === A || r.b === A).map((r) => (r.a === A ? r.b : r.a));
    return [...new Set(ids)].map((id) => model.shapes.find((s) => s.id === id)).filter((s): s is LfShape => !!s);
  }, [model, A]);

  const pickShape = (side: "a" | "b", id: string) => {
    if (side === "a") setA(id);
    else setB(id);
    setR(0);
    setName(null);
    focusLater(body.current, `[data-side="${side}"][data-s="${CSS.escape(id)}"]`);
  };

  const shapeChips = (side: "a" | "b") => {
    const cur = side === "a" ? A : B;
    const other = side === "a" ? B : A;
    return (
      <div
        role="radiogroup"
        aria-labelledby={`lf-${side}-cap`}
        className="flex flex-wrap gap-1.5"
        onKeyDown={(e) => onRadioKeys(e, (el) => pickShape(side, el.dataset.s!))}
      >
        {model!.shapes.map((s) => {
          const off = s.id === other || !s.items.length;
          return (
            <button
              key={s.id}
              type="button"
              role="radio"
              data-side={side}
              data-s={s.id}
              aria-checked={s.id === cur}
              aria-disabled={off || undefined}
              tabIndex={s.id === cur ? 0 : -1}
              title={`${s.type} · ${s.source}`}
              onClick={() => !off && pickShape(side, s.id)}
              className={cn(
                "inline-flex h-8 items-center gap-1.5 rounded-lg border bg-background px-2.5 text-[13px] font-medium outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                s.id === cur && "border-primary shadow-[inset_0_0_0_1px_var(--primary)]",
                off && "cursor-not-allowed opacity-55",
              )}
            >
              <i aria-hidden className="size-2 rounded-[2px]" style={{ background: s.color }} />
              {s.name}
              <AccessTag access={s.access} />
            </button>
          );
        })}
      </div>
    );
  };

  const create = () => {
    if (!ok || !a || !b || !rel || !model) return;
    try {
      apply({ name: finalName, asSpace, layout, a, b, rel, generatedAt: model.generatedAt });
      setName(null);
      onOpenChange(false);
    } catch (e) {
      toast.error("Konnte nicht angelegt werden", { description: e instanceof Error ? e.message : String(e) });
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[calc(100vh-40px)] flex-col gap-0 overflow-hidden p-0 sm:max-w-[980px]" finalFocus={returnFocus}>
        <style>{SCENE_CSS}</style>
        <DialogHeader className="flex-row items-center gap-2 px-4 pt-4 pb-3">
          <Layers className="size-4" aria-hidden />
          <DialogTitle>Layout finden</DialogTitle>
          <DialogDescription className="sr-only">Zwei Shapes aus dem Pod-Katalog, eine belegte Beziehung, vier Layout-Ideen.</DialogDescription>
        </DialogHeader>

        <div ref={body} className="grid gap-3.5 overflow-auto px-4 pb-4">
          {error && <p className="rounded-lg bg-destructive/10 p-3 text-destructive">Pod-Katalog nicht erreichbar: {error}</p>}
          {!model && !error && (
            <p className="flex items-center gap-2 text-muted-foreground">
              <Loader2 className="size-4 animate-spin" aria-hidden /> Pod-Katalog wird geladen…
            </p>
          )}
          {model && (
            <>
              <div className="grid items-start gap-2.5 md:grid-cols-[1fr_auto_1fr]">
                <div className="grid gap-1.5">
                  <span id="lf-a-cap" className="text-xs font-medium text-muted-foreground">Zeilen</span>
                  {shapeChips("a")}
                </div>
                <Button
                  variant="outline"
                  size="icon"
                  className="md:mt-5"
                  aria-label="Shapes tauschen"
                  onClick={() => {
                    setA(B);
                    setB(A);
                    setName(null);
                  }}
                >
                  <ArrowLeftRight />
                </Button>
                <div className="grid gap-1.5">
                  <span id="lf-b-cap" className="text-xs font-medium text-muted-foreground">Gruppen</span>
                  {shapeChips("b")}
                </div>
              </div>

              <div className="grid gap-1.5">
                <span id="lf-r-cap" className="text-xs font-medium text-muted-foreground">Beziehung</span>
                {rels.length && a && b ? (
                  <div
                    role="radiogroup"
                    aria-labelledby="lf-r-cap"
                    className="flex flex-wrap gap-1.5"
                    onKeyDown={(e) =>
                      onRadioKeys(e, (el) => {
                        setR(+el.dataset.r!);
                        focusLater(body.current, `[data-r="${el.dataset.r}"]`);
                      })
                    }
                  >
                    {rels.map((r, k) => (
                      <button
                        key={`${r.a}-${r.predicate}-${r.b}`}
                        type="button"
                        role="radio"
                        data-r={k}
                        aria-checked={r === rel}
                        tabIndex={r === rel ? 0 : -1}
                        title={r.evidence}
                        onClick={() => setR(k)}
                        className={cn(
                          "inline-flex items-center gap-2 rounded-full border bg-background px-2.5 py-1.5 text-[12.5px] outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                          r === rel && "border-primary shadow-[inset_0_0_0_1px_var(--primary)]",
                        )}
                      >
                        <span>
                          {a.name} {r.a === A ? "→" : "←"} {b.name}
                        </span>
                        <code className="font-mono text-[11.5px]">{r.predicate}</code>
                        {r.via && <Tag className="bg-muted text-muted-foreground">über {r.via}</Tag>}
                        <AccessTag access={r.access} />
                      </button>
                    ))}
                  </div>
                ) : (
                  <div className="flex flex-wrap items-center gap-1.5 rounded-lg bg-muted px-3 py-2.5 text-[12.5px] text-muted-foreground" role="status">
                    Keine belegte Beziehung zwischen {a?.name} und {b?.name}
                    {partners.length > 0 && " ·"}
                    {partners.map((p) => (
                      <button
                        key={p.id}
                        type="button"
                        className="rounded-full border bg-background px-2.5 py-0.5 text-xs text-foreground outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
                        onClick={() => pickShape("b", p.id)}
                      >
                        {a?.name} × {p.name}
                      </button>
                    ))}
                  </div>
                )}
              </div>

              <div className="grid gap-1.5">
                <span id="lf-l-cap" className="text-xs font-medium text-muted-foreground">Layout-Ideen</span>
                <div
                  role="radiogroup"
                  aria-labelledby="lf-l-cap"
                  className="grid grid-cols-2 gap-2.5 lg:grid-cols-4"
                  onKeyDown={(e) =>
                    onRadioKeys(e, (el) => {
                      setL(el.dataset.l as LayoutId);
                      setName(null);
                      focusLater(body.current, `[data-l="${el.dataset.l}"]`);
                    })
                  }
                >
                  {LAYOUTS.map((l) => {
                    const why = a && b ? unavailable(l.id, a, b, rel) : "—";
                    const timedName = a && b ? (isTimed(a) ? a.name : isTimed(b) ? b.name : null) : null;
                    return (
                      <button
                        key={l.id}
                        type="button"
                        role="radio"
                        data-l={l.id}
                        aria-checked={!why && l.id === layout}
                        aria-disabled={why ? true : undefined}
                        aria-label={why ? `${l.name}, ${why}` : l.name}
                        tabIndex={l.id === layout ? 0 : -1}
                        onClick={() => {
                          if (why) return;
                          setL(l.id);
                          setName(null);
                        }}
                        className={cn(
                          "grid gap-2 rounded-xl border bg-background p-2.5 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                          !why && l.id === layout && "border-primary shadow-[inset_0_0_0_1px_var(--primary)]",
                          why && "cursor-not-allowed opacity-50",
                        )}
                      >
                        <b className="text-[13px] font-semibold">{l.name}</b>
                        <div
                          aria-hidden
                          className="lfm h-[118px] rounded-lg border bg-muted"
                          dangerouslySetInnerHTML={{ __html: !why && a && b ? sceneHtml(l.id, a, b, rel, 200, 116) : "" }}
                        />
                        <div className="flex flex-wrap gap-1">
                          {why ? (
                            <Tag className={TAG["401"]}>{why}</Tag>
                          ) : (
                            <>
                              <Tag className="bg-muted text-muted-foreground">
                                {a!.items.length} × {b!.items.length}
                              </Tag>
                              {l.id === "timeline" && timedName && <Tag className={TAG.live}>Zeit: {timedName}</Tag>}
                              {rel?.access === "ref" && <Tag className={TAG.ref}>Ziel nur Verweis</Tag>}
                            </>
                          )}
                        </div>
                      </button>
                    );
                  })}
                </div>
              </div>
            </>
          )}
        </div>

        <div className="flex flex-wrap items-end gap-2.5 border-t px-4 py-3">
          <label className="grid min-w-[220px] flex-1 gap-1 text-xs font-medium text-muted-foreground">
            Name
            <Input value={name ?? defaultName} onChange={(e) => setName(e.target.value)} autoComplete="off" />
          </label>
          <div role="group" aria-label="Übernehmen als" className="inline-flex overflow-hidden rounded-lg border">
            {[
              [true, "Neuer Space"],
              [false, "Karte hier"],
            ].map(([v, label]) => (
              <button
                key={String(v)}
                type="button"
                aria-pressed={asSpace === v}
                onClick={() => setAsSpace(v as boolean)}
                className={cn(
                  "h-8 px-3 text-[13px] font-medium text-muted-foreground outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                  asSpace === v && "bg-accent text-foreground",
                )}
              >
                {label as string}
              </button>
            ))}
          </div>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Abbrechen
          </Button>
          <Button disabled={!ok} onClick={create}>
            Übernehmen
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
