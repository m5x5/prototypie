"use client";
import { useEffect, useId, useRef, useState, type RefObject } from "react";
import { AppWindow, ImagePlus, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useApp, type AppState } from "@/store";
import { ROOT_SPACE_ID } from "@/store/core";
import { canCreateIn } from "@/features/nested/root-guard";

/**
 * "+ → Import from app…": name an element that already exists in the project's app (e.g. "Toolbar") and where the
 * app lives (prefilled with the space's project repo), optionally with a screenshot. It becomes a one-card job in
 * the current space; with the Claude agent the queue item is marked `mode: "import"`, so the agent recreates the
 * element faithfully from the source instead of inventing one (scripts/agent-queue.md).
 */

const MAX_SHOT = 8 * 1024 * 1024;

interface Project {
  id: string;
  name: string;
  repo?: string;
  localPath?: string;
}

/** The top-level space above `spaceId` (projects get one each via "+ → Space"), or undefined on the main canvas. */
function topSpaceName(s: AppState, spaceId: string): string | undefined {
  let cur = s.spaces[spaceId];
  for (let n = 0; cur?.parentSpaceId && cur.parentSpaceId !== ROOT_SPACE_ID && n < 20; n++) {
    const up = s.spaces[cur.parentSpaceId];
    if (!up) break;
    cur = up;
  }
  return cur && cur.id !== ROOT_SPACE_ID ? cur.name : undefined;
}

const norm = (v: string) => v.trim().toLowerCase();

function readAsDataUrl(file: File) {
  return new Promise<string>((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result));
    r.onerror = () => reject(r.error ?? new Error("Couldn't read the file"));
    r.readAsDataURL(file);
  });
}

export function ImportFromAppDialog({
  open,
  onOpenChange,
  returnFocus,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  returnFocus?: RefObject<HTMLElement | null>;
}) {
  const spaceId = useApp((s) => s.activeSpaceId);
  const agent = useApp((s) => s.agentChoice);
  const [element, setElement] = useState("");
  const [source, setSource] = useState("");
  const [shot, setShot] = useState<{ name: string; url: string } | null>(null);
  const touched = useRef(false);
  const fileRef = useRef<HTMLInputElement>(null);
  const ids = { element: useId(), source: useId() };

  // Prefill the source with the repo of the project this space belongs to (matched by the top-level space's name).
  useEffect(() => {
    if (!open) return;
    touched.current = false;
    const name = topSpaceName(useApp.getState(), spaceId);
    let live = true;
    fetch("/api/projects", { cache: "no-store" })
      .then((r) => r.json())
      .then((d: { projects: Project[] }) => {
        const p = name ? d.projects.find((x) => norm(x.name) === norm(name) || norm(x.id) === norm(name)) : undefined;
        if (live && !touched.current) setSource(p?.localPath ?? p?.repo ?? "");
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [open, spaceId]);

  const pickShot = async (file: File | undefined) => {
    if (!file) return;
    if (!/^image\/(png|jpe?g|webp|gif)$/.test(file.type)) return toast.error("Use a PNG, JPEG, WebP or GIF screenshot.");
    if (file.size > MAX_SHOT) return toast.error("Screenshot is larger than 8 MB.");
    try {
      setShot({ name: file.name, url: await readAsDataUrl(file) });
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  };

  const canSubmit = agent === "claude" && !!element.trim() && !!source.trim();
  const submit = () => {
    if (!canSubmit || !canCreateIn(spaceId)) return;
    const name = element.trim();
    const label = `Import · ${name}`;
    void useApp
      .getState()
      .generate({
        spaceId,
        kind: "variation",
        label,
        request: {
          instruction: "",
          count: 1,
          focus: name,
          scope: "component",
          agent,
          importFrom: { element: name, source: source.trim(), ...(shot ? { screenshot: shot.url } : {}) },
        },
      })
      .then((ids) => {
        const d = ids.map((id) => useApp.getState().designs[id]).find(Boolean);
        if (d?.status === "error") toast.error(`${label} failed`, { description: d.error });
      });
    toast(`Importing ${name}…`, { description: "The agent recreates it from the app's source." });
    setElement("");
    setShot(null);
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md" finalFocus={returnFocus}>
        <DialogHeader>
          <DialogTitle>Import from app</DialogTitle>
          <DialogDescription>Recreate an element that already exists in the app, as a card to improve on.</DialogDescription>
        </DialogHeader>
        <form
          className="grid gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
        >
          <div className="grid gap-1.5">
            <Label htmlFor={ids.element}>Element</Label>
            <Input
              id={ids.element}
              autoFocus
              value={element}
              onChange={(e) => setElement(e.target.value)}
              placeholder="e.g. Toolbar"
              maxLength={200}
            />
          </div>
          <div className="grid gap-1.5">
            <Label htmlFor={ids.source}>Source</Label>
            <Input
              id={ids.source}
              value={source}
              onChange={(e) => {
                touched.current = true;
                setSource(e.target.value);
              }}
              placeholder="Repo path or URL"
              className="font-mono text-xs"
              maxLength={2000}
            />
          </div>
          <div className="flex items-center gap-2">
            <input
              ref={fileRef}
              type="file"
              accept="image/png,image/jpeg,image/webp,image/gif"
              className="hidden"
              onChange={(e) => {
                void pickShot(e.target.files?.[0]);
                e.target.value = "";
              }}
            />
            {shot ? (
              <div className="flex min-w-0 items-center gap-2 rounded-md border px-2 py-1 text-xs">
                {/* eslint-disable-next-line @next/next/no-img-element -- local data: URL preview */}
                <img src={shot.url} alt="" className="size-6 rounded object-cover" />
                <span className="truncate">{shot.name}</span>
                <Button type="button" size="icon-xs" variant="ghost" aria-label="Remove screenshot" onClick={() => setShot(null)}>
                  <X />
                </Button>
              </div>
            ) : (
              <Button type="button" size="sm" variant="outline" onClick={() => fileRef.current?.click()}>
                <ImagePlus /> Screenshot (optional)
              </Button>
            )}
          </div>
          {agent !== "claude" && (
            <p className="text-xs text-muted-foreground">Needs the Claude agent: switch the inspector&apos;s agent to Claude.</p>
          )}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={!canSubmit}>
              <AppWindow /> Import
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
