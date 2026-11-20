# Preview regression test

Checks card previews (canvas thumbnails) before a change to the preview logic (`src/components/design-preview.tsx`, card sizing) goes live: measure all ready cards with the current code (`--baseline`), change the code, measure again (`--compare`).

```sh
node scripts/preview-regression.mjs --baseline --space 5Ga2qjGn   # before the change
# … change the preview code, let the dev server reload …
node scripts/preview-regression.mjs --compare  --space 5Ga2qjGn   # exit 1 if a card regressed
node scripts/preview-regression.mjs --compare --reuse --space 5Ga2qjGn   # re-evaluate the last measurement, no browser
```

Without `--space`, every ready design in every space is measured. Further options: `--url` (default `http://localhost:3000`), `--dir` (default `~/.cache/prototypie-preview-baseline`), `--no-png`, `--settle <ms>` (wait after a card is `thumb-ready`, default 1600; the app crops 1200 ms after load), `--limit <n>`.

## What it does

- Reads all `ready` designs from `GET /api/state`, opens the real app page per space (`/?space=<id>`) in **one** headless Chromium (1400×900) and brings the cards into view one after another (keyboard focus on the React Flow node, which pans it into view; no dragging).
- Read-only: every non-GET request to `/api/*` from that browser is blocked, so a run never changes the shared state.
- Per card: box aspect ratio (layout px), crop (`scale`, `translate`, iframe height; a px height is the frozen viewport `crop.vh`), how far the lowest visible `img/svg/nav/footer/header` or `fixed/sticky` element hangs over the card's bottom edge, a blank check (share of the most common colour > 98.5 %), and a 16×12 grey-scale signature of the card picture. PNGs of the card boxes go to `<dir>/png-baseline-…` or `png-compare-…` (overwritten per run, at most 20 MB).
- `--compare` reports cards with: aspect ratio change > 5 %, overhang grown by > 4 px, now blank, a card that was ready and isn't anymore, or a picture difference (mean signature Δ > 0.08). New or removed cards are listed as info. Exit code 1 on regressions, 2 on setup errors.

Large overhangs in a baseline are normal for long pages (e.g. a 24-hour sticky time column); the test looks at *changes*.

## Dependencies (none added)

- `playwright-core`: resolved from `PLAYWRIGHT_CORE`, then a local `playwright-core`.
- Browser: `CHROMIUM`, default `~/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell`.
- PNG decoding uses `node:zlib` only.

Runtime: about 2.6 s per card (64 cards in space 5Ga2qjGn ≈ 165 s).
