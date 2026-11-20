# Third-party notices

Prototypie is MIT licensed (see [LICENSE](LICENSE)). It builds on the following; each keeps its own licence.

## Rune

- UI framework of the native app: <https://github.com/anomalyco/rune>, pinned at revision
  `4a2e437954f01eaacc13c0515a1d39474dd6512c`. MIT licence, Copyright (c) 2026 Anomaly.
- `apps/native/vendor/rune-patched/` is a copy of that revision with one local patch (`Pixels::with_rgba`), described
  in `apps/native/docs/rune-patches.md`. Its licence file is kept at `apps/native/vendor/rune-patched/LICENSE`.
- Its path siblings and vendored crates keep their own licences in their directories (for example
  `vendor/tiny-skia-path`).

## Fonts

- **Google Sans Flex**: SIL Open Font License 1.1. The licence text is `apps/native/crates/proto-app/assets/fonts/OFL.txt`
  and ships next to the font in the app bundle (`Resources/fonts`).
- **IBM Plex Mono** (bundled with Rune's `opencode` theme): SIL Open Font License 1.1, see Rune's repository.

## Icons

- **Lucide** icons (ISC licence): <https://lucide.dev>. Used throughout both apps.

## Dependencies

Rust crates and npm packages keep their own licences; see `apps/native/Cargo.lock` and `apps/web/pnpm-lock.yaml`.
Check the licence of any new dependency before adding it.

## Annotation feedback

- The native app's annotate mode, notes service, `prototypie` command and agent skill are original work, written to a
  functional specification without access to the source code of any third-party annotation toolbar (a clean-room
  process). They contain no code from the [Agentation](https://github.com/benjitaylor/agentation) package.
- The web app (`apps/web`) still lists the npm package `agentation` as an optional dependency for its feedback toolbar.
  That package is a separate project under the PolyForm Shield 1.0.0 licence, is installed from npm, and is not
  included in or modified by this repository. Check its terms before using it.
