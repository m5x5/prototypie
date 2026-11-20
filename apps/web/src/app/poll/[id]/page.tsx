import { Suspense } from "react";
import type { Metadata } from "next";
import { Loader2, SearchX } from "lucide-react";
import { readPoll } from "@/features/poll/server-store";
import { PollPlayer } from "./poll-player";

export const metadata: Metadata = {
  title: "Design poll · Prototypie",
  description: "Play with the live prototypes and vote on the decisions.",
};

export default function PollPage({ params }: PageProps<"/poll/[id]">) {
  return (
    <Suspense fallback={<Shell><Loader2 className="size-5 animate-spin text-muted-foreground" /></Shell>}>
      {params.then(({ id }) => (
        <PollLoader id={id} />
      ))}
    </Suspense>
  );
}

async function PollLoader({ id }: { id: string }) {
  const poll = await readPoll(id);
  if (!poll) {
    return (
      <Shell>
        <SearchX className="size-6 text-muted-foreground" />
        <h1 className="text-lg font-semibold">This poll doesn&apos;t exist</h1>
        <p className="max-w-sm text-sm text-muted-foreground">
          The link may be mistyped, or the poll was removed. Ask the designer for a fresh link.
        </p>
      </Shell>
    );
  }
  return <PollPlayer poll={poll} />;
}

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <main className="flex min-h-dvh flex-1 flex-col items-center justify-center gap-2 bg-muted/40 p-6 text-center">
      {children}
    </main>
  );
}
