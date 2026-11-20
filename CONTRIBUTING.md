# Contributing

Thanks for helping. Please open an issue before a large change so we can agree on the direction.

## Layout

- `apps/native`: the macOS app (Rust workspace). Read `apps/native/README.md` and `apps/native/docs/PLAN.md`.
- `apps/web`: the web app (Next.js). It has its own `AGENTS.md`: this Next.js version differs from older ones, read
  `node_modules/next/dist/docs/` before changing framework code.

## Native

```sh
cd apps/native
cargo test -p proto-core && cargo test -p proto-server && cargo test -p proto-app
cargo clippy --workspace --all-targets      # a handful of style warnings exist today; please don't add new ones
```

The code is formatted by hand with long lines (up to about 150 columns); follow the style of the file you edit.
`cargo fmt` is not used, it would reformat everything.

Look at a change with the headless snapshot (`cargo run -p proto-app -- --snap out.png --scene light`) before you
send it. Changes to Rune itself belong upstream (it does not take pull requests); a needed local patch goes in
`vendor/rune-patched/` and is described in `docs/rune-patches.md`.

## Web

```sh
cd apps/web
pnpm install
pnpm lint
pnpm build
```

## Pull requests

- One topic per pull request, with a short description of what and why.
- Keep personal data out: no absolute paths of your machine, hostnames, tokens or exported designs. Examples and
  tests use `/Users/example` and `my-pod`.
- New third-party code or assets: add their licence to `NOTICE.md`.

By contributing you agree that your contribution is licensed under the MIT licence of this repository.
