import fs from "node:fs/promises";
import path from "node:path";

/**
 * Bundles: JSON files of designs in public/imports/, built by your own scripts.
 * GET lists them for the import menu. Moving work between browsers no longer needs bundles: canvas state is
 * shared through /api/state.
 */
const DIR = path.join(process.cwd(), "public/imports");

export async function GET() {
  const files = (await fs.readdir(DIR).catch(() => [])).filter((f) => f.endsWith(".json"));
  const list = await Promise.all(
    files.map(async (f) => {
      try {
        const stat = await fs.stat(path.join(DIR, f));
        const data = JSON.parse(await fs.readFile(path.join(DIR, f), "utf8"));
        return { file: `/imports/${f}`, name: String(data.name ?? f), count: data.designs?.length ?? 0, updatedAt: stat.mtimeMs };
      } catch {
        return null;
      }
    }),
  );
  return Response.json(list.filter(Boolean).sort((a, b) => b!.updatedAt - a!.updatedAt));
}
