# Disk Analyzer

A DaisyDisk-style disk usage explorer in Rust with an iced 0.14 GUI, for
macOS, Linux and Windows. A personal tool: no licensing, no network, no
caching (every launch scans fresh). Performance and UX both matter.

## Platforms (enforced)

Everything works on macOS, Linux and Windows. No feature, label, shortcut or
code path may assume one OS:

- A feature works on all three, or is hidden where the OS has no equivalent,
  as decided by core's platform layer, never guessed by the GUI.
- Wording follows the platform: ⌘ vs Ctrl, Trash vs Recycle Bin, Finder vs
  Explorer.
- Sizes are counted the way each file system stores them: hard links,
  clones/reflinks and snapshots.

## Architecture (enforced)

Two crates:

- `dscan-core`: everything except presentation. Scanning, the result tree,
  sizes, hidden and purgeable space, sunburst geometry and hit-testing,
  search, collecting, the Trash, preview and reveal.
- `dscan-gui`: the iced app. It maps core state to widgets and input, and
  holds only UI state (focus, selection, history, open results, drag).

Rules:

1. **Core never depends on the GUI**: no iced, winit, wgpu, rfd, tokio or
   other UI or async-runtime crate. Its API is plain Rust types and functions.
2. **The GUI uses core's public API only**, and never duplicates core logic.
3. **No file-system or OS work in the GUI**: no `std::fs`, `libc`, `objc2` or
   syscalls outside tests. Add a function to core instead. Exceptions: the
   folder dialog (`rfd`) and `open`-ing the System Settings URL.
4. **Computation lives in core** (layout, hit-testing, tree updates, search,
   size accounting), so it is unit-tested without a window. The GUI only turns
   core's results into pixels.
5. **Blocking work runs off the UI thread**, through `off_thread` (tokio
   `spawn_blocking`) or a background thread, never in `update`/`view`.

When adding a feature, first decide which part is core (data, rules, I/O) and
which is GUI (how it looks and is driven).

### Platform layer

Everything that differs between operating systems lives in
`dscan-core/src/platform/<os>/` behind the `Platform` trait in
`platform/mod.rs`. `platform::Current` picks the build target's implementation.

1. **OS code only in `platform/`.** Elsewhere in core: no `libc`, `objc2`,
   `windows_sys`, `std::os::*`, or `cfg(unix|windows|target_os)` (unit-test
   fixtures excepted). Shared code calls `Current::…`.
2. **Every OS implements the whole trait.** A new capability is a new trait
   item, implemented for all three. Where an OS has no equivalent, return the
   honest neutral value (e.g. no purgeable space off macOS); never stub silently.
3. **Pure logic is tested on every host.** Parsing and rules that need no
   syscalls go in modules compiled under `cfg(any(target_os = "<os>", test))`,
   so `cargo test` on any machine checks all three. Only syscall modules are
   gated to their OS.
4. **Shared Unix helpers** go in `platform/unix.rs`, not copied per OS.

`dscan-core/tests/layering.rs` checks these rules and the dependency rule
below. If it fails, move the code; don't loosen the test.

### Dependencies

Declare every dependency once, in the root `Cargo.toml` under
`[workspace.dependencies]`, with `default-features = false` and only the
features in use. Member crates write `name.workspace = true` and nothing else.
Check what a feature pulls in before enabling it.

## After every change (required)

Before reporting work as done or committing, run in order; all must pass:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Fix what they report. No `#[allow(...)]`, no skipped or `#[ignore]`d tests,
no loosening `layering.rs`. If a lint is truly wrong somewhere, say why.

If you touched platform code or dependencies, also:

- Clippy core for all three targets, from any host (no linker needed;
  `rustup target add` once):

  ```sh
  cargo clippy -p dscan-core --all-targets --target aarch64-apple-darwin -- -D warnings
  cargo clippy -p dscan-core --all-targets --target x86_64-unknown-linux-gnu -- -D warnings
  cargo clippy -p dscan-core --all-targets --target x86_64-pc-windows-msvc -- -D warnings
  ```

- Run the Linux tests for real, as a normal user so permission tests mean
  something: in Docker, or in WSL 2 when Docker isn't running.

  ```sh
  docker run --rm -u "$(id -u):$(id -g)" -v "$PWD":/work -w /work \
    -e CARGO_HOME=/work/target/docker-cargo -e CARGO_TARGET_DIR=/work/target/docker-linux \
    rust:1-bookworm cargo test --workspace
  ```

  Reflink changes also need a run with `TMPDIR` on btrfs and on XFS, or the
  shared-block path is never exercised (the test prints which case it ran).

- Optionally, Windows core tests run under Wine: see `ci/windows-wine.Dockerfile`.

If you touched `Cargo.toml` or features, also test each crate alone
(`cargo test -p dscan-core`, `cargo test -p dscan-gui`): the workspace build
can hide a feature that only the other crate enables.

In your report, say which checks ran and their results, and which could not.

## Commands

```sh
cargo run --release                                            # the app
cargo run -p dscan-core --release --example scan -- / --disk   # benchmark a whole-disk scan
cargo build --profile profiling                                # release with symbols, for profilers
```

## No comments in code

No `//`, `///` or `/* */`. Make the code say it:

- Name things for what they mean (`Refusal::Protected`, `may_share_blocks`),
  not `flag` or `tmp`.
- Pull a step that needs explaining into a small, well-named function.
- Replace magic numbers with named constants.
- Let types carry meaning (`Option`, enums) instead of conventions like
  "0 means unknown".
- Put a test's why into its name and assertion messages. An OS quirk that
  code can't express becomes a test named after it.

Exception: `dscan-gui/src/iced_inspect.rs` is copied verbatim from the
iced-ui-inspect skill; keep it as shipped.

## Conventions

- Check UI changes with the headless tests in `inspect_tests.rs`, which write
  PNGs to `target/iced-inspect/`; look at them rather than only reasoning.
- Tests stay inside a temp dir: never the real Trash, and never launching
  other apps (assert that preview or reveal *messages* are sent; don't run them).
- Performance-sensitive paths are the scan walk, `tree::build`, search and
  layout. Read `docs/scan-performance.md` first, and measure with
  `examples/scan.rs` before and after: the whole-disk scan is bound by APFS
  metadata I/O, and many obvious tweaks were measured there without effect.
