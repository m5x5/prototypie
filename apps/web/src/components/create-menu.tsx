"use client";
import { useRef, useState } from "react";
import {
  AppWindow,
  ChevronDown,
  FolderPlus,
  Layers,
  LayoutDashboard,
  Plus,
  Wand2,
} from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "cn";
import { brandButtonColors } from "@/components/ui/button";
import { newExploration, ProjectSpaceDialog } from "@/features/nested";
import { useApp } from "@/store";
import { ROOT_SPACE_ID } from "@/store/core";
import { openNewPage } from "@/features/spokes";
import { LayoutFinderDialog } from "@/features/pod-layouts/layout-finder";
import { ImportFromAppDialog } from "@/features/imports/import-from-app";

/**
 * The header's single create control: a round "+" that shows a "New" label on hover/focus and opens a tile
 * menu (Page = the "New page" dialog, Layout finden… = LayoutFinderDialog, Import from app… = ImportFromAppDialog, Exploration = a new empty nested space, Project = a project on the
 * main canvas, see ProjectSpaceDialog). The main canvas holds only projects, so there every tile but Project is
 * disabled. Replaces the two separate buttons so the breadcrumb gets the room. Keyboard comes from the menu:
 * Enter/Space opens, arrows move, Esc closes and returns focus to "+".
 */

export function CreateMenu() {
  const [picker, setPicker] = useState(false);
  const [finder, setFinder] = useState(false);
  const [importer, setImporter] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const atRoot = useApp((s) => s.activeSpaceId === ROOT_SPACE_ID);
  const tiles = [
    {
      key: "page",
      label: "Page",
      hint: "Rough full-page prototype from a prompt",
      Icon: Wand2,
      run: openNewPage,
    },
    {
      key: "exploration",
      label: "Exploration",
      hint: "New empty space inside this canvas",
      Icon: FolderPlus,
      run: newExploration,
    },
    {
      key: "project",
      label: "Project",
      hint: "One of your projects, on the main canvas",
      Icon: LayoutDashboard,
      run: () => setPicker(true),
    },
    {
      key: "layout",
      label: "Layout finden…",
      hint: "Two pod shapes, one relation, four layouts",
      Icon: Layers,
      run: () => setFinder(true),
    },
    {
      key: "import",
      label: "Import from app…",
      hint: "Recreate an existing element from the app's source",
      Icon: AppWindow,
      run: () => setImporter(true),
    },
  ];
  return (
    <>
      <DropdownMenu orientation="horizontal">
        <DropdownMenuTrigger
          ref={trigger}
          aria-label="New"
          title="New page, exploration or project"
          className={cn(
            "group/new inline-flex h-8 min-w-8 max-md:h-10 max-md:min-w-10 shrink-0 items-center justify-center rounded-full px-2 text-sm font-medium outline-none transition-[background-color,padding] duration-200 hover:pr-2.5 focus-visible:pr-2.5 focus-visible:ring-3 aria-expanded:pr-2.5 motion-reduce:transition-none",
            brandButtonColors,
          )}
        >
          <Plus className="size-4 shrink-0" aria-hidden />
          <span
            aria-hidden
            className="inline-flex max-w-0 items-center gap-0.5 overflow-hidden whitespace-nowrap opacity-0 transition-[max-width,opacity,margin] duration-200 group-hover/new:ml-1 group-hover/new:max-w-16 group-hover/new:opacity-100 group-focus-visible/new:ml-1 group-focus-visible/new:max-w-16 group-focus-visible/new:opacity-100 group-aria-expanded/new:ml-1 group-aria-expanded/new:max-w-16 group-aria-expanded/new:opacity-100 motion-reduce:transition-none"
          >
            New
            <ChevronDown className="size-3.5 opacity-60" />
          </span>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          sideOffset={8}
          className="w-[26rem] max-w-[calc(100vw-24px)] p-2"
          aria-label="Create new"
        >
          <div className="grid grid-cols-2 gap-1.5">
            {tiles.map(({ key, label, hint, Icon, run }) => {
              const off = atRoot && key !== "project";
              return (
                <DropdownMenuItem
                  key={key}
                  disabled={off}
                  onClick={run}
                  className="flex-col items-start gap-2.5 rounded-xl border p-3 text-left focus:border-ring/40 data-highlighted:bg-accent"
                >
                  <span className="grid size-10 place-items-center rounded-lg border bg-muted">
                    <Icon className="size-5" />
                  </span>
                  <span className="font-medium">{label}</span>
                  <span className="text-xs leading-snug text-muted-foreground">
                    {off ? "Open a project first" : hint}
                  </span>
                </DropdownMenuItem>
              );
            })}
          </div>
        </DropdownMenuContent>
      </DropdownMenu>
      <ProjectSpaceDialog
        open={picker}
        onOpenChange={setPicker}
        returnFocus={trigger}
      />
      <LayoutFinderDialog open={finder} onOpenChange={setFinder} returnFocus={trigger} />
      <ImportFromAppDialog open={importer} onOpenChange={setImporter} returnFocus={trigger} />
    </>
  );
}
