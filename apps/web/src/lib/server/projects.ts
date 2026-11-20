import { readdir, readFile, stat } from "node:fs/promises";
import path from "node:path";

/**
 * The user's projects from private-projects.ttl (schema:WebSite with schema:name, schema:codeRepository,
 * schema:keywords), for "+ → Space". Path: PROTOTYPIE_PROJECTS_TTL (unset: no project list; a project can still be started
 * from a name or a folder path).
 * The file is small and regular, so a tiny Turtle reader is enough (no RDF dependency).
 */

export const PROJECTS_TTL = process.env.PROTOTYPIE_PROJECTS_TTL ?? path.join(process.cwd(), "data", "projects.ttl");

export interface Project {
  /** The subject's local name (proj:<id>), safe for URLs. */
  id: string;
  name: string;
  /** schema:codeRepository as written in the file. */
  repo?: string;
  /** Where the repo is on this machine: the stored path, else a folder of the same name next to the TTL file. */
  localPath?: string;
  keywords: string[];
  /** The project's record on the Solid pod it was exported from (projects/index.ttl next to its producer's clients/index.ttl). */
  pod?: { doc: string; subject: string };
}

export interface ProjectDetail extends Project {
  /** First prose paragraph of the local README (plain text, shortened, lines that look like secrets dropped). */
  readme?: string;
}

type Term = { kind: "iri" | "pname" | "literal" | "a"; value: string } | { kind: "punct"; value: ";" | "," | "." };

