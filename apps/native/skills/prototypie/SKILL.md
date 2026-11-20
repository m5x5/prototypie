---
name: prototypie
description: Work with Prototypie, the local design-canvas app on this Mac, through its `prototypie` command. Watch for and handle the notes people pin on the app and on designs, take design jobs from Prototypie's agent queue and deliver HTML designs, and create design cards on its canvas. Use when the user mentions Prototypie, its notes or feedback, design jobs, cards or the canvas, or asks you to wait for notes.
---

# Prototypie

Prototypie is a native macOS app for designing with agents. While it runs, it serves a local API on
`127.0.0.1:3124`. You work with it only through the `prototypie` command; it prints readable text by default and
the exact data with `--json`.

## Before you start

1. Check that the app runs: `prototypie status`. Exit code 3 means it is not reachable: tell the user to start
   Prototypie, wait about 30 seconds, and try again at most three times. Do not loop on it.
2. Pick a name and use it for every command: `export PROTOTYPIE_AGENT=<name>` (or `--as <name>`). If you were given a
   worker name such as `proto-worker-3` or `proto-lead`, use it; otherwise a short name like `claude-shop`. The app
   shows this name next to the tasks you work on, and your replies are signed with it.
3. If the shell says `prototypie: command not found`, ask the user for the command shown in Prototypie's Agents panel
   (it contains the full path), or try `~/.local/bin/prototypie`.

## The loop: wait, take, work, close

```sh
prototypie notes watch
```

This returns at once with the notes that are already waiting, or waits until new ones arrive (up to about 110
seconds, collecting notes that arrive within 10 seconds of each other into one batch). The last line of the output
is the exact command to run next, for example:

```
Next: prototypie notes watch --after 58
```

Always continue with that command (or the `next` field with `--json`): the `--after` cursor makes sure you see each
note once and miss none. "No new notes" is a normal result (exit code 0): just run the next command again. If your
shell tool stops commands after a short time, pass a shorter wait, for example `--timeout 25s --batch 3s`.

For each note of a batch: decide whether it is for you (next section). If it is, take it on with
`prototypie notes ack <id>` before you start; exit code 5 means another agent was faster, so skip it. Leave notes
that are not for you untouched; the cursor keeps them from coming back to you. When the batch is done, run the next
watch command. Read a note in full with `prototypie notes show <id>`.

If every note is yours (you are the only agent), `prototypie notes watch --ack` takes all returned notes on at once.

## What kind of note is it?

