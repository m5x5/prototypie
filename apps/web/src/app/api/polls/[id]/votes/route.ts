import { nanoid } from "nanoid";
import { issueMessage, tallyDecision, VoteRequestSchema, type Vote } from "@/features/poll/schema";
import { readPoll, updatePoll } from "@/features/poll/server-store";
import type { Poll } from "@/features/poll/schema";

function results(poll: Poll) {
  return { votes: poll.votes, tallies: poll.decisions.map((d) => tallyDecision(d, poll.votes)) };
}

/** Votes + per-decision tallies, without the heavy HTML snapshots. */
export async function GET(_request: Request, ctx: RouteContext<"/api/polls/[id]/votes">) {
  const { id } = await ctx.params;
  const poll = await readPoll(id);
  if (!poll) return Response.json({ error: "Poll not found." }, { status: 404 });
  return Response.json(results(poll), { headers: { "Cache-Control": "no-store" } });
}

/** Cast or change a vote. One vote per voter per decision (re-voting replaces it). */
export async function POST(request: Request, ctx: RouteContext<"/api/polls/[id]/votes">) {
  const { id } = await ctx.params;
  let json: unknown;
  try {
    json = await request.json();
  } catch {
    return Response.json({ error: "Body must be JSON." }, { status: 400 });
  }
  const parsed = VoteRequestSchema.safeParse(json);
  if (!parsed.success) {
    return Response.json({ error: issueMessage(parsed.error, "Invalid vote.") }, { status: 400 });
  }
  const v = parsed.data;

  let invalid: string | null = null;
  const updated = await updatePoll(id, (poll) => {
    const decision = poll.decisions.find((d) => d.id === v.decisionId);
    if (!decision) return (invalid = "Unknown decision.");
    if (!decision.options.some((o) => o.id === v.optionId)) return (invalid = "Unknown option.");
    const now = Date.now();
    const existing = poll.votes.find((x) => x.voterId === v.voterId && x.decisionId === v.decisionId);
    if (existing) {
      Object.assign(existing, { voter: v.voter, optionId: v.optionId, comment: v.comment || undefined, updatedAt: now });
    } else {
      const vote: Vote = { id: nanoid(10), ...v, comment: v.comment || undefined, createdAt: now, updatedAt: now };
      poll.votes.push(vote);
    }
    // Keep the voter's name consistent across their votes.
    for (const x of poll.votes) if (x.voterId === v.voterId) x.voter = v.voter;
    return null;
  });

  if (!updated) return Response.json({ error: "Poll not found." }, { status: 404 });
  if (invalid) return Response.json({ error: invalid }, { status: 400 });
  return Response.json(results(updated.poll));
}
