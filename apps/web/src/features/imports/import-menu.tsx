"use client";
import { useState, type ReactNode, type Ref } from "react";
import { FolderInput, Loader2, MoreHorizontal } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useApp } from "@/store";
import { ROOT_SPACE_ID, NODE_W } from "@/store/core";
import { enterSpace } from "@/features/nested/nav";
import type { DesignKind } from "@/lib/types";

interface BundleDesign {
  key?: string;
  parentKey?: string;
  group: string;
  title: string;
  rationale: string;
  html: string;
  viewport?: number;
  source?: string;
  instruction?: string;
  kind?: DesignKind;
  starred?: boolean;
  position?: { x: number; y: number };
}
interface ImportFile {
  name: string;
  designs: BundleDesign[];
}
interface BundleInfo {
  file: string;
  name: string;
  count: number;
}

/**
 * Imports a bundle into its own space: a new project on the main canvas, else an exploration of the current space.
 * Designs keep their position and parent links (key/parentKey) when the bundle has them, else one row per group.
 */
export async function importBundle(url: string) {
  const res = await fetch(url, { cache: "no-store" });
  if (!res.ok) throw new Error(`Bundle missing (${res.status}).`);
  const data = (await res.json()) as ImportFile;
  const s = useApp.getState();
  // The main canvas holds only projects: there the bundle becomes a project, inside a space an exploration of it.
  const { spaceId } =
    s.activeSpaceId === ROOT_SPACE_ID
      ? s.createProject({ name: data.name })
      : s.createExploration({ parentSpaceId: s.activeSpaceId, name: data.name });

  const groups = [...new Set(data.designs.map((d) => d.group))];
  const idByKey = new Map<string, string>();
  const pending: { id: string; parentKey: string }[] = [];
  groups.forEach((group, row) => {
    data.designs
      .filter((d) => d.group === group)
      .forEach((d, col) => {
        const id = s.addDesign({
          spaceId,
          kind: d.kind ?? "page",
          title: d.title,
          rationale: d.rationale,
          html: d.html,
          viewport: d.viewport,
          starred: d.starred,
          status: "ready",
          instruction: d.instruction ?? (d.source ? `Imported from ${d.source}` : undefined),
          position: d.position ?? { x: col * (NODE_W + 160), y: row * 620 },
        });
        if (d.key) idByKey.set(d.key, id);
        if (d.parentKey) pending.push({ id, parentKey: d.parentKey });
      });
  });
  for (const p of pending) {
    const parentId = idByKey.get(p.parentKey);
    if (parentId) s.updateDesign(p.id, { parentId });
  }
  enterSpace(spaceId);
  return { spaceId, count: data.designs.length };
}

/** Header overflow menu: import a script-built bundle. Moving work between browsers needs nothing: canvas state is shared via /api/state. */
export function ImportMenu({ items, triggerRef }: { items?: ReactNode; triggerRef?: Ref<HTMLButtonElement> } = {}) {
  const [busy, setBusy] = useState(false);
  const [bundles, setBundles] = useState<BundleInfo[] | null>(null);

  const wrap = async (fn: () => Promise<void>) => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DropdownMenu
      onOpenChange={(open) => {
        if (open)
          void fetch("/api/bundles", { cache: "no-store" })
            .then((r) => r.json())
            .then(setBundles)
            .catch(() => setBundles([]));
      }}
    >
      <DropdownMenuTrigger
        render={<Button ref={triggerRef} size="icon-sm" className="max-md:size-10" variant="ghost" aria-label="More actions" title="More actions" disabled={busy} />}
      >
        {busy ? <Loader2 className="animate-spin" /> : <MoreHorizontal />}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-64 max-w-[calc(100vw-1.5rem)]">
        {items && (
          <>
            <DropdownMenuGroup>{items}</DropdownMenuGroup>
            <DropdownMenuSeparator />
          </>
        )}
        <DropdownMenuGroup>
        <DropdownMenuLabel className="text-xs text-muted-foreground">Import bundle</DropdownMenuLabel>
        {bundles === null ? (
          <DropdownMenuItem disabled>
            <Loader2 className="animate-spin" /> Loading…
          </DropdownMenuItem>
        ) : bundles.length === 0 ? (
          <DropdownMenuItem disabled>No bundles yet</DropdownMenuItem>
        ) : (
          bundles.map((b) => (
            <DropdownMenuItem
              key={b.file}
              onClick={() =>
                void wrap(async () => {
                  const { count } = await importBundle(b.file);
                  toast.success(`Imported ${count} designs from ${b.name}`);
                })
              }
            >
              <FolderInput />
              <span className="truncate">{b.name}</span>
              <span className="ml-auto text-xs tabular-nums text-muted-foreground">{b.count}</span>
            </DropdownMenuItem>
          ))
        )}
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
