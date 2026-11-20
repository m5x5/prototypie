# Local Rune patches

Rune (`anomalyco/rune`) doesn't take pull requests. When prototypie needs something Rune's public
API lacks, the change lives in a minimal local copy, applied with `[patch]` in the root
`Cargo.toml`, and an issue goes upstream. Drop the patch once upstream has an equivalent and the pin
moves past it.

## Layout

`vendor/rune-patched/` is a copy of `anomalyco/rune` at the pinned rev
`4a2e437954f01eaacc13c0515a1d39474dd6512c`, holding only:

- `crates/rune`: the patched crate.
- Its path siblings `crates/rune-motion`, `crates/rune-staging`, `crates/rune-taffy`,
  `crates/procwatch` and `vendor/tiny-skia-path`. They are patched too, so `rune-kit` (still from
  git) and `rune` share one copy of each. A second copy would make their types mismatch.
- The few files `rune`'s sources `include_bytes!`/`include_str!` from outside the crate: the
  `rune-fx` and `rune-light` shaders, and the IBM Plex Mono font under `themes/opencode`.
- A small workspace `Cargo.toml` for `edition`, `license` and lints inheritance. Upstream warnings
  are allowed there, so builds stay quiet. The root workspace has `exclude = ["vendor"]`;
  otherwise cargo makes the copies members of our workspace.

All files are byte-identical to the pinned rev except where listed below.

### Moving the pin

1. Re-copy the same paths from the new rev, for example from
   `~/.cargo/git/checkouts/rune-*/<rev>`.
2. Re-apply the patches below and bump the rev in the header comment.
3. Run `cargo check -p proto-app`.

## Patches

### 1. `Pixels::with_rgba` (2026-11-19, proto-worker-5, for the perf audit)

`crates/rune/src/image/pixels.rs`, after `size()`:

```rust
pub fn with_rgba<R>(&self, f: impl FnOnce(&[u8], u32, u32) -> R) -> R {
    let frame = self.frame.lock().unwrap();
    f(&frame.rgba, frame.width, frame.height)
}
```

**Why.** Card pictures are rendered at the zoom's resolution and never lowered. After a deep zoom,
zooming far out keeps every visible card at full resolution: about 7 MB each, with many cards in
view. The fix shrinks the pictures already held:

1. proto-web's `Snapshots::reduce` copies the bytes out with `with_rgba` on the main thread.
2. It box-filters them on its worker thread.
3. It replaces them in place with `Pixels::set`.

Every holder shares the frame, so the big buffer is freed, with no visible change and no WebKit
re-render. Without this API the bytes can't be read: `Frame` owns its `Vec<u8>`, `rgba()` is
`pub(crate)`, and no public constructor shares a caller-held buffer. Keeping a second copy in
proto-web would double the memory being saved.

**Draft upstream issue** (for the user to file):

> **Public read access to `Pixels`' RGBA (e.g. `Pixels::with_rgba`)**
>
> `Pixels::set` takes an owned `Vec<u8>`, but there is no public way to read the image back:
> `Pixels::rgba()` is `pub(crate)`, and no constructor wraps a caller-owned shared buffer. A
> producer that wants to derive a smaller copy of an image it already handed to Rune therefore has
> two options: keep a second full-size copy (doubling memory) or regenerate it from the source.
> Our case is thumbnails rendered at zoom resolution, reduced to a box-filtered smaller copy in
> place (`set`) once the canvas rests zoomed out.
>
> Proposal: `pub fn with_rgba<R>(&self, f: impl FnOnce(&[u8], u32, u32) -> R) -> R`, which runs
> `f(rgba, width, height)` under the frame lock. The bytes are empty for decoded images with mips.
> A closure keeps the lock scoped and avoids a copy for callers that only read. We carry exactly
> this as a local patch at 4a2e437.

### 2. Quiet vendored warnings (2026-11-19)

`vendor/tiny-skia-path/src/lib.rs` gets `#![allow(warnings)]` after its own lint attributes. As a
git dependency cargo capped its lints, but as a path dependency it printed 4 warnings on every
build. The other crates get the same through `[workspace.lints.rust] warnings = "allow"` in
`vendor/rune-patched/Cargo.toml`. Nothing to file upstream.

