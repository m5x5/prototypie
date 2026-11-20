"use client";
import { GlassMiniMap } from "./glass-minimap";
import { useCallback, useRef, useState, useSyncExternalStore } from "react";
import { useTheme } from "next-themes";
import {
  Background,
  BackgroundVariant,
  Controls,
  ReactFlow,
  applyNodeChanges,
  type Edge,
  type Node,
  type NodeChange,
  type NodeTypes,
  type ReactFlowInstance,
} from "@xyflow/react";
import { useApp, type AppState } from "@/store";
import { DeleteShortcut, SelectionBar } from "@/features/multi/selection-bar";
import {
  isValidMixConnection,
  mixExtraEdges,
  mixExtraNodes,
  mixNodeTypes,
  mixOnNodeMove,
  onMixConnect,
  onMixConnectEnd,
} from "@/features/multi/mix";
import { DesignNode } from "./design-node";
import { nestedExtraNodes, nestedNodeTypes, nestedOnNodeMove } from "@/features/nested";
import { pollExtraNodes, pollNodeTypes, pollOnNodeMove } from "@/features/poll";
import { spokesExtraNodes, spokesNodeTypes, spokesOnNodeMove } from "@/features/spokes";
import { framePadding } from "@/lib/canvas-insets";

/** React Flow chrome (zoom controls, minimap) drawn with the app's light/dark tokens, whatever colorMode it ended up with. */
const FLOW_THEME = {
  // Set on the app shell root (app-shell.tsx), shared with the header gradient so the two never drift.
  "--xy-background-color": "var(--canvas-bg)",
  "--xy-controls-button-background-color": "var(--card)",
  "--xy-controls-button-background-color-hover": "var(--muted)",
  "--xy-controls-button-color": "var(--foreground)",
  "--xy-controls-button-color-hover": "var(--foreground)",
  "--xy-controls-button-border-color": "var(--border)",
  "--xy-minimap-background-color": "var(--background)",
  "--xy-minimap-node-background-color": "var(--muted-foreground)",
  "--xy-minimap-mask-background-color": "color-mix(in oklab, var(--foreground) 12%, transparent)",
} as React.CSSProperties;

const nodeTypes: NodeTypes = {
  design: DesignNode,
  ...nestedNodeTypes,
  ...pollNodeTypes,
  ...spokesNodeTypes,
  ...mixNodeTypes,
};

const MULTI_SELECT_KEYS = ["Shift", "Meta", "Control"];
const extraNodeSources = [nestedExtraNodes, pollExtraNodes, spokesExtraNodes, mixExtraNodes];
const extraMoveHandlers = [nestedOnNodeMove, pollOnNodeMove, spokesOnNodeMove, mixOnNodeMove];

function selectGraph(s: AppState) {
  const spaceId = s.activeSpaceId;
  const designs = Object.values(s.designs).filter((d) => d.spaceId === spaceId);
  const nodes: Node[] = designs.map((d) => ({
    id: d.id,
    type: "design",
    position: d.position,
    data: { designId: d.id },
    selected: s.selectedId === d.id,
    dragHandle: undefined,
  }));
  for (const src of extraNodeSources) nodes.push(...src(s, spaceId));
  const ids = new Set(designs.map((d) => d.id));
  // A round's siblings share one prompt: label only the middle edge of each round, not every edge.
  const rounds = new Map<string, string[]>();
  for (const d of designs) {
    if (!d.parentId || !ids.has(d.parentId)) continue;
    const key = `${d.parentId}|${d.instruction ?? ""}`;
    rounds.set(key, [...(rounds.get(key) ?? []), d.id]);
  }
  const labelled = new Set(
    [...rounds.values()].map((sib) => {
      const sorted = [...sib].sort((a, b) => s.designs[a].position.x - s.designs[b].position.x);
      return sorted[Math.floor((sorted.length - 1) / 2)];
    }),
  );
  const edges: Edge[] = designs
    .filter((d) => d.parentId && ids.has(d.parentId))
    .map((d) => ({
      id: `${d.parentId}->${d.id}`,
      source: d.parentId!,
      target: d.id,
      animated: d.status === "generating",
      label: d.instruction && labelled.has(d.id) ? truncate(d.instruction, 60) : undefined,
      labelBgPadding: [6, 3] as [number, number],
      labelBgBorderRadius: 4,
      style: { strokeWidth: 1.5 },
    }));
  // Draft cards (drag a handle onto empty canvas) and the extra bases of combined designs.
  edges.push(...mixExtraEdges(s, spaceId));
  return { nodes, edges };
}

type Graph = ReturnType<typeof selectGraph>;

/** One level deep: nodes/edges are flat records whose values are primitives or small plain objects/arrays. */
function sameItem(a: object, b: object) {
  if (a === b) return true;
  const ka = Object.keys(a);
  if (ka.length !== Object.keys(b).length) return false;
  return ka.every((k) => {
    const x = (a as Record<string, unknown>)[k];
    const y = (b as Record<string, unknown>)[k];
    if (Object.is(x, y)) return true;
    if (!x || !y || typeof x !== "object" || typeof y !== "object") return false;
    const kx = Object.keys(x);
    return kx.length === Object.keys(y).length && kx.every((j) => Object.is((x as never)[j], (y as never)[j]));
  });
}
const sameList = (a: object[], b: object[]) => a.length === b.length && a.every((x, i) => sameItem(x, b[i]));
const sameGraph = (a: Graph, b: Graph) => sameList(a.nodes, b.nodes) && sameList(a.edges, b.edges);