| The note | How to recognise it | What to do |
|---|---|---|
| Design job | starts with `[prototypie job <jobId>#<index>]` (`tag.kind` is `job`) | Follow "Design jobs" below. Do not resolve the note yourself: delivering closes it. |
| Implement request | starts with `[prototypie implement <designId>]` | Implement that design in the codebase you are working in. The comment names the HTML file of the design. Resolve with a summary when done. |
| Project info | starts with `[prototypie project-info <spaceId>]` | Research the project named in the note (its repository path is the note's element path), create cards in that space with what you found, then resolve. Never copy secrets. |
| Feedback on the app | location starts with `prototypie-rune://` and there is no `designId` | Feedback on Prototypie's own interface. The comment ends with lines like ``— Rune element `review-dock` at crates/…/dock.rs:33`` that point to the source. Handle it only if you are working in Prototypie's own repository; otherwise leave it pending. |
| Feedback on a design | has a `designId` | Leave it pending. The user turns design notes into a design job ("Apply notes"), which then reaches you as a job. |
| Anything else | | A request from the user. Handle it like feedback in the codebase you are working in, or ask in the thread. |

## Talking to the user

- Take a note on: `prototypie notes ack <id>` (exit code 5 means another agent already took it: skip it). Design-job
  notes too: acknowledge them, then claim the job.
- Ask or report progress: `prototypie notes reply <id> "Which of the two headers do you mean?"`. Keep replies short.
  The user sees them in the app.
- Done: `prototypie notes resolve <id> --summary "Made the review button 20% smaller in dock.rs."` Resolve a note
  when it asked for a clear change and you made it; if the request was unclear or you are not sure the user wants
  the result, reply instead and leave the note open.
- Not doable or not wanted: `prototypie notes dismiss <id> --reason "The search field was removed last week."` A
  reason is required.
- Never resolve or dismiss a design-job note yourself; `prototypie queue deliver` closes it.

## Design jobs

A design job asks you for one design (one "direction") that the app shows as a card. Its note looks like
`[prototypie job J1abc#0] make it pop · … · rules: …`.

1. Read the job: `prototypie queue show J1abc#0` (all fields: `--json`). Important fields:
   - `instruction` and `direction` (`title`, `brief`): what to make. `focus` and `annotations`: what the user pointed at.
   - `scope`: `component` means only the discussed component, not a whole page; `page` means the whole page.
   - `context`: the space (`spaceName`, `spacePath`), its cards, the base design.
   - `baseHtml` / `baseTitle`: the design to start from. Get it raw with
     `prototypie queue show J1abc#0 --base-html > base.html`.
2. Claim it before you start: `prototypie queue claim J1abc#0`. Exit code 5: it is done or cancelled, stop.
3. Build the HTML, following the job's special fields:
   - `mode: edit` (EDIT IN PLACE): return `baseHtml` with only the requested change; everything else stays
     byte-identical. Delivering overwrites that card.
   - `mode: import` (IMPORT): recreate the named element faithfully from `importSource` (and the screenshot, if any).
     Read the source read-only and invent nothing.
   - `bases` (COMBINE): merge the listed designs into one.
   - `components` (REUSE COMPONENTS AS-IS): use these components' HTML unchanged.
   - `guidelines` (FOLLOW DESIGN GUIDELINES): follow the text exactly.
   - `podContext` (POD DATA): use these entries as the design's real data.
4. Deliver: `prototypie queue deliver J1abc#0 --title "Bolder price block" --rationale "Price first, larger, with the saving as a badge." --html design.html`
   (`--html -` reads stdin). This fills the placeholder card and closes the job's note.
   - Exit code 5 "the card … was deleted": the user removed it. Stop; do not recreate it unless the user asks.
   - Exit code 4 "no queue item … is waiting": the job was cancelled or already delivered. Stop.
   - Exit code 2 with a field problem: fix the field and deliver again.

## Cards without a job

- See the spaces and their cards: `prototypie cards list` and `prototypie cards list --space <id>`.
- Create a card: `prototypie cards create --space <spaceId> --title "Compact header" --html header.html`
  (or `--near <cardId>` to place it next to a card). Several cards at once: `--from cards.json` with
  `{"spaceId": "...", "cards": [{"title": "...", "html": "..."}]}`. A new exploration:
  `--new-exploration "Checkout ideas" --parent-space <spaceId>`.
- The main canvas (`root`) holds only projects: always pass a project's space or one of its explorations.

## Rules for design cards

These rules apply to every card, delivered or created.

- **One variant per card.** Asked for three options? Make three cards (or deliver three jobs), never three variants
  side by side on one card.
- **Only what was discussed.** With `scope: component`, design only that component, at its natural size, without the
  rest of the page around it.
- **No explanatory text in the design.** No captions like "Variant A", no labels such as "New:" or "Before/After",
  no notes about what you changed, no placeholder text that explains the design. Put your reasoning in the
  rationale (one or two sentences), never in the HTML.
- **Short titles.** 2 to 5 words, at most 40 characters, naming what is special about this design within its space,
  for example "Bolder price block" or "Kompakter Header".
- **Title rules.** No project or product name, no space name (the space already says it), no words like
  "Component"/"Komponente", "Variant"/"Variante", "Design", "Mockup" or "Version", no numbering, no quotes, no
  emoji, no trailing period.
- **Language.** Write the title, the rationale and any new visible text in the language of the request; if that is
  unclear, in the language of the space's existing cards. Keep the existing text of a base design in its language.
- **HTML.** A complete, self-contained document (`<!doctype html>`, inline CSS, inline SVG for icons). Set the card
  size with `<meta name="viewport" content="width=390">` (200 to 4000; add `height=…` for tall or fixed-height
  designs). Do not load scripts, fonts or images from the internet unless the job provides them; local paths under
  `/vendor/` and `/imports/` work.
- **No secrets.** Never put credentials, tokens, private keys or personal data from files or the environment into
  cards, rationales or replies.

## Errors

| Exit code | Meaning | What to do |
|---|---|---|
| 0 | OK (also: no new notes) | Continue. |
| 2 | Wrong use or bad input | Read the message; fix the command (`prototypie <group> <command> --help`). |
| 3 | Prototypie not reachable | Tell the user; wait about 30 s; retry at most three times. |
| 4 | Not found | The note, job, card or space is gone. Skip it. |
| 5 | Conflict | Someone else took it, or it is already done. Skip it. |
| 1 | Other server error | Report the message to the user; do not retry in a loop. |

Messages start with `prototypie:` and are written to stderr. With `--json`, errors are JSON on stderr.

## Examples

Feedback on the codebase:

```sh
prototypie notes watch
#   n7k2x9q0bz4  pending  2026-11-20 10:00Z  http://localhost:3124/
#     Button text on the pricing page is cut off on small screens
#   Next: prototypie notes watch --after 57
prototypie notes ack n7k2x9q0bz4 --message "On it: making the label wrap below 400 px."
# ... change the code, run the tests ...
prototypie notes resolve n7k2x9q0bz4 --summary "Label wraps now; checked at 320 px."
prototypie notes watch --after 57
```

A design job:

```sh
# the batch contained n1a2b3c4d5e: [prototypie job J1abc#0] make it pop · …
prototypie notes ack n1a2b3c4d5e
prototypie queue show J1abc#0
prototypie queue claim J1abc#0
prototypie queue show J1abc#0 --base-html > base.html
# ... write design.html ...
prototypie queue deliver J1abc#0 --title "Bolder price block" --rationale "Price first, larger." --html design.html
```

## Commands

```
prototypie status                                  app running? what is waiting?
prototypie notes watch [--ack] [--after C] [--timeout 100s] [--batch 10s] [--tag job|implement|project-info|none]
prototypie notes list [--status pending|acknowledged|resolved|dismissed|active|all] [--location URL] [--prefix URL]
prototypie notes show ID
prototypie notes ack ID... [--message TEXT] [--force]
prototypie notes reply ID TEXT | --file FILE|-
prototypie notes resolve ID [--summary TEXT]
prototypie notes dismiss ID --reason TEXT
prototypie queue list | show REF [--base-html] | claim REF
prototypie queue deliver REF --title T --html FILE|- [--rationale R] [--viewport N] [--viewport-height N]
prototypie cards list [--space ID]
prototypie cards create (--space ID | --near ID) --title T --html FILE|- [--rationale R] [--kind page|variation|spoke]
prototypie skill install | show
Options everywhere: --json, --as NAME (PROTOTYPIE_AGENT), --server URL (PROTOTYPIE_SERVER)
```

REF is a job and index as in the note, for example `J1abc#0`.
