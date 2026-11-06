# Disk Analyzer

A fast, local, DaisyDisk-style disk usage explorer for macOS (APFS), Linux and
Windows, written in Rust with an [iced](https://iced.rs) GUI. It has no licensing, telemetry or
network access.

## Run

```sh
cargo run --release -p dscan-gui              # start screen: volumes + "Open folder…"
cargo run --release -p dscan-gui -- ~/Movies  # scan a folder right away
```

On macOS, for a whole-disk scan, give the terminal you launch from **Full Disk
Access** (System Settings → Privacy & Security → Full Disk Access) and restart
it. Without it, protected folders show as "no access" and their bytes end up in
*Hidden space*. On Linux and Windows, folders your user can't read are counted
the same way; run as root/administrator to see everything.

| | macOS | Linux | Windows |
|---|---|---|---|
| Directory reader | `getattrlistbulk` (hundreds of entries per call) | `readdir` + `fstatat` | `GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)`, 128-bit file ids |
| Whole-disk scan | all volumes of the APFS container, firmlinks once | the root file system and its other subvolumes (btrfs), bind mounts once | one drive; junctions and symlinks are not followed |
| Hard links counted once | ✓ | ✓ | ✓ (by file id; NTFS, ReFS) |
| Clones counted once | ✓ (APFS) | – (reflinks count in full, like `du`) | – |
| Purgeable space | ✓ | – | – |
| Remove | Trash | Trash (freedesktop) | Recycle Bin |
| Tested | natively | in Docker (arm64), all tests | pure logic on every host; core tests under Wine; not yet on a real Windows machine |

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

*On disk* shows allocated bytes, which is what deleting actually frees; hard
links count once. *Logical* shows file lengths, as Finder does. In a
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
