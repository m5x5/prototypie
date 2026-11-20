#!/usr/bin/env node
/**
 * Regression test for card previews (thumbnails) on the canvas.
 *
 *   node scripts/preview-regression.mjs --baseline [--space <id>]   # measure every ready card, store as baseline
 *   node scripts/preview-regression.mjs --compare  [--space <id>]   # measure again, list cards that changed; exit 1 on regressions
 *
 * Renders the real app page (one headless Chromium, cards one after another) and records per card:
 * box aspect ratio, crop (scale / translate / frozen iframe height), how far the lowest visible
 * img/svg/nav/footer/header or fixed/sticky element hangs over the card's bottom edge, whether the card
 * is (almost) one colour, and a small grey-scale signature of the picture (+ an optional PNG).
 *
 * Read-only towards the app: every non-GET request to /api/* is blocked, nothing is dragged, and only
 * the Shift key is pressed (to make React Flow's keyboard focus pan a card into view).
 * See scripts/preview-regression.md.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import zlib from "node:zlib";
import { createRequire } from "node:module";

// ---------- options ----------
const args = process.argv.slice(2);
const opt = (name, def) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && args[i + 1] && !args[i + 1].startsWith("--") ? args[i + 1] : def;
};
const flag = (name) => args.includes(`--${name}`);
const MODE = flag("baseline") ? "baseline" : flag("compare") ? "compare" : null;
if (!MODE || flag("help")) {
  console.log(`usage: node scripts/preview-regression.mjs --baseline|--compare [--space <id>] [--url http://localhost:3000]
       [--dir ~/.cache/prototypie-preview-baseline] [--no-png] [--settle 1600] [--limit N]
       [--reuse]  (with --compare: re-evaluate the last measurement, no browser)
env:   PLAYWRIGHT_CORE=<path to playwright-core>  CHROMIUM=<path to chrome-headless-shell>`);
  process.exit(MODE ? 0 : 2);
}
const APP = opt("url", "http://localhost:3000").replace(/\/$/, "");
const SPACE = opt("space", null);
const DIR = path.resolve(opt("dir", path.join(os.homedir(), ".cache", "prototypie-preview-baseline")).replace(/^~(?=\/)/, os.homedir()));
const SETTLE = Number(opt("settle", "1600")); // ms after thumb-ready (the app crops ~1200 ms after load)
const LIMIT = Number(opt("limit", "0")) || Infinity;
const SAVE_PNG = !flag("no-png");
const PNG_BUDGET = 20 * 1024 * 1024; // keep the PNG folder small (disk is tight)
const TH = { ratio: 0.05, overhang: 4, image: 0.08 }; // compare thresholds

// ---------- playwright-core + headless shell, no new dependency ----------
const require = createRequire(import.meta.url);
const PW_CANDIDATES = [
  process.env.PLAYWRIGHT_CORE,
  "playwright-core",
].filter(Boolean);
let chromium;
for (const c of PW_CANDIDATES) {
  try {
    ({ chromium } = require(c));
    break;
  } catch {
    /* next */
  }
}
if (!chromium) {
  console.error(`playwright-core not found. Set PLAYWRIGHT_CORE to its folder (tried: ${PW_CANDIDATES.join(", ")})`);
  process.exit(2);
}
const CHROMIUM =
  process.env.CHROMIUM ||
  path.join(os.homedir(), "Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell");

