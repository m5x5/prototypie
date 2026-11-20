# Prototypie (native)

The native macOS app of [Prototypie](../../README.md): an infinite canvas of AI-generated UI design variations,
grouped into projects and explorations. It is feature-equivalent to the web app in [`apps/web`](../web), backend
included, and runs on its own data.

> **Built on Rune.** The UI is drawn with [Rune](https://github.com/anomalyco/rune) (Rust, GPU-drawn, MIT licence,
> Copyright (c) 2026 Anomaly). Prototypie pins Rune at one git revision and carries a small local patch
> (`Pixels::with_rgba`) in `vendor/rune-patched/`, a copy of the pinned revision with its licence kept. Rune does not
> take pull requests, so patches live here and are reported upstream as issues; see
> [`docs/rune-patches.md`](docs/rune-patches.md) for what changed and how to move the pin. Rune and its dependencies
> keep their own licences, listed in the repository's [`NOTICE.md`](../../NOTICE.md).

The plan and ownership are in `docs/PLAN.md`; the crate APIs are in `docs/api-*.md`.

| Crate | What it is |
|---|---|
| `proto-core` | Types, the record store (`state.json` + change log), queries, write helpers, reference import |
| `proto-web` | WKWebView rendering: offscreen design snapshots and live interactive web views |
| `proto-ai` | Generation (Codex/OpenAI provider), the agent queue, jobs |
| `proto-server` | The embedded HTTP server on `127.0.0.1:3124` (state API, agent queue, polls, assets) |
| `proto-app` | The app (binary `prototypie`): canvas, cards, inspector, composer, features |

## Build and run

