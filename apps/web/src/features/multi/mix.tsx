"use client";
import { memo, useEffect, useRef } from "react";
import {
  Handle,
  Position,
  type Connection,
  type Edge,
  type FinalConnectionState,
  type Node,
  type NodeProps,
  type NodeTypes,
  type ReactFlowInstance,
} from "@xyflow/react";
import { ArrowUp, Combine, Sparkles, X } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { AGENTS } from "@/components/prompt-composer";
import { cn } from "@/lib/utils";
import { useApp, type AppState } from "@/store";
import { NODE_H, NODE_W } from "@/store/core";
import { generateFromSources } from "./generate";

/**
 * Drag-to-create: drag from a card's handle and let go on empty canvas to start a draft card there (a variation of
 * that card). Connect more cards to the draft (drag from their handle onto it, or drop one card's drag onto
 * another card) and it becomes a MIX. Submitting generates from all connected sources through the same
 * `bases` request as the selection bar's Combine (see ./generate.ts).
 */

export const MIX_PREFIX = "mix:";
const MIX_W = 340;
const MAX_COUNT = 4;

type MixNodeType = Node<{ draftId: string }, "mix">;

export const mixNodeTypes: NodeTypes = { mix: memo(MixNode) };

/** Draft cards in `spaceId`. Pure function of state. */
export function mixExtraNodes(state: AppState, spaceId: string): Node[] {
  return Object.values(state.mixDrafts)
    .filter((d) => d.spaceId === spaceId)
    .map((d) => ({ id: MIX_PREFIX + d.id, type: "mix", position: d.position, data: { draftId: d.id }, deletable: false }));
}

/** Dashed edges from each source into its draft, and from every combined base into the design it produced. */
export function mixExtraEdges(state: AppState, spaceId: string): Edge[] {
  const edges: Edge[] = [];
  for (const d of Object.values(state.mixDrafts)) {
    if (d.spaceId !== spaceId) continue;
    for (const src of d.sourceIds) {
      if (state.designs[src]?.spaceId !== spaceId) continue;
      edges.push({
        id: `${src}->${MIX_PREFIX}${d.id}`,
        source: src,
        target: MIX_PREFIX + d.id,
        animated: true,
        style: { strokeWidth: 1.5, strokeDasharray: "5 4" },
      });
    }
  }
  // A combined design has parentId = its first base (drawn by the canvas); add the other bases here.
  for (const d of Object.values(state.designs)) {
    const bases = d.request?.bases;
    if (d.spaceId !== spaceId || !bases || bases.length < 2) continue;
    for (const b of bases) {
      if (b.id === d.parentId || state.designs[b.id]?.spaceId !== spaceId) continue;
      edges.push({
        id: `${b.id}->${d.id}`,
        source: b.id,
        target: d.id,
        animated: d.status === "generating",
        style: { strokeWidth: 1.5 },
      });
    }
  }
  return edges;
}

export function mixOnNodeMove(id: string, type: string | undefined, pos: { x: number; y: number }): boolean {
  if (type !== "mix" || !id.startsWith(MIX_PREFIX)) return false;
  useApp.getState().moveMix(id.slice(MIX_PREFIX.length), pos);
  return true;
}

const isDesign = (id: string | null | undefined): id is string => !!id && !!useApp.getState().designs[id];

/** Only designs feed a connection; it may end on another design (starts a mix) or on a draft (adds a source). */
export function isValidMixConnection(c: Connection | Edge): boolean {
  if (c.source === c.target || !isDesign(c.source)) return false;
  if (c.target.startsWith(MIX_PREFIX)) {
    const draft = useApp.getState().mixDrafts[c.target.slice(MIX_PREFIX.length)];
    return !!draft && !draft.sourceIds.includes(c.source);
  }
  return isDesign(c.target);
}