// ---------- minimal PNG decoder (8-bit RGB/RGBA, non-interlaced: what Chromium writes) ----------
function decodePng(buf) {
  let pos = 8, w = 0, h = 0, type = 0, depth = 0, interlace = 0;
  const idat = [];
  while (pos < buf.length) {
    const len = buf.readUInt32BE(pos), kind = buf.toString("ascii", pos + 4, pos + 8), data = buf.subarray(pos + 8, pos + 8 + len);
    if (kind === "IHDR") ({ 0: w, 1: h } = [data.readUInt32BE(0), data.readUInt32BE(4)]), (depth = data[8]), (type = data[9]), (interlace = data[12]);
    else if (kind === "IDAT") idat.push(data);
    else if (kind === "IEND") break;
    pos += 12 + len;
  }
  if (depth !== 8 || interlace || ![2, 6].includes(type)) throw new Error(`unsupported PNG (depth ${depth}, type ${type}, interlace ${interlace})`);
  const bpp = type === 6 ? 4 : 3, stride = w * bpp, raw = zlib.inflateSync(Buffer.concat(idat)), out = Buffer.alloc(h * stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)], src = y * (stride + 1) + 1, dst = y * stride;
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? out[dst + x - bpp] : 0, b = y ? out[dst - stride + x] : 0, c = x >= bpp && y ? out[dst - stride + x - bpp] : 0;
      let v = raw[src + x];
      if (f === 1) v += a;
      else if (f === 2) v += b;
      else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) {
        const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      out[dst + x] = v & 255;
    }
  }
  return { w, h, bpp, px: out };
}
/** Share of pixels close to the most common colour, and a 16×12 grey-scale signature (0..1). */
function analyse(png) {
  const { w, h, bpp, px } = decodePng(png);
  const counts = new Map();
  const step = Math.max(1, Math.floor((w * h) / 20000));
  let n = 0;
  for (let i = 0; i < w * h; i += step, n++) {
    const o = i * bpp, key = ((px[o] >> 4) << 8) | ((px[o + 1] >> 4) << 4) | (px[o + 2] >> 4);
    counts.set(key, (counts.get(key) || 0) + 1);
  }
  const uniform = Math.max(...counts.values()) / n;
  const SW = 16, SH = 12, sig = [];
  for (let sy = 0; sy < SH; sy++)
    for (let sx = 0; sx < SW; sx++) {
      let sum = 0, k = 0;
      for (let y = Math.floor((sy * h) / SH); y < Math.floor(((sy + 1) * h) / SH); y++)
        for (let x = Math.floor((sx * w) / SW); x < Math.floor(((sx + 1) * w) / SW); x++) {
          const o = (y * w + x) * bpp;
          sum += 0.299 * px[o] + 0.587 * px[o + 1] + 0.114 * px[o + 2];
          k++;
        }
      sig.push(k ? +(sum / k / 255).toFixed(3) : 0);
    }
  return { uniform: +uniform.toFixed(3), blank: uniform > 0.985, sig };
}
const sigDiff = (a, b) => (a && b && a.length === b.length ? a.reduce((s, v, i) => s + Math.abs(v - b[i]), 0) / a.length : 1);

// ---------- data ----------
async function readyDesigns() {
  const res = await fetch(`${APP}/api/state`);
  if (!res.ok) throw new Error(`GET /api/state → ${res.status}`);
  const { records } = await res.json();
  const spaces = {}, designs = [];
  for (const [k, r] of Object.entries(records)) {
    if (r?.deleted || !r?.value) continue;
    if (k.startsWith("spaces/")) spaces[r.value.id] = r.value;
    if (k.startsWith("designs/") && r.value.status === "ready" && r.value.html && (!SPACE || r.value.spaceId === SPACE))
      designs.push({ id: k.slice(8), title: r.value.title, spaceId: r.value.spaceId || "root" });
  }
  return { spaces, designs };
}