The toolchain is pinned to Rust 1.99.1 (`rust-toolchain.toml`). Use rustup's cargo, because Homebrew's older cargo
otherwise comes first on PATH:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo run -p proto-app                 # debug build, opens the window
cargo run -p proto-app --release       # optimised
```

On start the app loads the store on a background thread and starts the embedded server on `127.0.0.1:3124`. Designs
load their assets (`/vendor/…`) from that server.

## Bundle as a Mac app

```sh
scripts/bundle-mac.sh              # cargo build --release (with agent tooling) → dist/Prototypie.app
scripts/bundle-mac.sh --no-build   # re-package the existing target/release/prototypie
scripts/bundle-mac.sh --lean       # without agent tooling: --snap/--tree/--script are refused, ~3 MB smaller
```

`dist/` is gitignored. The bundle has:
- bundle id `dev.prototypie.app`
- display name Prototypie, category Developer Tools, macOS 13+
- the app icon from `assets/icon.png` (set `PROTOTYPIE_ICON=<image>` to use another)
- Google Sans Flex and its OFL licence under `Resources/fonts`. The UI font is also compiled into the binary.

It is signed **ad hoc** (`MAC_APP_SIGN=<identity>` to use a real identity). It isn't notarised, so on another Mac
Gatekeeper asks for a right-click → Open. The script never installs anything: to use the app, drag it into
`/Applications` (or `~/Applications`) yourself, or `open dist/Prototypie.app`.

## Headless checks (no window)

Builds with agent tooling (debug, or release with `--features tooling`, which includes the bundle) render without
opening a window:

```sh
cargo run -p proto-app -- --snap out.png --scene light --size 1280x800   # a PNG of a scene
cargo run -p proto-app -- --tree out.txt --scene nested                  # the layout/accessibility tree
cargo run -p proto-app -- --snap /dev/null --scene ?                     # lists the scenes
dist/Prototypie.app/Contents/MacOS/prototypie --snap out.png --scene projects   # same, from the bundle
```

Headless runs read the user's data but never change it. They work on an in-memory copy, so `Store::data_dir()` is
`None` and nothing writes files. `--script` and the other tooling flags come from Rune; see its `crates/rune/README.md`.

## Test

```sh
cargo test -p proto-core       # one crate at a time: the shared target/ is locked per build
cargo test -p proto-server
cargo test -p proto-app
cargo clippy -p <crate>
PROTOTYPIE_REFERENCE_DATA=../web/data cargo test -p proto-core -- --ignored   # also round-trips a web-app state.json (read only)
```

## Data

Everything lives in one data dir. It is `$PROTOTYPIE_DATA` if that's set, else `~/Library/Application Support/Prototypie`:

| Path | What |
|---|---|
| `state.json` | All records (`{epoch, version, records{"<kind>/<id>": {rev, value}}}`). It's the same format as the web app's `data/state.json`. It's written atomically, debounced by 300 ms |
| `state-log.ndjson` | Append-only change log (who, what, before) |
| `prefs.json` | Window/UI preferences |
| `public/vendor`, `public/imports` | Static assets designs load, served at `/` by the embedded server |
| `agent-queue/`, `polls/`, … | Files of the server and AI crates |

**Import from the web app (optional).** If `PROTOTYPIE_REFERENCE_DATA` names the web app's `data/` directory (for
example `../web/data`) and the native data dir has no `state.json`, the app copies that `state.json` into it once. On
every start it also copies any of the web app's `public/vendor` and `public/imports` files that are missing. The web
app's files are only ever read. Without the variable nothing is imported. To start over from the web app's state, quit
the app and move the data dir's `state.json` away.

## Configuration

Project sources are optional and nothing is assumed. Put them in `config.json` in the data dir (an environment variable
of the same meaning wins):

```json
{
  "projectsTtl": "~/projects.ttl",
  "projectsRoot": "~/code",
  "podProjects": "https://pod.example/me/projects/index.ttl"
}
```

- `projectsTtl` (`PROTOTYPIE_PROJECTS_TTL`): a Turtle file of `schema:WebSite` projects (name, repository, keywords) for
  "+ → Project". Default: `projects.ttl` in the data dir.
- `projectsRoot` (`PROTOTYPIE_PROJECTS_ROOT`): a folder; its sub-folders are listed as projects too.
- `podProjects` (`PROTOTYPIE_POD_PROJECTS`): a Solid pod's project register, read like the Turtle file. Off by default.

Without any of them "+ → Project" still starts a project from a name or an absolute folder path.

## Connecting an agent

Generation can run through the Codex login or through your own coding agent. The agents watch Prototypie's notes
service (the `prototypie notes` command) and deliver through the app's HTTP API on `:3124`. The agent-tasks panel (header, next to "+") has a "Prompt kopieren" button that copies the prompt to hand to any
coding agent.

## Environment variables

| Variable | Default | Effect |
|---|---|---|
| `PROTOTYPIE_DATA` | `~/Library/Application Support/Prototypie` | The data dir |
| `PROTOTYPIE_ADDR` | `127.0.0.1:3124` | Where the embedded server listens |
| `PROTOTYPIE_SERVER` | `127.0.0.1:3124` | Where the app's features call the server (set it with `PROTOTYPIE_ADDR`) |
| `PROTOTYPIE_ASSET_BASE` | `http://127.0.0.1:3124/` | Base URL web views load design html against |
| `PROTOTYPIE_MODEL` | provider default | Model for generation |
| `CODEX_HOME` | `~/.codex` | Where the Codex login is read |
| `PROTOTYPIE_PROJECTS_TTL` | `projects.ttl` in the data dir | Project list (local paths, pod links) |
| `PROTOTYPIE_PROJECTS_ROOT` | – | Folder whose sub-folders are projects |
| `PROTOTYPIE_POD_PROJECTS` | – | A Solid pod's project register |
| `PROTOTYPIE_CONFIG` | `config.json` in the data dir | Another configuration file |
| `PROTOTYPIE_REFERENCE_DATA` | – | The web app's `data/` directory, for the one-time import |
| `PROTOTYPIE_LAN_IP` | auto-detected | LAN address shown in poll links |
| `PROTOTYPIE_ICON` | `assets/icon.png` | Icon source for `scripts/bundle-mac.sh` |
| `MAC_APP_SIGN` | `-` (ad hoc) | Codesign identity for `scripts/bundle-mac.sh` |
| `PROTOTYPIE_LAUNCH_LOG=1` | off | Prints launch timing marks |
| `PROTOTYPIE_TRACE_POLL=1`, `PROTO_WEB_TRACE=1` | off | Debug traces (poll wakeups, web view slots) |
| `PROTOTYPIE_TASKS_FIXTURE` | – | Agent-tasks JSON file to use instead of the server (headless snapshots) |

While the web app runs on `:3123`, this app uses `:3124`. Run test servers on `:3126+`.
