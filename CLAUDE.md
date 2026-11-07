# Disk Analyzer

A DaisyDisk-style disk usage explorer for macOS (APFS), Linux and Windows:
Rust, with an iced 0.14 GUI. It is a personal tool: no licensing, no network, no caching (every launch
scans fresh). Performance and UX both matter.

## Supported platforms (enforced)

We support **macOS, Linux and Windows**, and everything must work on each of
them. No feature, button, shortcut, label or code path may assume one OS:

- Every user-visible feature works on all three, or is hidden where the OS has
  no equivalent (decided by core's platform layer, never by the GUI guessing).
- Wording follows the platform: ⌘ vs Ctrl, Trash vs Recycle Bin, Finder vs
  Explorer.
- Size accounting is right on each file system: hard links, clones/reflinks
  and snapshots are counted the way that OS stores them.
- A change is not done until it builds, passes clippy and passes tests for all
  three targets (see "After every change" below).

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
dependencies, and banned APIs in GUI sources), and the platform rules below. If it fails, move the code to the
right crate rather than loosening the test.

When adding a feature, first decide what is core (data, rules, I/O) and what is
GUI (how it looks and is driven), and put each part in its crate.

## Platform layer (enforced)

Everything that differs between operating systems lives in
`crates/dscan-core/src/platform/`, one folder per OS, behind the `Platform`
trait in `platform/mod.rs`. Each OS implements it once (`MacOs`, `Linux`,
`Windows`), and `platform::Current` picks the one for the build target.

Rules:

1. **OS code only in `platform/<os>/`.** Outside `src/platform/`, core code
   must not use `libc`, `objc2`, `windows_sys`, `std::os::unix`/`windows`,
   or `cfg(unix)`/`cfg(windows)`/`cfg(target_os …)` (unit-test modules may,
   for fixtures). Shared code calls `Current::…` through the trait.
2. **Every OS implements the whole trait.** Adding a capability means adding
   a trait item and implementing it for all three. Never stub one OS silently;
   if an OS has no equivalent (e.g. purgeable space off macOS), return the
   honest neutral value.
3. **Pure logic is tested on every host.** Parsing and rules that need no
   syscalls (mountinfo, Windows directory records, drive names, path syntax,
   protected-folder tables) go in their own modules that compile under
   `cfg(any(target_os = "<os>", test))`, so `cargo test` on any machine
   checks all three platforms. Only the modules that make syscalls are gated
   to their OS.
4. **Shared Unix helpers** (fds, `CString` paths, devices) go in
   `platform/unix.rs`, not copied into `macos/` and `linux/`.

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
  scan/mod.rs                  parallel walk (platform-neutral), progress, pause/cancel
  scan/shared_blocks.rs        ledger that counts blocks shared by clones/reflinks once
  platform/mod.rs              Platform trait, DirEntry, DeviceId, WholeDiskBoundaries
  platform/unix.rs             helpers shared by macOS and Linux
  platform/macos/              getattrlistbulk reader, APFS container/firmlinks, NSURL space
  platform/linux/              readdir+fstatat reader, /proc/self/mountinfo, statvfs
  platform/linux/extents.rs    FIEMAP layout: shared extents on btrfs and XFS
  platform/windows/            FileIdExtdDirectoryInfo reader, drive letters, verbatim paths
  platform/*/protected.rs      per-OS protected folders (matched by protection.rs)
  tree.rs                      arena tree (NodeId), sizes, remove(), sort orders
  layout.rs                    sunburst geometry + hit-testing
  volumes.rs, hidden.rs        volume list, hidden/purgeable split
  search.rs, actions.rs, protection.rs, kind.rs
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
- If you touched platform code or dependencies, also check the other targets
  (they need no linker; `rustup target add` them once):

  ```sh
  cargo clippy -p dscan-core --all-targets --target x86_64-unknown-linux-gnu -- -D warnings
  cargo clippy -p dscan-core --all-targets --target x86_64-pc-windows-msvc -- -D warnings
  ```

  and run the Linux tests for real in Docker (as a normal user, so that
  permission tests mean something):

  ```sh
  docker run --rm -u "$(id -u):$(id -g)" -v "$PWD":/work -w /work \
    -e CARGO_HOME=/work/target/docker-cargo -e CARGO_TARGET_DIR=/work/target/docker-linux \
    rust:1-bookworm cargo test --workspace
  ```
  Windows code can run its core tests under Wine 10 (x86 emulated, slow):

  ```sh
  docker build --platform linux/amd64 -t dscan-wine -f ci/windows-wine.Dockerfile ci
  docker run --rm --platform linux/amd64 -v "$PWD":/work -w /work \
    -e CARGO_HOME=/work/target/docker-cargo-wine -e CARGO_TARGET_DIR=/work/target/docker-wine \
    dscan-wine sh -c 'wineboot -i; cargo test -p dscan-core --target x86_64-pc-windows-gnu'
  ```

  Without Docker, WSL 2 does the same job (run as your normal user). The
  shared-block path (reflinks) is only exercised when the temp dir is on btrfs
  or XFS: when touching it, mount a loop image of each, owned by your user, and
  run `TMPDIR=/mnt/<fs> cargo test -p dscan-core` on both. The test prints
  which case it checked.

  Wine is not Windows: it lacks `FileIdExtdDirectoryInfo`, so the reader
  takes its fallback and the two hard-link tests fail there. Every other test
  must pass; real NTFS behaviour still needs a Windows machine.
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

## No comments in code

Do not write comments in code: no `//`, no `///` doc comments, no `/* */`.
Make the code say it instead:

- Name things for what they mean: variables, functions, types, constants,
  enum variants (`Refusal::Protected`, `may_share_blocks`), not `flag`, `tmp`.
- Pull a step that needs explaining into a small, well-named function.
- Replace magic numbers and offsets with named constants.
- Use types to carry meaning (`Option` for "may be absent", enums for states)
  instead of documenting conventions like "0 means unknown".
- Put the why of a test into its name and its assertion messages.

If a fact truly cannot be expressed in code (e.g. an OS quirk), encode it in
a test whose name states it, rather than a comment.

The one exception is `crates/dscan-gui/src/iced_inspect.rs`, a helper copied
verbatim from the iced-ui-inspect skill: keep it as shipped so it can be
updated from the skill.

## Conventions

- For UI changes, check the result with the headless Simulator tests in
  `inspect_tests.rs` (they write PNGs to `target/iced-inspect/`), not only by
  reasoning.
- Tests must not touch the real Trash or anything outside a temp dir.
- Performance-sensitive paths: the scan walk, `tree::build`, search and layout.
  Measure with `examples/scan.rs` before and after changing them, and read
  `docs/scan-performance.md` first: the whole-disk scan is bound by APFS
  metadata I/O, and many obvious tweaks were measured there without effect.
