# Prototypie

An infinite canvas of AI-generated UI design variations. You describe a screen or a component, and Prototypie
produces several designs side by side. Vary them, explore a direction in a nested space, promote the best ones, compare
them, put them to a poll and hand the winner to a coding agent to build.

This is a monorepo with two implementations of the same app:

| Path | What | Stack |
|---|---|---|
| [`apps/native`](apps/native) | The native macOS app | Rust, [Rune](https://github.com/anomalyco/rune), WKWebView |
| [`apps/web`](apps/web) | The web app | Next.js, React, Tailwind |

Both keep designs in a `state.json` of the same format, so work can move between them (see
[`apps/native/README.md`](apps/native/README.md#data)).

> **Built on Rune.** The native app draws its UI with [Rune](https://github.com/anomalyco/rune) (Rust, GPU-drawn, MIT
> licence, Copyright (c) 2026 Anomaly). It pins one Rune revision and carries a small local patch in
> `apps/native/vendor/rune-patched/`. Rune does not take pull requests, so changes to it live there and are reported
> upstream as issues. Details and the licence texts: [`apps/native/docs/rune-patches.md`](apps/native/docs/rune-patches.md)
> and [`NOTICE.md`](NOTICE.md).

## Quick start

**Native app (macOS 13+).** Install the pinned Rust toolchain through [rustup](https://rustup.rs) (it picks it up from
`rust-toolchain.toml`), then:

```sh
cd apps/native
cargo run -p proto-app --release        # opens the window
scripts/bundle-mac.sh                   # builds dist/Prototypie.app (ad hoc signed)
```

**Web app.** Node 20+ and [pnpm](https://pnpm.io):

```sh
cd apps/web
pnpm install
cp .env.example .env                    # optional: ANTHROPIC_API_KEY for generation through the Anthropic API
pnpm dev                                # http://localhost:3000
```

## Generating designs

Designs are generated either through a model provider or by **your own coding agent** (Claude Code, Codex, Cursor, …).
For the agent route, the native app runs its own notes service on `127.0.0.1:3124` and ships a `prototypie` command and an
agent skill: install the skill with `prototypie skill install`, then let your agent run `prototypie notes watch` to pick up
annotations and `prototypie queue` / `prototypie cards create` to deliver designs. The agent-tasks panel shows the two
commands with a copy button. Nothing is sent anywhere unless you configure a provider or an agent.

## Your data stays yours

Everything is stored locally: `~/Library/Application Support/Prototypie` (native) or `apps/web/data` (web, git-ignored).
No project, pod or host is assumed; see the configuration section of the native README for optional sources.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security issues: [SECURITY.md](SECURITY.md). Changes: [CHANGELOG.md](CHANGELOG.md).

## Licence

[MIT](LICENSE). Third-party licences (Rune, fonts, icons) are in [NOTICE.md](NOTICE.md).