/** Tokens of a simple Turtle document: IRIs, prefixed names, "literals" (with optional @lang/^^type), `a`, ; , . */
function tokenize(src: string): Term[] {
  const out: Term[] = [];
  let i = 0;
  while (i < src.length) {
    const c = src[i];
    if (/\s/.test(c)) {
      i++;
    } else if (c === "#") {
      while (i < src.length && src[i] !== "\n") i++;
    } else if (c === "<") {
      const end = src.indexOf(">", i);
      out.push({ kind: "iri", value: src.slice(i + 1, end) });
      i = end + 1;
    } else if (c === '"') {
      let v = "";
      i++;
      while (i < src.length && src[i] !== '"') {
        if (src[i] === "\\" && i + 1 < src.length) {
          const e = src[i + 1];
          v += e === "n" ? "\n" : e === "t" ? "\t" : e;
          i += 2;
        } else v += src[i++];
      }
      i++;
      // skip @lang or ^^type
      if (src[i] === "@") while (i < src.length && /[\w-@]/.test(src[i])) i++;
      else if (src.startsWith("^^", i)) {
        i += 2;
        while (i < src.length && !/[\s;,.]/.test(src[i])) i++;
      }
      out.push({ kind: "literal", value: v });
    } else if (c === ";" || c === ",") {
      out.push({ kind: "punct", value: c });
      i++;
    } else if (c === "." && !/[\w-]/.test(src[i + 1] ?? "")) {
      out.push({ kind: "punct", value: "." });
      i++;
    } else {
      let w = "";
      while (i < src.length && !/[\s;,<"]/.test(src[i]) && !(src[i] === "." && !/[\w-]/.test(src[i + 1] ?? ""))) w += src[i++];
      out.push(w === "a" ? { kind: "a", value: "a" } : { kind: "pname", value: w });
    }
  }
  return out;
}

/** Subjects with their predicates (prefixed name or IRI, as written) and object values. @prefix lines are skipped. */
export function parseTurtle(src: string): Map<string, Map<string, string[]>> {
  const t = tokenize(src);
  const subjects = new Map<string, Map<string, string[]>>();
  let i = 0;
  while (i < t.length) {
    if (t[i].kind === "pname" && t[i].value.startsWith("@")) {
      while (i < t.length && t[i].value !== ".") i++;
      i++;
      continue;
    }
    const subject = t[i++].value;
    const props = subjects.get(subject) ?? new Map<string, string[]>();
    subjects.set(subject, props);
    while (i < t.length && t[i].value !== ".") {
      const pred = t[i].kind === "a" ? "rdf:type" : t[i].value;
      i++;
      const values = props.get(pred) ?? [];
      while (i < t.length && t[i].value !== ";" && t[i].value !== ".") {
        if (t[i].value !== ",") values.push(t[i].value);
        i++;
      }
      props.set(pred, values);
      if (t[i]?.value === ";") i++;
    }
    i++;
  }
  return subjects;
}

async function isDir(p: string) {
  try {
    return (await stat(p)).isDirectory();
  } catch {
    return false;
  }
}

/** <pod>/projects/index.ttl + the subject IRI, when the producer is that pod's <pod>/clients/index.ttl#… and the subject a proj: name. */
function podRecord(subject: string, props: Map<string, string[]>, projNs?: string): Project["pod"] {
  const producer = props.get("schema:producer")?.[0];
  if (!producer || !projNs || !subject.startsWith("proj:")) return undefined;
  try {
    const u = new URL(producer);
    if (!/^https?:$/.test(u.protocol) || !u.pathname.endsWith("/clients/index.ttl")) return undefined;
    return { doc: new URL("../projects/index.ttl", u).href.replace(/#.*$/, ""), subject: projNs + subject.slice(5) };
  } catch {
    return undefined;
  }
}

/** All schema:WebSite projects, sorted by name. undefined when the file can't be read. */
export async function listProjects(): Promise<Project[] | undefined> {
  let src: string;
  try {
    src = await readFile(PROJECTS_TTL, "utf8");
  } catch {
    return undefined;
  }
  const near = path.dirname(PROJECTS_TTL);
  // proj: is the subjects' prefix; a pod's projects/index.ttl writes it as urn:example:project:<id>.
  const projNs = /@prefix\s+proj:\s*<([^>]*)>/.exec(src)?.[1];
  const out: Project[] = [];
  for (const [subject, props] of parseTurtle(src)) {
    if (!props.get("rdf:type")?.includes("schema:WebSite")) continue;
    const id = subject.replace(/^.*[:#/]/, "");
    if (!/^[\w.-]+$/.test(id)) continue;
    const name = props.get("schema:name")?.[0] ?? id;
    const repo = props.get("schema:codeRepository")?.[0];
    let localPath: string | undefined;
    if (repo && path.isAbsolute(repo) && (await isDir(repo))) localPath = repo;
    else if (repo && (await isDir(path.join(near, path.basename(repo))))) localPath = path.join(near, path.basename(repo));
    out.push({ id, name, repo, localPath, keywords: [...new Set(props.get("schema:keywords") ?? [])], pod: podRecord(subject, props, projNs) });
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * The local folder a space name stands for (adapter until a shared project-info source exists): a project from
 * private-projects.ttl whose name or id matches (case-insensitive), else a folder of that exact name next to the TTL.
 */
export async function matchLocalPath(name: string): Promise<string | undefined> {
  const key = name.trim().toLowerCase();
  if (!key) return undefined;
  const hit = (await listProjects())?.find((p) => p.localPath && (p.name.toLowerCase() === key || p.id.toLowerCase() === key));
  if (hit) return hit.localPath;
  const base = name.trim();
  if (base !== path.basename(base) || base.startsWith(".")) return undefined;
  const near = path.join(path.dirname(PROJECTS_TTL), base);
  return (await isDir(near)) ? near : undefined;
}

const SECRET = /(token|secret|password|passwd|api[_-]?key|private[_-]?key|bearer|authorization)|[A-Za-z0-9_\-]{32,}|-----BEGIN/i;

/** First prose paragraph of a README: no headings, badges, HTML, code or lists; at most ~400 characters. */
export function firstParagraph(md: string): string | undefined {
  const blocks = md
    .replace(/```[\s\S]*?```/g, "")
    .replace(/<!--[\s\S]*?-->/g, "")
    .split(/\n\s*\n/);
  for (const block of blocks) {
    const lines = block
      .split("\n")
      .map((l) => l.trim())
      .filter((l) => l && !/^(#|!\[|\[!\[|<|[-*+] |\d+\. |>|\||---|===)/.test(l) && !SECRET.test(l));
    const text = lines
      .join(" ")
      .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
      .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
      .replace(/[*_`]+/g, "")
      .replace(/\s+/g, " ")
      .trim();
    if (text.length >= 20) return text.length > 400 ? `${text.slice(0, 397).replace(/\s+\S*$/, "")}…` : text;
  }
  return undefined;
}

export async function getProject(id: string): Promise<ProjectDetail | undefined> {
  const project = (await listProjects())?.find((p) => p.id === id);
  if (!project) return undefined;
  if (!project.localPath) return project;
  try {
    const file = (await readdir(project.localPath)).find((n) => /^readme(\.md|\.markdown|\.txt)?$/i.test(n));
    if (!file) return project;
    const readme = firstParagraph((await readFile(path.join(project.localPath, file), "utf8")).slice(0, 20_000));
    return readme ? { ...project, readme } : project;
  } catch {
    return project;
  }
}