// ---------- measuring one card (runs in the page) ----------
function measureInPage(id) {
  const node = document.querySelector(`.react-flow__node[data-id="${CSS.escape(id)}"]`);
  if (!node) return { state: "missing" };
  const box = node.querySelector('[class*="thumb-"]');
  if (!box) return { state: "no-preview" };
  const state = [...box.classList].find((c) => c.startsWith("thumb-"))?.slice(6);
  const r = box.getBoundingClientRect();
  const zoom = box.offsetWidth ? r.width / box.offsetWidth : 1; // canvas zoom: report layout px, not screen px
  const frame = box.querySelector("iframe");
  const out = { state, box: { w: box.offsetWidth, h: box.offsetHeight }, ratio: box.offsetHeight ? +(box.offsetWidth / box.offsetHeight).toFixed(4) : null, rect: { x: r.x, y: r.y, w: r.width, h: r.height } };
  if (!frame) return out;
  const t = frame.style.transform || "";
  const num = (re) => (t.match(re) ? parseFloat(t.match(re)[1]) : null);
  out.crop = {
    cropped: /translate/.test(t),
    scale: num(/scale\(([-\d.e]+)\)/),
    tx: num(/translate\(([-\d.e]+)px/),
    ty: num(/translate\([^,]+,\s*([-\d.e]+)px/),
    frameHeight: frame.style.height || null, // px = frozen viewport height (crop.vh), % = follows the box
  };
  try {
    const doc = frame.contentDocument, win = frame.contentWindow;
    const fr = frame.getBoundingClientRect(), k = frame.offsetWidth ? fr.width / frame.offsetWidth : 1;
    let worst = 0, el = null;
    for (const e of doc.querySelectorAll("body *")) {
      const cs = win.getComputedStyle(e);
      const kind = /^(IMG|SVG|NAV|FOOTER|HEADER)$/i.test(e.tagName) || cs.position === "fixed" || cs.position === "sticky";
      if (!kind || cs.visibility === "hidden" || cs.display === "none" || +cs.opacity === 0) continue;
      if (e.closest("svg") && e.tagName.toLowerCase() !== "svg") continue;
      const er = e.getBoundingClientRect();
      if (er.width < 2 || er.height < 2 || er.top >= win.innerHeight || er.bottom <= 0) continue; // only what the frame shows
      const over = (fr.top + er.bottom * k - r.bottom) / zoom;
      if (over > worst) (worst = over), (el = `${e.tagName.toLowerCase()}${cs.position === "fixed" || cs.position === "sticky" ? `[${cs.position}]` : ""}${e.className && typeof e.className === "string" ? "." + e.className.trim().split(/\s+/)[0] : ""}`);
    }
    out.overhang = +worst.toFixed(1);
    out.overhangEl = el;
  } catch (e) {
    out.overhangError = String(e);
  }
  return out;
}

// ---------- run ----------
async function measureAll() {
  const { spaces, designs } = await readyDesigns();
  const bySpace = new Map();
  for (const d of designs.slice(0, LIMIT)) (bySpace.get(d.spaceId) || bySpace.set(d.spaceId, []).get(d.spaceId)).push(d);
  if (!designs.length) throw new Error(SPACE ? `no ready designs in space ${SPACE}` : "no ready designs");
  const browser = await chromium.launch({ headless: true, executablePath: fs.existsSync(CHROMIUM) ? CHROMIUM : undefined });
  const result = { meta: { app: APP, space: SPACE, viewport: { w: 1400, h: 900 }, at: new Date().toISOString(), settle: SETTLE }, cards: {} };
  try {
    const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 }, colorScheme: "dark" })).newPage();
    // Read-only: no writes to the app's API from this browser.
    await page.route(`${APP}/api/**`, (route) => (route.request().method() === "GET" ? route.continue() : route.abort()));
    for (const [spaceId, list] of bySpace) {
      const url = spaceId === "root" || !spaces[spaceId]?.parentSpaceId ? `${APP}/` : `${APP}/?space=${encodeURIComponent(spaceId)}`;
      await page.goto(url, { waitUntil: "domcontentloaded" });
      await page.waitForSelector(".react-flow__node", { timeout: 30000 });
      await page.waitForTimeout(1500); // fitView + hydration
      for (const d of list) {
        const started = Date.now();
        // Pan the card into view the way a keyboard user would: focus-visible focus on the node.
        await page.keyboard.press("Shift");
        await page.evaluate((id) => document.querySelector(`.react-flow__node[data-id="${CSS.escape(id)}"]`)?.focus(), d.id);
        let m = null;
        for (let i = 0; i < 40; i++) {
          m = await page.evaluate(measureInPage, d.id);
          if (m.state === "ready" || m.state === "missing") break;
          await page.waitForTimeout(250);
        }
        if (m.state === "ready") {
          await page.waitForTimeout(SETTLE);
          // wait until the crop has settled (two equal reads)
          for (let i = 0, prev = ""; i < 10; i++) {
            m = await page.evaluate(measureInPage, d.id);
            const key = JSON.stringify([m.crop, m.ratio]);
            if (key === prev) break;
            prev = key;
            await page.waitForTimeout(300);
          }
          if (m.rect.w >= 4 && m.rect.h >= 4) {
            const png = await page.screenshot({ clip: { x: m.rect.x, y: m.rect.y, width: m.rect.w, height: m.rect.h } });
            Object.assign(m, analyse(png));
            m.png = png; // written below, within the budget
          }
        }
        delete m.rect;
        result.cards[d.id] = { title: d.title, spaceId: d.spaceId, ms: Date.now() - started, ...m };
        process.stdout.write(`${m.state === "ready" ? "." : "!"}`);
      }
    }
  } finally {
    await browser.close();
  }
  process.stdout.write("\n");
  return result;
}

