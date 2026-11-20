"use client";
import type { RefObject } from "react";
import { BarChart3, Vote } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import { DockEntry, DockHead } from "@/components/activity-dock";
import { DropdownMenuItem } from "@/components/ui/dropdown-menu";
import { Button } from "@/components/ui/button";
import { useApp } from "@/store";
import { usePollUI } from "./client-utils";
import { ShareLink } from "./share-link";

function useDecisionsStatus() {
  const n = useApp((s) => s.pollCandidates.filter((id) => s.designs[id]).length);
  const published = useApp((s) => s.publishedPolls.length);
  const parts = [n ? `${n} in poll` : null, published ? `${published} shared` : null].filter(Boolean);
  return { n, status: parts.length ? parts.join(" · ") : "Share a poll" };
}

/** "Decisions" item of the header's More menu: opens the Decisions popover (anchored to the More button). */
export function DecisionsMenuItem() {
  const { status } = useDecisionsStatus();
  const setOpen = usePollUI((s) => s.setDockOpen);
  return (
    <DropdownMenuItem onClick={() => setOpen(true)} aria-label={`Decisions: ${status}`}>
      <Vote />
      <span>Decisions</span>
      <span className="ml-auto truncate text-xs text-muted-foreground">{status}</span>
    </DropdownMenuItem>
  );
}

/** The Decisions popover: poll candidates and the latest shared poll, anchored to the header's More button. */
export function DecisionsDock({ anchor }: { anchor: RefObject<HTMLElement | null> }) {
  const candidates = useApp(useShallow((s) => s.pollCandidates.filter((id) => s.designs[id]).map((id) => s.designs[id].title)));
  const latest = useApp((s) => s.publishedPolls[0]);
  const setPanel = usePollUI((s) => s.setOpen);
  const setTab = usePollUI((s) => s.setTab);
  const showResults = usePollUI((s) => s.showResults);
  const open = usePollUI((s) => s.dockOpen);
  const setOpen = usePollUI((s) => s.setDockOpen);
  const { status } = useDecisionsStatus();
  const n = candidates.length;
  const build = () => {
    setOpen(false);
    setTab("build");
    setPanel(true);
  };

  return (
    <DockEntry
      icon={<Vote />}
      name="Decisions"
      status={status}
      label={`Decisions: ${status}`}
      open={open}
      onOpenChange={setOpen}
      panelLabel="Share a poll"
      anchor={anchor}
    >
      <DockHead title="Share a poll" count={`${n} candidate${n === 1 ? "" : "s"}`} />
      <div className="min-h-0 space-y-3 overflow-y-auto p-3.5">
        {latest && (
          <section aria-label="Latest poll" className="space-y-1.5">
            <div className="flex items-baseline justify-between gap-2">
              <span className="truncate text-xs font-medium" title={latest.title}>
                {latest.title}
              </span>
              <span className="shrink-0 text-[11px] text-muted-foreground">
                {latest.decisions.length} decision{latest.decisions.length === 1 ? "" : "s"}
              </span>
            </div>
            <ShareLink url={latest.shareUrl} />
            <Button
              size="xs"
              variant="ghost"
              className="-ml-2"
              onClick={() => {
                setOpen(false);
                showResults(latest.id);
              }}
            >
              <BarChart3 /> See results
            </Button>
          </section>
        )}
        {n > 0 ? (
          <ul className="space-y-1" aria-label="Designs in the poll">
            {candidates.slice(0, 4).map((title, i) => (
              <li key={i} className="truncate rounded-md bg-muted/50 px-2 py-1 text-xs">
                {title}
              </li>
            ))}
            {n > 4 && <li className="px-2 text-[11px] text-muted-foreground">+{n - 4} more</li>}
          </ul>
        ) : (
          <p className="text-xs text-muted-foreground">Add two or more designs with “Poll” on a card, then build the poll.</p>
        )}
      </div>
      <div className="border-t bg-muted/30 p-2.5">
        <Button size="sm" className="w-full" onClick={build}>
          {latest ? "Build a new poll" : "Build poll"}
        </Button>
      </div>
    </DockEntry>
  );
}
