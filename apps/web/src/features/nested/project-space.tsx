"use client";
import { useEffect, useId, useMemo, useRef, useState, type RefObject } from "react";
import { FolderGit2, LayoutDashboard, Loader2, Search } from "lucide-react";
import { toast } from "sonner";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { useApp } from "@/store";
import { cn } from "@/lib/utils";
import { enterSpace } from "./nav";

/**
 * "+ → Project": pick one of the user's projects (private-projects.ttl via GET /api/projects) and get a project on
 * the main canvas named after it, with a "Projekt-Info" card (name, repo, keywords, README paragraph) and an
 * Agentation note "[prototypie project-info <spaceId>]" so an agent enriches it. A project that is already on the
 * canvas (same local folder) is opened instead. "New project" makes one from what was typed: a name, or an
 * absolute folder path (its folder name becomes the name).
 */

interface Project {
  id: string;
  name: string;
  repo?: string;
  localPath?: string;
  keywords: string[];
  readme?: string;
}

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

/** Self-contained card HTML with only the fields the project file (and README) actually has. */
function infoCardHtml(p: Project): string {
  const row = (label: string, value: string) => `<div class="row"><dt>${label}</dt><dd>${value}</dd></div>`;
  return `<!doctype html><html lang="de"><head><meta charset="utf-8"><title>Projekt-Info · ${esc(p.name)}</title>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Google+Sans+Flex:wght@400;500;600;700&display=swap">
<style>
:root{--bg:#fff;--fg:#18181b;--mfg:#71717a;--line:#e4e4e7;--chip:#f4f4f5}
@media (prefers-color-scheme:dark){:root{--bg:#0a0a0a;--fg:#fafafa;--mfg:#a1a1aa;--line:#27272a;--chip:#18181b}}
*{box-sizing:border-box;margin:0}
body{background:var(--bg);color:var(--fg);font:15px/1.5 "Google Sans Flex",system-ui,sans-serif;padding:40px}
.k{font-size:12px;font-weight:600;letter-spacing:.08em;text-transform:uppercase;color:var(--mfg)}
h1{font-size:34px;font-weight:650;margin:6px 0 18px;overflow-wrap:anywhere}
p.readme{max-width:62ch;font-size:16px;margin-bottom:22px}
dl{display:grid;gap:10px;border-top:1px solid var(--line);padding-top:16px;max-width:760px}
.row{display:grid;grid-template-columns:140px 1fr;gap:12px}
dt{color:var(--mfg)}
dd{overflow-wrap:anywhere}
code{font:13px ui-monospace,SFMono-Regular,Menlo,monospace}
.chips{display:flex;flex-wrap:wrap;gap:6px}
.chip{background:var(--chip);border:1px solid var(--line);border-radius:999px;padding:2px 10px;font-size:13px}
</style></head><body>
<div class="k">Projekt-Info</div>
<h1>${esc(p.name)}</h1>
${p.readme ? `<p class="readme">${esc(p.readme)}</p>` : ""}
<dl>
${p.repo ? row("Repository", `<code>${esc(p.repo)}</code>`) : ""}
${p.localPath && p.localPath !== p.repo ? row("Lokal", `<code>${esc(p.localPath)}</code>`) : ""}
${p.keywords.length ? row("Keywords", `<div class="chips">${p.keywords.map((k) => `<span class="chip">${esc(k)}</span>`).join("")}</div>`) : ""}
</dl>
</body></html>`;
}

/** A typed absolute path stands for that folder; anything else is a name. */
function fromQuery(query: string): { name: string; localPath?: string } {
  const q = query.trim().replace(/\/+$/, "");
  if (q.startsWith("/") && q.length > 1) return { name: q.split("/").pop()!, localPath: q };
  return { name: q || "New project" };
}

