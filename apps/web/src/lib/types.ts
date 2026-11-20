// Shared domain model. Feature slices may ADD optional fields, never rename these.
import type { EditableFields } from "@/lib/ai-edit";

export type DesignStatus = "generating" | "ready" | "error";

/** "page" = full-page block of clay, "spoke" = one component pulled out of a page, "variation" = generic. */
export type DesignKind = "page" | "spoke" | "variation";

/** A card. In-place AI edits add `editing` / `editError` / `versions` (see src/lib/ai-edit.ts). */
export interface Design extends EditableFields {
  id: string;
  spaceId: string;
  parentId?: string; // design this was derived from (draws an edge when in the same space)
  kind: DesignKind;
  title: string;
  rationale: string;
  html: string; // self-contained HTML document, rendered in a sandboxed iframe
  status: DesignStatus;
  error?: string;
  position: { x: number; y: number };
  createdAt: number;
  instruction?: string; // prompt that produced it
  starred?: boolean;
  /** Free-text note the designer keeps on this card (fullscreen side panel). Synced like any design field. */
  notes?: string;
  /** Virtual width the design is laid out at (default 1280). Phone screens use ~400. */
  viewport?: number;
  /** Virtual height of a fixed-size screen (e.g. 960 for a 640×960 phone): the card frame keeps viewport:viewportHeight. Omitted: free height. */
  viewportHeight?: number;
  /** A reusable component (not a page): listed first for # mentions. Set for component-scope results, imports and agent cards marked as components. */
  component?: boolean;
  /** The request that produced it (minus the base HTML, which comes from parentId). Lets it be retried. */
  request?: Omit<VariationRequest, "baseHtml">;
  /** When it was handed to the coding agent to implement in the product (see /api/implement). */
  implementRequestedAt?: number;
  /**
   * Set when this card is its space's design guidelines (at most one per space; the newest mark wins).
   * Every generation in the space, and in nested spaces without their own, must follow its content.
   */
  guidelinesAt?: number;
  /**
   * Set when this card is the main variant of a component that live views place by reference
   * (`<prototypie-slot data-design="…">`, see src/features/live-view): the newest mark among a component and its variations wins.
   */
  mainAt?: number;
}

/** A canvas. The root space has no parentSpaceId. Nested spaces are entered through a portal node. */
export interface Space {
  id: string;
  name: string;
  parentSpaceId?: string;
  createdAt: number;
  /** The local project folder this space belongs to (sub-spaces inherit it); without it the card matches by name. */
  localPath?: string;
  /** "project": a top-level project space. The main canvas holds only projects, and projects never nest. */
  kind?: "project";
}

/** Shape the AI returns for one variation. */
export interface Variation {
  title: string;
  rationale: string;
  html: string;
  /** Virtual width to lay the design out at (e.g. 1240 for a tablet). Omitted: the card keeps its frame. */
  viewport?: number;
  /** Height of a fixed-size screen (e.g. 960 for a 640×960 phone): the card keeps that ratio. */
  viewportHeight?: number;
}

