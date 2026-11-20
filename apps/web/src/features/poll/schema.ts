// Shared (client + server) shapes for the decisioning layer / visual poll.
import { z } from "zod";

export const POLL_ID_RE = /^[A-Za-z0-9_-]{6,32}$/;
export const PollId = z.string().regex(POLL_ID_RE, "Invalid poll id");

const MAX_HTML = 2_000_000; // per option snapshot

// ---- /api/decisions ----------------------------------------------------------

export const DecisionsRequestSchema = z.object({
  designs: z
    .array(
      z.object({
        id: z.string().min(1).max(64),
        title: z.string().max(300),
        rationale: z.string().max(4000).default(""),
        html: z.string().max(MAX_HTML),
      }),
    )
    .min(2, "Add at least two designs to the poll first.")
    .max(16, "That's a lot of candidates - keep it to 16 or fewer."),
  /** Optional hint from the designer about what they're unsure about. */
  context: z.string().max(2000).optional(),
});
export type DecisionsRequest = z.infer<typeof DecisionsRequestSchema>;

export interface SuggestedDecision {
  question: string;
  why: string;
  options: { designId: string; label: string }[];
}
export interface DecisionsResponse {
  decisions: SuggestedDecision[];
}

// ---- /api/polls --------------------------------------------------------------

export const PollOptionSchema = z.object({
  id: z.string().min(1).max(64),
  /** Source design on the designer's canvas (used to star winners). */
  designId: z.string().min(1).max(64),
  label: z.string().trim().min(1).max(120),
  title: z.string().max(300),
  rationale: z.string().max(4000).default(""),
  html: z.string().min(1).max(MAX_HTML),
});

export const PollDecisionSchema = z.object({
  id: z.string().min(1).max(64),
  question: z.string().trim().min(1, "Every decision needs a question.").max(300),
  why: z.string().max(1000).default(""),
  options: z.array(PollOptionSchema).min(2, "Every decision needs at least two options.").max(4),
});

export const CreatePollSchema = z.object({
  title: z.string().trim().min(1).max(200),
  intro: z.string().max(2000).default(""),
  decisions: z.array(PollDecisionSchema).min(1).max(8),
});
export type CreatePollRequest = z.input<typeof CreatePollSchema>;

export const VoteRequestSchema = z.object({
  voterId: z.string().min(4).max(64),
  voter: z.string().trim().min(1, "Please tell us your name.").max(80),
  decisionId: z.string().min(1).max(64),
  optionId: z.string().min(1).max(64),
  comment: z.string().trim().max(2000).optional(),
});
export type VoteRequest = z.infer<typeof VoteRequestSchema>;

export type PollOption = z.infer<typeof PollOptionSchema>;
export type PollDecision = z.infer<typeof PollDecisionSchema>;

export interface Vote {
  id: string;
  voterId: string;
  voter: string;
  decisionId: string;
  optionId: string;
  comment?: string;
  createdAt: number;
  updatedAt: number;
}

export interface Poll {
  id: string;
  title: string;
  intro: string;
  createdAt: number;
  decisions: PollDecision[];
  votes: Vote[];
}

export interface CreatePollResponse {
  id: string;
  path: string;
  /** Network-reachable origins of this server (e.g. http://192.168.1.20:3123). */
  lanOrigins: string[];
}

// ---- results -----------------------------------------------------------------

export interface OptionTally {
  optionId: string;
  count: number;
  voters: { name: string; comment?: string }[];
}
export interface DecisionTally {
  decisionId: string;
  total: number;
  options: OptionTally[];
  /** Option ids with the highest count (several on a tie, empty when no votes). */
  leaders: string[];
}

export function tallyDecision(decision: Pick<PollDecision, "id" | "options">, votes: Vote[]): DecisionTally {
  const mine = votes.filter((v) => v.decisionId === decision.id);
  const options = decision.options.map((o) => {
    const vs = mine.filter((v) => v.optionId === o.id).sort((a, b) => a.createdAt - b.createdAt);
    return { optionId: o.id, count: vs.length, voters: vs.map((v) => ({ name: v.voter, comment: v.comment })) };
  });
  const max = Math.max(0, ...options.map((o) => o.count));
  return {
    decisionId: decision.id,
    total: mine.length,
    options,
    leaders: max > 0 ? options.filter((o) => o.count === max).map((o) => o.optionId) : [],
  };
}

/** Pull a readable message out of a failed fetch response. */
export async function readError(res: Response): Promise<string> {
  try {
    const body = (await res.json()) as { error?: string };
    if (body?.error) return body.error;
  } catch {
    // not JSON
  }
  return `Request failed (${res.status})`;
}

/** First zod issue as a readable sentence, prefixed with where it happened. */
export function issueMessage(error: z.ZodError, fallback = "Invalid request."): string {
  const issue = error.issues[0];
  if (!issue) return fallback;
  const where = issue.path.length ? `${issue.path.join(".")}: ` : "";
  return `${where}${issue.message}`;
}