/** Reuse the previous object for every node/edge that didn't change, so memoised node components skip re-rendering. */
function reuse<T extends { id: string }>(prev: T[], next: T[]): T[] {
  const old = new Map(prev.map((x) => [x.id, x]));
  return next.map((x) => {
    const o = old.get(x.id);
    return o && sameItem(o, x) ? o : x;
  });
}

/**
 * A graph selector that returns the previous result while the graph is unchanged, and otherwise reuses
 * every node/edge object that didn't change. Unrelated store updates (drafts, jobs, notes) then don't
 * touch React Flow at all, and memoised node components only re-render for their own changes.
 */
function makeGraphSelector() {
  let prev: Graph | undefined;
  return (s: AppState): Graph => {
    const next = selectGraph(s);
    if (prev && sameGraph(prev, next)) return prev;
    prev = prev ? { nodes: reuse(prev.nodes, next.nodes), edges: reuse(prev.edges, next.edges) } : next;
    return prev;
  };
}

export function Canvas() {
  const [selectStableGraph] = useState(makeGraphSelector);
  const graph = useApp(selectStableGraph);
  const updateDesign = useApp((s) => s.updateDesign);
  const select = useApp((s) => s.select);

  // Local copy so dragging is smooth; positions are committed to the store on drop.
  const [nodes, setNodes] = useState<Node[]>(graph.nodes);
  const [syncedFrom, setSyncedFrom] = useState(graph.nodes);
  if (syncedFrom !== graph.nodes) {
    // Store changed: adopt its nodes (adjusting state during render, per React docs).
    // A multi-selection (shift/⌘-click) lives only here; keep it while it still includes the store's selection.
    setSyncedFrom(graph.nodes);
    const multi = nodes.filter((n) => n.selected);
    const keep = multi.length > 1 && multi.some((n) => n.id === useApp.getState().selectedId);
    if (keep) {
      const ids = new Set(multi.map((n) => n.id));
      setNodes(graph.nodes.map((n) => (ids.has(n.id) && !n.selected ? { ...n, selected: true } : n)));
    } else setNodes(graph.nodes);
  }

  const onNodesChange = useCallback(
    (changes: NodeChange[]) => {
      setNodes((prev) => {
        const next = applyNodeChanges(changes, prev);
        for (const c of changes) {
          if (c.type === "position" && c.dragging === false) {
            const n = next.find((x) => x.id === c.id);
            if (!n) continue;
            // Defer store writes until after this state update.
            queueMicrotask(() => {
              if (useApp.getState().designs[c.id]) updateDesign(c.id, { position: n.position });
              else for (const h of extraMoveHandlers) if (h(c.id, n.type, n.position)) break;
            });
          } else if (c.type === "select" && c.selected && useApp.getState().designs[c.id]) {
            // Extending a multi-selection keeps the inspector on the first design.
            if (next.filter((n) => n.selected).length === 1) queueMicrotask(() => select(c.id));
          }
        }
        return next;
      });
    },
    [updateDesign, select],
  );

  // Follow the app theme (next-themes) so Controls/MiniMap match it; "system" got stuck on light after hydration.
  const { resolvedTheme } = useTheme();
  const mounted = useSyncExternalStore(
    () => () => {},
    () => true,
    () => false,
  );

  const onPaneClick = useCallback(() => select(null), [select]);
  // Drag a card's handle: onto empty canvas starts a draft card, onto another card or a draft starts/extends a mix.
  const rfRef = useRef<ReactFlowInstance | null>(null);
  const onInit = useCallback((rf: ReactFlowInstance) => {
    rfRef.current = rf;
  }, []);
  const onConnectEnd = useCallback(
    (event: MouseEvent | TouchEvent, state: Parameters<typeof onMixConnectEnd>[2]) => onMixConnectEnd(rfRef.current, event, state),
    [],
  );

  return (
    <ReactFlow
      nodes={nodes}
      edges={graph.edges}
      nodeTypes={nodeTypes}
      onNodesChange={onNodesChange}
      onPaneClick={onPaneClick}
      onInit={onInit}
      onConnect={onMixConnect}
      onConnectEnd={onConnectEnd}
      isValidConnection={isValidMixConnection}
      fitView
      fitViewOptions={{ padding: framePadding(0.1) }}
      // Not onlyRenderVisibleElements: it unmounts nodes as they leave the viewport, so every pan
      // remounted design iframes (srcdoc re-parse, React commits, Agentation re-scanning all frames).
      // Nodes stay mounted; thumbnails load lazily on first view and are cheap while idle.
      // Trackpad: two-finger swipe pans, pinch (= ctrl + wheel) zooms. Mouse wheel pans too; ctrl/⌘ + wheel zooms.
      panOnScroll
      zoomOnScroll={false}
      zoomOnPinch
      panOnDrag
      zoomOnDoubleClick={false}
      nodeDragThreshold={4}
      // Explicit "light" until mounted, then the resolved theme. Not "system": React Flow resolves that via
      // matchMedia during render, so the server (no window → light) and a dark-OS client disagree on hydration.
      colorMode={mounted && resolvedTheme === "dark" ? "dark" : "light"}
      style={FLOW_THEME}
      minZoom={0.05}
      maxZoom={2}
      proOptions={{ hideAttribution: true }}
      deleteKeyCode={null}
      multiSelectionKeyCode={MULTI_SELECT_KEYS}
    >
      <Background variant={BackgroundVariant.Dots} gap={24} size={1} />
      <Controls position="bottom-left" className="max-md:!mb-20 max-md:!ml-3 max-md:[&_button]:!size-10 max-md:[&_button_svg]:!size-5" />
      <GlassMiniMap />
      <SelectionBar />
      <DeleteShortcut />
    </ReactFlow>
  );
}

function truncate(s: string, n: number) {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}