/** A valid connection was made: add to a draft, or start a mix of two designs between them. */
export function onMixConnect(c: Connection) {
  if (!isValidMixConnection(c)) return;
  const s = useApp.getState();
  if (c.target.startsWith(MIX_PREFIX)) return s.addMixSource(c.target.slice(MIX_PREFIX.length), c.source);
  startMixBetween(c.source, c.target);
}

function startMixBetween(a: string, b: string) {
  const s = useApp.getState();
  const da = s.designs[a];
  const db = s.designs[b];
  if (!da || !db) return;
  const x = (da.position.x + db.position.x) / 2 + NODE_W / 2 - MIX_W / 2;
  const y = Math.max(da.position.y, db.position.y) + NODE_H + 120;
  s.startMix(da.spaceId, { x, y }, [a, b]);
}

/**
 * Dropped without a valid handle target: on empty canvas start a draft from the dragged card; on a draft's or
 * another card's body (not its handle) add to / start a mix.
 */
export function onMixConnectEnd(
  rf: ReactFlowInstance | null,
  event: MouseEvent | TouchEvent,
  state: FinalConnectionState,
) {
  if (state.isValid || !rf) return;
  const from = state.fromNode?.id;
  if (!isDesign(from)) return;
  const point = "changedTouches" in event ? event.changedTouches[0] : event;
  if (!point) return;
  const hit = document.elementFromPoint(point.clientX, point.clientY);
  const nodeEl = hit?.closest<HTMLElement>(".react-flow__node");
  const s = useApp.getState();
  if (nodeEl) {
    const id = nodeEl.dataset.id;
    if (id?.startsWith(MIX_PREFIX)) s.addMixSource(id.slice(MIX_PREFIX.length), from);
    else if (isDesign(id) && id !== from) startMixBetween(from, id);
    return;
  }
  if (!hit?.closest(".react-flow__pane")) return;
  const p = rf.screenToFlowPosition({ x: point.clientX, y: point.clientY });
  s.startMix(s.designs[from].spaceId, { x: p.x - MIX_W / 2, y: p.y - 16 }, [from]);
}

