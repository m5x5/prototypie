"use client";
import { Fragment, useState } from "react";
import { FolderPlus, Pencil } from "lucide-react";
import { useShallow } from "zustand/react/shallow";
import {
  Breadcrumb,
  BreadcrumbEllipsis,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useApp } from "@/store";
import type { Space } from "@/lib/types";
import { ROOT_SPACE_ID } from "@/store/core";
import { spacePath } from "./slice";
import { enterSpace } from "./nav";
import { canCreateIn } from "./root-guard";

/** Path breadcrumb and "new exploration". */
/** Breadcrumb of the current space; sits on the left of the header, after the logo. Going up = clicking the logo or a parent crumb. */
export function SpaceBreadcrumbImpl() {
  const path = useApp(useShallow((s) => spacePath(s, s.activeSpaceId)));
  return (
    <div className="flex min-w-0 items-center gap-1">
      {/* The main canvas isn't a crumb: the header logo leads there (goToMainCanvas); the separator after it stays. */}
      <SpacePath path={path.slice(1)} leadingSeparator />
    </div>
  );
}

/** Back to the main canvas (the header logo); a no-op when already there. */
export function goToMainCanvas() {
  if (useApp.getState().activeSpaceId !== ROOT_SPACE_ID) enterSpace(ROOT_SPACE_ID);
}

/** New empty exploration inside the current space, then step into it (the header's "+ → Exploration"). */
export function newExploration() {
  if (!canCreateIn()) return;
  const s = useApp.getState();
  const { spaceId } = s.createExploration({ parentSpaceId: s.activeSpaceId });
  enterSpace(spaceId);
}

/** Standalone "New exploration" button (the header uses the "+" create menu instead). */
export function NestedToolbarImpl() {
  return (
    <Tooltip>
      <TooltipTrigger render={<Button size="sm" variant="outline" onClick={newExploration} />}>
        <FolderPlus /> Exploration
      </TooltipTrigger>
      <TooltipContent>New empty exploration space inside this canvas</TooltipContent>
    </Tooltip>
  );
}

const MAX_CRUMBS = 4;

function SpacePath({ path, leadingSeparator }: { path: Space[]; leadingSeparator?: boolean }) {
  if (!path.length) return null;
  const current = path[path.length - 1];
  const ancestors = path.slice(0, -1);
  // Collapse the middle of deep paths into a menu: root … parent > current
  const collapsed = ancestors.length >= MAX_CRUMBS ? ancestors.slice(1, -1) : [];
  const visible = collapsed.length ? [ancestors[0], ancestors[ancestors.length - 1]] : ancestors;

  return (
    <Breadcrumb className="mr-1 min-w-0">
      {/* min-w-0 down the chain, so the names' own truncate works and the "Back up" button never gets pushed out. */}
      <BreadcrumbList className="min-w-0 flex-nowrap gap-1 sm:gap-1">
        {leadingSeparator && <BreadcrumbSeparator />}
        {visible.map((sp, i) => (
          <Fragment key={sp.id}>
            {i === 1 && collapsed.length > 0 && (
              <>
                <BreadcrumbItem>
                  <DropdownMenu>
                    <DropdownMenuTrigger
                      render={<Button size="icon-xs" variant="ghost" className="w-auto px-[3px]" aria-label="Show hidden spaces" />}
                    >
                      <BreadcrumbEllipsis className="size-auto" />
                    </DropdownMenuTrigger>
                    <DropdownMenuContent className="w-auto">
                      {collapsed.map((c) => (
                        <DropdownMenuItem key={c.id} onClick={() => enterSpace(c.id)}>
                          {c.name}
                        </DropdownMenuItem>
                      ))}
                    </DropdownMenuContent>
                  </DropdownMenu>
                </BreadcrumbItem>
                <BreadcrumbSeparator />
              </>
            )}
            <BreadcrumbItem className="min-w-0">
              <BreadcrumbLink
                render={<button type="button" />}
                className="max-w-36 cursor-pointer truncate px-1"
                title={sp.name}
                onClick={() => enterSpace(sp.id)}
              >
                {sp.name}
              </BreadcrumbLink>
            </BreadcrumbItem>
            <BreadcrumbSeparator />
          </Fragment>
        ))}
        <BreadcrumbItem className="min-w-0">
          <EditableName key={current.id} space={current} />
        </BreadcrumbItem>
      </BreadcrumbList>
    </Breadcrumb>
  );
}

function EditableName({ space }: { space: Space }) {
  const renameSpace = useApp((s) => s.renameSpace);
  const [draft, setDraft] = useState<string | null>(null);

  if (draft !== null) {
    const commit = () => {
      const name = draft.trim();
      if (name && name !== space.name) renameSpace(space.id, name);
      setDraft(null);
    };
    return (
      <input
        autoFocus
        aria-label="Space name"
        value={draft}
        size={Math.max(draft.length, 6)}
        onChange={(e) => setDraft(e.target.value)}
        onFocus={(e) => e.currentTarget.select()}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            setDraft(null);
          }
        }}
        className="h-6 max-w-56 rounded-md border border-input bg-background px-1.5 text-sm text-foreground outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
      />
    );
  }

  return (
    <span aria-current="page" className="group/name flex min-w-0 items-center gap-1 text-foreground">
      <button
        type="button"
        title="Rename"
        className="flex max-w-48 min-w-0 cursor-text items-center gap-1 rounded-md px-1 py-0.5 font-medium hover:bg-muted"
        onClick={() => setDraft(space.name)}
      >
        <span className="truncate">{space.name}</span>
        <Pencil className="size-3 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover/name:opacity-100" />
      </button>
    </span>
  );
}
