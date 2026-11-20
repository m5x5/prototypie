// Server-only JSON persistence for polls: data/polls/<id>.json. Never import from client code.
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { networkInterfaces } from "node:os";
import path from "node:path";
import { POLL_ID_RE, type Poll } from "./schema";

const DIR = path.join(process.cwd(), "data", "polls");

function fileFor(id: string) {
  if (!POLL_ID_RE.test(id)) throw new Error("Invalid poll id");
  return path.join(DIR, `${id}.json`);
}

export async function readPoll(id: string): Promise<Poll | null> {
  if (!POLL_ID_RE.test(id)) return null;
  try {
    return JSON.parse(await readFile(fileFor(id), "utf8")) as Poll;
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw err;
  }
}

export async function writePoll(poll: Poll): Promise<void> {
  await mkdir(DIR, { recursive: true });
  const file = fileFor(poll.id);
  const tmp = `${file}.${process.pid}.${Date.now()}.tmp`;
  await writeFile(tmp, JSON.stringify(poll), "utf8");
  await rename(tmp, file); // atomic replace, readers never see half a file
}

// Serialise read-modify-write per poll so concurrent votes don't clobber each other.
const locks = new Map<string, Promise<unknown>>();

export function updatePoll<T>(id: string, fn: (poll: Poll) => T | Promise<T>): Promise<{ poll: Poll; result: T } | null> {
  const prev = locks.get(id) ?? Promise.resolve();
  const next = prev
    .catch(() => undefined)
    .then(async () => {
      const poll = await readPoll(id);
      if (!poll) return null;
      const result = await fn(poll);
      await writePoll(poll);
      return { poll, result };
    });
  locks.set(id, next);
  void next.finally(() => {
    if (locks.get(id) === next) locks.delete(id);
  });
  return next;
}

/** Origins other machines on the network can use to reach this server. */
export function lanOrigins(requestUrl: string): string[] {
  const url = new URL(requestUrl);
  const port = url.port ? `:${url.port}` : "";
  const out: string[] = [];
  for (const list of Object.values(networkInterfaces())) {
    for (const net of list ?? []) {
      if (net.family === "IPv4" && !net.internal) out.push(`${url.protocol}//${net.address}${port}`);
    }
  }
  return out;
}
