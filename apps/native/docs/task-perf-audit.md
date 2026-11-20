# Task: performance audit (least resources, snappy, no UX compromise)

User: "How good is the performance of the app? Please check it closer. I really want to make sure it uses the
least possible amount of resources while still being snappy and have no compromises on the outstanding ux."

Lead: proto-worker-7. Fixes go to the owning worker (see PLAN.md); the lead tracks them in docs/perf.md.

## Allowed measurement setup
Real windows are allowed for this audit only as background launches (`open -g`, never activated), using a
dedicated bundle id, a temp copy of the data dir, port 3127, and quitting afterwards (AppleScript Quit). Windows may
stay open as long as a scenario needs (minutes). Never the user's data dir, :3124 or their running app. Drive
input with Rune's --script where possible; otherwise measure passive scenarios. Delete artifacts afterwards (disk is
tight: keep at most one state copy).

## Budgets (release build, M-series Mac, 120 Hz display)
| Area | Budget |
|---|---|
| Launch | window visible < 300 ms warm; projects drawn < 500 ms; first thumbnails < 1 s after the first frame |
| Interaction frames | p95 < 8.3 ms CPU (120 Hz) and no dropped frames for pan, pinch, zoom inertia, card drag, marquee, edge drag, fullscreen morph, inspector scroll, composer typing, menus opening, review J/K stepping, compare scroll |
| Input latency | key/click to visible change within 1 frame (no view rebuild > 4 ms on the hot path) |
| Idle | 0 frames drawn and ~0% CPU after 2 s at rest, including with generating runs present (dash animation only while visible), notes service connected, agent-tasks pill open/closed |
| Memory | RSS plateaus while panning the biggest project and switching all projects (bounded thumbnail maps + proto-web LRU + WKWebView pool); no growth across 10 fullscreen open/close cycles |
| GPU | no per-frame texture re-uploads at rest; atlas and thumbnail texture memory bounded; backdrop blurs ≤ Rune's per-frame limit |
| Energy | Activity Monitor "Energy Impact" ≈0 at rest; background WebKit content processes idle (no timers/animations running in hidden web views); hidden live views torn down or suspended |
| Disk/IO | no file I/O on the UI thread; store saves coalesced; no repeated writes at rest (check fs_usage or file mtimes over 60 s) |
| Network | no polling faster than needed: agent-tasks, notes service and SSE intervals justified and paused when the window is hidden/occluded |

## Method
1. Build the scenarios as Rune scripts on the real (copied) store: main canvas, the biggest project (~142 designs), fullscreen stepping, review, compare, composer typing, menus.
2. Measure: RUNE_FRAME_LOG (frame CPU, commits and their `why`), PROTOTYPIE_TRACE_POLL, `ps`/`top`/`footprint` RSS
   over time, `xctrace` Time Profiler / Allocations / Energy Log on background windows where it works, WebKit
   process count and CPU (`ps -ax | grep WebContent`).
3. Write docs/perf.md: a table per budget with measured value, pass/fail, root cause, owner, fix and re-measure.
4. Window occlusion: when the window is hidden, minimised or occluded, stop all nonessential work (polls, thumbnails,
   animations). Check Rune's occlusion/visibility events.
5. Re-measure after the fixes, and report the before/after table to the lead.

No UX compromises: don't fix a budget by removing an animation, lowering quality the user can see, or delaying
something the user waits for. Make the work cheaper, smarter (visibility, coalescing, caching), or move it off the
critical path.
