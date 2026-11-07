<img src="assets/icon/icon-128.png" width="96" align="right" alt="">

# Disk Analyzer

[![CI](https://github.com/Fee0/disk_analyzer/actions/workflows/ci.yml/badge.svg)](https://github.com/Fee0/disk_analyzer/actions/workflows/ci.yml)

A fast, local, DaisyDisk-style disk usage explorer for macOS (APFS), Linux and
Windows, written in Rust with an [iced](https://iced.rs) GUI. It has no licensing, telemetry or
network access.

## Install

Download the file for your system from the
[latest release](https://github.com/Fee0/disk_analyzer/releases/latest). The builds
are not signed, so each system asks once before the first launch.

**macOS** (11 or later, Apple silicon and Intel): open the `.dmg` and drag
*Disk Analyzer* into *Applications*. The first time you open it, macOS says it
can't check the app for malware: click *Done*, then go to System Settings →
Privacy & Security, scroll down and click **Open Anyway**. Or, in Terminal:

```sh
xattr -dr com.apple.quarantine "/Applications/Disk Analyzer.app"
```

**Windows** (10 or later, x64): unzip and run `disk-analyzer.exe`. If SmartScreen
says it protected your PC, click **More info → Run anyway**.

**Linux** (x86_64 or aarch64, glibc 2.35 or later, X11 or Wayland): unpack the
`.tar.gz` and run `./install.sh`. It puts the app in `~/.local/bin` and adds it to
your application menu. Or just run `./disk-analyzer` from the folder. The
*Open folder…* dialog needs `xdg-desktop-portal`, which desktop environments
normally include.

### Permissions

On macOS, for a whole-disk scan, give *Disk Analyzer* **Full Disk Access**
(System Settings → Privacy & Security → Full Disk Access) and restart it. When
you run it with `cargo run`, give the access to your terminal instead. Without it,
protected folders show as "no access" and their bytes end up in *Hidden space*.
On Linux and Windows, folders your user can't read are counted the same way; run
as root/administrator to see everything.

## Build from source

With [Rust](https://rustup.rs) 1.90 or later:

```sh
cargo run --release -p dscan-gui              # start screen: volumes + "Open folder…"
cargo run --release -p dscan-gui -- ~/Movies  # scan a folder right away
```

Pushing a tag such as `v0.2.0` (matching the version in `Cargo.toml`) builds
every platform and drafts a GitHub release; see
`.github/workflows/release.yml`.

| | macOS | Linux | Windows |
|---|---|---|---|
| Directory reader | `getattrlistbulk` (hundreds of entries per call) | `readdir` + `fstatat` | `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)`, 128-bit file ids |
| Whole-disk scan | all volumes of the APFS container, firmlinks once | the root file system and its other subvolumes (btrfs), bind mounts once | one drive; junctions and symlinks are not followed |
| Hard links counted once | ✓ | ✓ | ✓ (by file id; NTFS, ReFS) |
| Clones counted once | ✓ (APFS) | – (reflinks count in full, like `du`) | – |
| Purgeable space | ✓ | – | – |
| Remove | Trash | Trash (freedesktop) | Recycle Bin |
| Tested | natively, and in CI | in Docker (arm64) and in CI, all tests | natively on Windows 10, and in CI |

## Using it

| | |
|---|---|
| Hover | highlight a segment and its contents, with a tooltip showing size and % |
| Click / ←→ | select a segment, or a sibling with the arrow keys |
| ↑ / ↓ | select the parent / the first child |
| Double-click / Enter / ⌘↓ | open a folder |
| Click the center / ⌘↑ | go up one level |
| Mouse back/forward buttons, Backspace or ⌘[ / ⌘] | back / forward through visited folders |
| ⌘F | search (matching items stay lit; everything else dims) |
| Esc | clear the search, then the selection |
| C, or drag onto the collector | collect items, then **Move to Trash…** (Recycle Bin on Windows), or hold **Hold 3 s to delete permanently** for 3 seconds to delete them right away |

⌘ is Ctrl on Linux and Windows.

Sizes are always on disk: allocated bytes, which is what deleting actually
frees. Hard links count once, and so do blocks shared between copies: unchanged APFS
clones, and reflinked copies on btrfs and XFS (which `cp` makes by default). In a
whole-volume scan, grey segments show *Purgeable space* (macOS) and *Hidden
space*: usage the file walk can't see, such as snapshots, file system metadata
and unreadable folders.

## Layout

```
crates/dscan-core   library, no GUI dependencies
  scan/mod.rs       parallel walk (rayon), hard-link and clone dedup, whole-disk boundaries
  platform/         everything OS-specific, behind one `Platform` trait
    macos/          getattrlistbulk reader, APFS container + firmlinks, NSURL purgeable space
    linux/          readdir+fstatat reader, /proc/self/mountinfo volumes, statvfs
    windows/        FileIdExtdDirectoryInfo reader, drive letters, verbatim (long) paths
    */protected.rs  folders the collector refuses, per OS
  tree.rs           compact breadth-first arena (≈40 B/node) with per-metric sort orders
  volumes.rs        start-screen volume list
  hidden.rs         used / purgeable / hidden space
  layout.rs         sunburst geometry + O(log n) hit-testing
  search.rs         parallel case-insensitive name search
  actions.rs        collector, Move to Trash / delete permanently + tree update
  examples/scan.rs  CLI for benchmarking: `cargo run -p dscan-core --release --example scan -- / --disk`
crates/dscan-gui    iced application
  app.rs            state, messages, update, subscriptions
  view.rs           start / scanning / browse screens
  sunburst.rs       canvas::Program for the chart
  theme.rs          colors by file kind
  build.rs          embeds the icon and version info in the Windows executable
assets/icon         app icon in every format; `python assets/icon/generate.py` (Pillow) redraws them
packaging/          macOS Info.plist, Linux desktop entry and installer, used by the release workflow
.github/workflows   CI on all three systems; release builds on a `v*` tag
```

## Tests

```sh
cargo test --workspace
```

The core tests use real temporary file trees: hard links, sparse files,
symlinks and unreadable folders. Platform logic that needs no syscalls (mount
tables, Windows directory records, drive names, protected folders) is tested
for all three OSes on any host. See CLAUDE.md for the Linux (Docker) and
cross-target checks. The GUI tests drive the views headlessly with
`iced_test`: keyboard navigation, drag-to-collect and the metric toggle. They
also write screenshots to `target/iced-inspect/`.
