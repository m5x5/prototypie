import type { ComponentRef, Design, PodContextEntry } from "@/lib/types";

/**
 * In-place AI edits: an instruction rewrites an existing card's html (same id, position and edges) instead of
 * adding a variation. The server drives the edit and writes the result straight into the card's shared record;
 * the previous html is kept in `versions` so it can be restored.
 */

/** A previous state of a card's content, newest last. */
export interface DesignVersion {
  html: string;
  title: string;
  rationale: string;
  viewport?: number;
  viewportHeight?: number;
  /** When this content was replaced, and by which instruction. */
  replacedAt: number;
  instruction?: string;
}

export interface DesignEditing {
  /** Server job running the edit (cancel: DELETE /api/edit/<designId>). */
  jobId: string;
  instruction: string;
  agent: "codex" | "claude";
  startedAt: number;
}

/** Fields an in-place edit adds to a Design record. */
export interface EditableFields {
  /** Set while an AI edit is running on this card. */
  editing?: DesignEditing;
  /** Why the last edit failed (cleared by the next edit). */
  editError?: string;
  /** Earlier contents, newest last (at most MAX_VERSIONS). */
  versions?: DesignVersion[];
}

export type EditableDesign = Design & EditableFields;

export const MAX_VERSIONS = 5;

/** Body of POST /api/edit. */
export interface EditRequest {
  designId: string;
  instruction: string;
  /** Element(s) to change; everything else stays as it is. */
  focus?: string;
  /** Agentation notes pinned on the card; each one gets addressed. */
  annotations?: { element?: string; comment: string }[];
  agent?: "codex" | "claude";
  podContext?: PodContextEntry[];
  /** #-mentioned cards whose components the edit must reuse as they are. */
  components?: ComponentRef[];
}