function writePngs(result, sub) {
  const dir = path.join(DIR, sub);
  fs.rmSync(dir, { recursive: true, force: true }); // older runs are overwritten
  if (!SAVE_PNG) return;
  fs.mkdirSync(dir, { recursive: true });
  let used = 0;
  for (const [id, c] of Object.entries(result.cards)) {
    if (!c.png) continue;
    if (used + c.png.length > PNG_BUDGET) continue;
    fs.writeFileSync(path.join(dir, `${id}.png`), c.png);
    used += c.png.length;
    c.pngFile = `${sub}/${id}.png`;
  }
}
const strip = (result) => {
  for (const c of Object.values(result.cards)) delete c.png;
  return result;
};

const fileFor = (kind) => path.join(DIR, `${kind}${SPACE ? "-" + SPACE : ""}.json`);
fs.mkdirSync(DIR, { recursive: true });
const t0 = Date.now();
const REUSE = MODE === "compare" && flag("reuse");
if (REUSE && !fs.existsSync(fileFor("compare"))) {
  console.error(`--reuse: no previous measurement at ${fileFor("compare")}`);
  process.exit(2);
}
const now = REUSE ? JSON.parse(fs.readFileSync(fileFor("compare"), "utf8")) : await measureAll();
const tag = SPACE ? `-${SPACE}` : "";
if (MODE === "baseline") {
  writePngs(now, `png-baseline${tag}`);
  fs.writeFileSync(fileFor("baseline"), JSON.stringify(strip(now), null, 1));
  const n = Object.keys(now.cards).length, notReady = Object.values(now.cards).filter((c) => c.state !== "ready").length;
  console.log(`baseline: ${n} cards (${notReady} not ready) → ${fileFor("baseline")} in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  process.exit(0);
}
// compare
if (!fs.existsSync(fileFor("baseline"))) {
  console.error(`no baseline at ${fileFor("baseline")}; run with --baseline first`);
  process.exit(2);
}
const base = JSON.parse(fs.readFileSync(fileFor("baseline"), "utf8"));
if (!REUSE) {
  writePngs(now, `png-compare${tag}`);
  fs.writeFileSync(fileFor("compare"), JSON.stringify(strip(now), null, 1));
}
const rows = [], info = [];
for (const [id, c] of Object.entries(now.cards)) {
  const b = base.cards[id];
  if (!b) {
    info.push(`new card ${id} (${c.title})`);
    continue;
  }
  const why = [];
  if (b.state === "ready" && c.state !== "ready") why.push(`state ${b.state}→${c.state}`);
  if (b.ratio && c.ratio && Math.abs(c.ratio / b.ratio - 1) > TH.ratio) why.push(`ratio ${(+b.ratio).toFixed(3)}→${(+c.ratio).toFixed(3)}`);
  if ((c.overhang ?? 0) - (b.overhang ?? 0) > TH.overhang) why.push(`overhang ${b.overhang ?? 0}→${c.overhang} px (${c.overhangEl})`);
  if (c.blank && !b.blank) why.push(`now blank (${c.uniform})`);
  const sd = sigDiff(b.sig, c.sig);
  if (b.sig && c.sig && sd > TH.image) why.push(`image Δ ${sd.toFixed(3)}`);
  if (why.length) rows.push([id, (c.title || "").slice(0, 40), why.join("; ")]);
}
for (const id of Object.keys(base.cards)) if (!now.cards[id]) info.push(`card gone ${id} (${base.cards[id].title})`);
const n = Object.keys(now.cards).length;
console.log(`compare: ${n} cards against baseline from ${base.meta.at} (${((Date.now() - t0) / 1000).toFixed(1)} s)`);
if (info.length) console.log(info.map((s) => `  info: ${s}`).join("\n"));
if (!rows.length) {
  console.log("no regressions");
  process.exit(0);
}
const w = [12, 40];
console.log(`\n${"card".padEnd(w[0])} | ${"title".padEnd(w[1])} | change\n${"-".repeat(w[0])}-+-${"-".repeat(w[1])}-+-------`);
for (const [id, t, why] of rows) console.log(`${id.padEnd(w[0])} | ${t.padEnd(w[1])} | ${why}`);
console.log(`\n${rows.length} regression(s)`);
process.exit(1);