/** Create the project (and, for a listed one, its info card and the agent note), then step into it. */
async function createProjectSpace(project: Project | null, query: string) {
  const s = useApp.getState();
  if (!project) {
    const { spaceId, created } = s.createProject(fromQuery(query));
    enterSpace(spaceId);
    if (!created) toast(`„${useApp.getState().spaces[spaceId]?.name}“ is already on the canvas`);
    return;
  }
  const res = await fetch(`/api/projects/${encodeURIComponent(project.id)}`, { cache: "no-store" });
  const detail = res.ok ? ((await res.json()) as Project) : project;
  const { spaceId, created } = s.createProject({ name: detail.name, localPath: detail.localPath });
  if (!created) {
    enterSpace(spaceId);
    toast(`„${useApp.getState().spaces[spaceId]?.name}“ is already on the canvas`);
    return;
  }
  const cardId = s.addDesign({
    spaceId,
    kind: "page",
    title: `Projekt-Info · ${detail.name}`,
    rationale: "Belegte Felder aus private-projects.ttl (Name, Repository, Keywords) und, falls das Repo lokal liegt, der erste README-Absatz.",
    html: infoCardHtml(detail),
    status: "ready",
    instruction: `Project ${detail.name} from private-projects.ttl`,
    position: { x: 0, y: 0 },
  });
  enterSpace(spaceId, cardId);
  const note = await fetch(`/api/projects/${encodeURIComponent(project.id)}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ spaceId }),
  }).catch(() => undefined);
  if (note?.ok) toast.success(`Space „${detail.name}“ angelegt`, { description: "Ein Agent sammelt weitere Projekt-Infos." });
  else toast.warning(`Space „${detail.name}“ angelegt`, { description: "Kein Agent erreichbar: die Projekt-Info bleibt bei der ersten Karte." });
}

export function ProjectSpaceDialog({
  open,
  onOpenChange,
  returnFocus,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  returnFocus?: RefObject<HTMLElement | null>;
}) {
  const [data, setData] = useState<{ available: boolean; projects: Project[] } | null>(null);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [busy, setBusy] = useState(false);
  const listId = useId();
  const listRef = useRef<HTMLUListElement>(null);

  useEffect(() => {
    if (!open) return;
    let live = true;
    fetch("/api/projects", { cache: "no-store" })
      .then((r) => r.json())
      .then((d: { available: boolean; projects: Project[] }) => live && setData(d))
      .catch(() => live && setData({ available: false, projects: [] }));
    return () => {
      live = false;
    };
  }, [open]);

  const options = useMemo(() => {
    const q = query.trim().toLowerCase();
    const hits = (data?.projects ?? []).filter(
      (p) => !q || p.name.toLowerCase().includes(q) || p.keywords.some((k) => k.toLowerCase().includes(q)),
    );
    return [null, ...hits] as (Project | null)[];
  }, [data, query]);
  const current = Math.min(active, options.length - 1);

  useEffect(() => {
    listRef.current?.querySelector(`[data-index="${current}"]`)?.scrollIntoView({ block: "nearest" });
  }, [current]);

  const choose = async (p: Project | null) => {
    if (busy) return;
    setBusy(true);
    try {
      await createProjectSpace(p, query);
      setQuery("");
      setActive(0);
      onOpenChange(false);
    } catch (err) {
      toast.error("Space konnte nicht angelegt werden", { description: err instanceof Error ? err.message : String(err) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        // Start fresh next time.
        if (!o) {
          setQuery("");
          setActive(0);
        }
        onOpenChange(o);
      }}
    >
      <DialogContent className="gap-0 p-0 sm:max-w-md" finalFocus={returnFocus}>
        <DialogHeader className="border-b px-4 pt-4 pb-3">
          <DialogTitle>New project</DialogTitle>
          <DialogDescription>Pick a project: it gets its name, folder and a project info card.</DialogDescription>
        </DialogHeader>
        <div className="flex items-center gap-2 border-b px-4">
          <Search className="size-4 shrink-0 text-muted-foreground" aria-hidden />
          <input
            autoFocus
            role="combobox"
            aria-expanded
            aria-controls={listId}
            aria-activedescendant={`${listId}-${current}`}
            aria-label="Search projects"
            placeholder="Search projects, or type a name or folder path…"
            value={query}
            disabled={busy}
            onChange={(e) => {
              setQuery(e.target.value);
              setActive(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setActive(Math.min(current + 1, options.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActive(Math.max(current - 1, 0));
              } else if (e.key === "Home" || e.key === "End") {
                e.preventDefault();
                setActive(e.key === "Home" ? 0 : options.length - 1);
              } else if (e.key === "Enter") {
                e.preventDefault();
                void choose(options[current]);
              }
            }}
            className="h-11 min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
          />
          {busy && <Loader2 className="size-4 animate-spin text-muted-foreground" aria-hidden />}
        </div>
        <ul ref={listRef} id={listId} role="listbox" aria-label="Projects" className="max-h-80 overflow-y-auto p-1.5">
          {options.map((p, i) => (
            <li
              key={p?.id ?? "__empty"}
              id={`${listId}-${i}`}
              data-index={i}
              role="option"
              aria-selected={i === current}
              onMouseMove={() => setActive(i)}
              onClick={() => void choose(p)}
              className={cn(
                "flex cursor-pointer items-start gap-2.5 rounded-md px-2.5 py-2 text-sm",
                i === current && "bg-accent text-accent-foreground",
              )}
            >
              {p ? (
                <FolderGit2 className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden />
              ) : (
                <LayoutDashboard className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden />
              )}
              <span className="min-w-0 flex-1">
                <span className="block truncate font-medium">{p ? p.name : `New project${query.trim() ? ` „${fromQuery(query).name}“` : ""}`}</span>
                <span className="block truncate text-xs text-muted-foreground">
                  {p ? p.keywords.join(" · ") || "No keywords" : (fromQuery(query).localPath ?? "Type a name or an absolute folder path")}
                </span>
              </span>
            </li>
          ))}
          {data === null && (
            <li className="flex items-center gap-2 px-2.5 py-3 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" aria-hidden /> Loading projects…
            </li>
          )}
          {data && !data.available && (
            <li className="px-2.5 py-3 text-sm text-muted-foreground">
              No project list found (private-projects.ttl). You can still start a new project.
            </li>
          )}
          {data?.available && options.length === 1 && (
            <li className="px-2.5 py-3 text-sm text-muted-foreground">No project matches “{query}”.</li>
          )}
        </ul>
      </DialogContent>
    </Dialog>
  );
}