function MixNode({ data }: NodeProps<MixNodeType>) {
  const draft = useApp((s) => s.mixDrafts[data.draftId]);
  const sources = useApp(useShallow((s) => (draft?.sourceIds ?? []).map((id) => s.designs[id]).filter((d) => !!d)));
  const agent = useApp((s) => s.agentChoice);
  const removeMixSource = useApp((s) => s.removeMixSource);
  const closeMix = useApp((s) => s.closeMix);
  const setMixInput = useApp((s) => s.setMixInput);
  // Text and count live in the (locally persisted) draft, so a reload or HMR full reload keeps them.
  const text = draft?.text ?? "";
  const count = draft?.count ?? 1;
  const areaRef = useRef<HTMLTextAreaElement>(null);

  // A new draft takes the keyboard right away (after React Flow finishes the connection, which moves focus);
  // adding a source later keeps the caret where it was.
  useEffect(() => {
    const t = setTimeout(() => areaRef.current?.focus({ preventScroll: true }), 50);
    return () => clearTimeout(t);
  }, []);

  if (!draft) return null;
  const mix = sources.length > 1;
  const ready = sources.filter((d) => d.status === "ready");
  const canSend = ready.length > 0;
  const AgentIcon = AGENTS[agent].Icon;
  const sendLabel = mix
    ? `Mix ${sources.length} into ${count} design${count === 1 ? "" : "s"}`
    : `Generate ${count} variation${count === 1 ? "" : "s"}`;

  const send = () => {
    if (!canSend) return;
    if (generateFromSources({ sourceIds: draft.sourceIds, instruction: text, count, origin: draft.position })) closeMix(draft.id);
  };

  return (
    <div
      role="group"
      aria-label={mix ? "New mix" : "New card"}
      className="w-[340px] rounded-xl border border-dashed border-ring/70 bg-card text-card-foreground shadow-lg"
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          closeMix(draft.id);
        }
      }}
    >
      <Handle type="target" position={Position.Top} className="!size-3 !bg-primary" />
      <div className="flex items-center gap-2 px-3 pt-2.5">
        <span className="grid size-6 place-items-center rounded-md bg-muted text-muted-foreground">
          {mix ? <Combine className="size-3.5" /> : <Sparkles className="size-3.5" />}
        </span>
        <span className="text-sm font-medium">{mix ? `Mix of ${sources.length}` : "New card"}</span>
        <button
          type="button"
          aria-label="Discard draft (Esc)"
          title="Discard (Esc)"
          onClick={() => closeMix(draft.id)}
          className="nodrag ml-auto grid size-6.5 max-md:size-10 place-items-center rounded-md text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          <X className="size-3.5" />
        </button>
      </div>
      <div className="flex flex-wrap gap-1 px-3 pt-2" aria-label="Made from">
        {sources.map((d) => (
          <span key={d.id} className="inline-flex h-6 max-md:h-8 max-w-full items-center gap-1 rounded-full bg-muted pr-1 pl-2 text-xs">
            <span className="truncate">{d.title}</span>
            {sources.length > 1 && (
              <button
                type="button"
                aria-label={`Remove ${d.title}`}
                onClick={() => removeMixSource(draft.id, d.id)}
                className="nodrag grid size-4.5 max-md:size-7 place-items-center rounded-full text-muted-foreground hover:bg-background/60 hover:text-foreground"
              >
                <X className="size-3" />
              </button>
            )}
          </span>
        ))}
        {!mix && <span className="inline-flex h-6 items-center px-1 text-xs text-muted-foreground">Connect more cards to mix them</span>}
      </div>
      <div className="px-3 pt-2">
        <textarea
          ref={areaRef}
          rows={3}
          value={text}
          aria-label={mix ? "How should they be combined?" : "What should it become?"}
          placeholder={mix ? "Optional: steer the mix, e.g. layout of the first, colours of the second…" : "What should it become? e.g. a darker, denser version…"}
          className="nodrag nowheel block w-full resize-none rounded-[10px] border border-input bg-foreground/[0.04] px-2.5 py-2 text-sm max-md:min-h-24 max-md:text-base leading-[1.45] outline-none placeholder:text-muted-foreground focus:border-ring focus:ring-3 focus:ring-ring/50"
          onChange={(e) => setMixInput(data.draftId, { text: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              send();
            }
          }}
        />
      </div>
      <div className="flex flex-wrap items-center gap-2 p-3 pt-2">
        <div role="group" aria-label={mix ? "Combinations" : "Variations"} className="nodrag inline-flex gap-px rounded-md bg-foreground/5 p-0.5">
          {Array.from({ length: MAX_COUNT }, (_, i) => i + 1).map((n) => (
            <button
              key={n}
              type="button"
              aria-pressed={n === count}
              aria-label={`${n}`}
              onClick={() => setMixInput(data.draftId, { count: n })}
              className={cn(
                "h-5.5 w-5 max-md:h-10 max-md:w-9 rounded-[5px] text-[11.5px] tabular-nums outline-none focus-visible:ring-2 focus-visible:ring-ring",
                n === count ? "bg-primary font-semibold text-primary-foreground" : "text-muted-foreground hover:text-foreground",
              )}
            >
              {n}
            </button>
          ))}
        </div>
        <button
          type="button"
          disabled={!canSend}
          onClick={send}
          title={`${sendLabel} (↵) · ${AGENTS[agent].label} works on it`}
          className="nodrag ml-auto inline-flex h-8 max-md:h-11 items-center gap-1.5 rounded-[10px] bg-primary px-2.5 text-[13px] font-medium text-primary-foreground outline-none hover:bg-primary/80 focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50"
        >
          <AgentIcon className="size-3.5" />
          {sendLabel}
          <ArrowUp className="size-3.5" />
        </button>
      </div>
    </div>
  );
}