/** Request body for POST /api/variations */
export interface VariationRequest {
  /** HTML of the design to vary. Omit to generate from scratch from `instruction`. */
  baseHtml?: string;
  /**
   * Several designs to COMBINE into new ones (multi-selection "Combine"). Each result merges ideas from all of
   * them, steered by `instruction`. When set, `baseHtml` is not used. Stored requests keep id/title only (html "").
   */
  bases?: { id: string; title: string; html: string }[];
  baseTitle?: string;
  instruction: string;
  count: number;
  /** Optional: describe one component/region to explore in isolation (hub & spoke). */
  focus?: string;
  /** "page" for full pages, "component" to return only the focused component as its own doc. */
  scope?: "page" | "component";
  /** Skip planning and build exactly this direction (used to retry one failed design). */
  direction?: { title: string; brief: string };
  /** Agentation notes pinned on the base design; the agent must address each one. */
  annotations?: { element?: string; comment: string }[];
  /** Who builds the designs: "codex" (server default when omitted) calls the LLM directly; "claude" queues work for a Claude Code watcher. The inspector defaults to "claude". */
  agent?: AgentChoice;
  /** Pod data @-mentioned in the prompt: the selected entries and fields that flow into the generation. */
  podContext?: PodContextEntry[];
  /** Space the results land in; the server looks up that space's design guidelines (see Design.guidelinesAt). */
  spaceId?: string;
  /**
   * "+ → Import from app…": recreate an element that already exists in the project's app (read from its source)
   * instead of inventing one. Claude agent only; the result is a component-scope card.
   */
  importFrom?: ImportFrom;
  /** Cards #-mentioned in the prompt: existing components the result must reuse as they are. Stored requests keep id/title only (html ""). */
  components?: ComponentRef[];
}

/** A card #-mentioned as a component to reuse (see src/features/pod-mention/components.tsx). */
export interface ComponentRef {
  id: string;
  title: string;
  html: string;
}

export interface ImportFrom {
  /** What to import, as the designer named it ("Toolbar", "Note list item"). */
  element: string;
  /** Where the app lives: a local repo path or a URL (prefilled with the space's project repo). */
  source: string;
  /** Optional screenshot of the element as a data: URL. Never stored on the card; the server saves it next to the queue item. */
  screenshot?: string;
}

/* ---- Solid pod data catalog (GET /api/pod-catalog) ---- */

export type PodEntryKind = "pod" | "container" | "shape" | "resource";
/** live = publicly readable right now; 401 = locked, values only from documented captures; ref = known only by reference; cache = app cache with a timestamp. */
export type PodAccess = "live" | "401" | "ref" | "cache";

export interface PodField {
  /** Predicate or field name, e.g. "ical:due". */
  p: string;
  /** Documented value (resources), if any. */
  v?: string;
  /** Expected type (shapes), if any. */
  type?: string;
}

export interface PodCatalogEntry {
  id: string;
  kind: PodEntryKind;
  /** Pod id from PodCatalog.pods. */
  pod: string;
  label: string;
  /** Path relative to the pod root ("" for the pod itself), or a short description for shapes. */
  path: string;
  access: PodAccess;
  /** When the values were last seen (dc:modified, app cache time …). */
  stand?: string;
  types?: string[];
  fields: PodField[];
  /** Documented edges to other entries, e.g. schema:about → course. */
  relations?: { p: string; target: string }[];
  /** Entries this one contains or describes (container contents, instances of a shape). */
  children?: string[];
  /** Honest note about gaps ("listing is 401", "content unread"). */
  note?: string;
  /** Where the values come from. */
  provenance: string;
  /** Result of the live probe, when the entry has a public URL. */
  probe?: { status: number | "error"; checkedAt: string; contains?: string[] };
}

export interface PodCatalog {
  version: number;
  pods: { id: string; label: string; display: string; note?: string }[];
  entries: PodCatalogEntry[];
  /** When the live probes ran (cached for a few minutes). */
  checkedAt: string;
}

/** One @-mentioned catalog entry as it is sent with a generation request (only the selected fields). */
export interface PodContextEntry {
  id: string;
  kind: PodEntryKind;
  label: string;
  /** Pod display name + path, e.g. "my-pod · :3100/me/tasks/…". */
  source: string;
  access: PodAccess;
  stand?: string;
  fields: PodField[];
  relations?: { p: string; target: string }[];
  note?: string;
}

export type AgentChoice = "codex" | "claude";

/** NDJSON events streamed back from /api/variations, one JSON object per line. */
export type VariationEvent =
  | { type: "job"; id: string }
  | { type: "plan"; directions: { title: string; brief: string }[] }
  | { type: "variation"; index: number; variation: Variation }
  | { type: "error"; index: number | null; message: string }
  | { type: "done" };
