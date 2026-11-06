# Disk Analyzer

A DaisyDisk-style disk usage explorer for macOS/APFS: Rust, with an iced 0.14
GUI. It is a personal tool: no licensing, no network, no caching (every launch
scans fresh). Performance and UX both matter.

## Core / GUI split (enforced)

The workspace has two crates, and the boundary between them is the main
architectural rule.

- `crates/dscan-core`: everything that is not presentation. Scanning
  (`getattrlistbulk`, APFS volumes and firmlinks), the result tree, sizes,
  hidden and purgeable space, sunburst geometry and hit-testing, search,
  collecting and moving to the Trash.
- `crates/dscan-gui`: the iced app. It maps core state to widgets, colors and
  input, and holds UI state only (focus, selection, history, open results,
  drag state).

Rules:

1. **Core never depends on the GUI.** `dscan-core` must not depend on iced,
   winit, wgpu, rfd, tokio or any other UI or async-runtime crate, and must not
   know a GUI exists. Its API is plain Rust types and functions.
2. **The GUI goes through core's public API only.** Use what `dscan-core/src/lib.rs`
   re-exports (or `pub` items of its modules). Don't reach into core internals,
   and don't duplicate core logic in the GUI.
3. **No file-system or OS work in the GUI.** No `std::fs`, `libc`, `objc2` or
   syscalls in non-test GUI code. If the GUI needs a fact about the disk or a
   file operation, add a function to core. The only exceptions are UI-level
   system calls: the native folder dialog (`rfd`) and `open`-ing a System
   Settings URL.
4. **Computation lives in core.** Layout math, hit-testing, tree updates,
   search and size accounting belong in core, so they are unit-tested without
   a window. The GUI may only convert core's results to pixels (e.g. fractions
   of a turn to angles).
5. **Blocking work runs off the UI thread.** Scans, searches, volume listing and
   freeing large trees go through `off_thread` (tokio `spawn_blocking`) or a
   background thread, never directly in `update`/`view`.

`crates/dscan-core/tests/layering.rs` checks rules 1 and 3 mechanically (core's
dependencies, and banned APIs in GUI sources). If it fails, move the code to the
right crate rather than loosening the test.

When adding a feature, first decide what is core (data, rules, I/O) and what is
GUI (how it looks and is driven), and put each part in its crate.

## Dependencies

All dependencies are declared once, in the root `Cargo.toml` under
`[workspace.dependencies]`, each with `default-features = false` and only the
features the code needs. Member crates list them as `name.workspace = true`,
with no versions or features of their own. To add one, put it in the root
first. Before enabling a feature, check what it pulls in: most defaults are
Linux-only or unused here. `layering.rs` checks this too.

## Layout

```
crates/dscan-core/src/
  scan/{mod,bulk,volumes}.rs   parallel walk, getattrlistbulk, APFS volumes/firmlinks
  tree.rs                      arena tree (NodeId), sizes, remove(), sort orders
  layout.rs                    sunburst geometry + hit-testing
  search.rs, actions.rs, hidden.rs, kind.rs
  examples/scan.rs             CLI for benchmarking
crates/dscan-gui/src/
  app.rs      state, Message, update, subscriptions
  view.rs     start / scanning / browse screens
  sunburst.rs canvas::Program drawing the chart
  theme.rs, icon.rs, fmt.rs
  iced_inspect.rs, inspect_tests.rs   headless UI tests (iced_test)
```

## After every change (required)

Run all three, in this order, after any code change and before reporting
the work as done or committing. All three must pass.

```sh
cargo fmt --all                                         # formatting (rustfmt.toml: 120 columns)
cargo clippy --workspace --all-targets -- -D warnings   # no warnings allowed
cargo test --workspace                                  # unit, layering and headless UI tests
```

- Fix what they report; don't silence it. Don't add `#[allow(...)]`, skip or
  `#[ignore]` tests, or loosen `layering.rs` to get green. If a lint is truly
  wrong for a spot, say so and explain why.
- If you touched `Cargo.toml` or features, also build each crate alone
  (`cargo test -p dscan-core`, `cargo test -p dscan-gui`): the workspace build
  can hide a missing feature that another crate happens to enable.
- In your report, say which of these ran and their result. If one could not
  run, say that instead of implying it passed.

## Commands

```sh
cargo run --release                                     # the app
cargo run -p dscan-core --release --example scan -- / --disk   # benchmark a whole-disk scan
```

## Conventions

- For UI changes, check the result with the headless Simulator tests in
  `inspect_tests.rs` (they write PNGs to `target/iced-inspect/`), not only by
  reasoning.
- Tests must not touch the real Trash or anything outside a temp dir.
- Performance-sensitive paths: the scan walk, `tree::build`, search and layout.
  Measure with `examples/scan.rs` before and after changing them.
