import { nanoid } from "nanoid";
import { CreatePollSchema, issueMessage, type CreatePollResponse, type Poll } from "@/features/poll/schema";
import { lanOrigins, writePoll } from "@/features/poll/server-store";

/** Publish a poll: decisions + HTML snapshots of every option. */
export async function POST(request: Request) {
  let json: unknown;
  try {
    json = await request.json();
  } catch {
    return Response.json({ error: "Body must be JSON." }, { status: 400 });
  }
  const parsed = CreatePollSchema.safeParse(json);
  if (!parsed.success) {
    return Response.json({ error: issueMessage(parsed.error, "Invalid poll.") }, { status: 400 });
  }

  const id = nanoid(10);
  const poll: Poll = { id, ...parsed.data, createdAt: Date.now(), votes: [] };
  try {
    await writePoll(poll);
  } catch (err) {
    return Response.json({ error: `Could not save the poll: ${(err as Error).message}` }, { status: 500 });
  }
  const body: CreatePollResponse = { id, path: `/poll/${id}`, lanOrigins: lanOrigins(request.url) };
  return Response.json(body, { status: 201 });
}
