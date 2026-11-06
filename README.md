# Disk Analyzer

A fast, local, DaisyDisk-style disk usage explorer for macOS (APFS), written in
Rust with an [iced](https://iced.rs) GUI. It has no licensing, telemetry or
network access.

## Run

```sh
cargo run --release -p dscan-gui              # start screen: volumes + "Open folder…"
cargo run --release -p dscan-gui -- ~/Movies  # scan a folder right away
```

For a whole-disk scan, give the terminal you launch from **Full Disk Access**
(System Settings → Privacy & Security → Full Disk Access) and restart it.
Without it, protected folders show as "no access" and their bytes end up in
*Hidden space*.

## Using it

| | |
|---|---|
| Hover | highlight a segment and its contents, with a tooltip showing size and % |
| Click / ←→ | select a segment, or a sibling with the arrow keys |
| ↑ / ↓ | select the parent / the first child |
| Double-click / Enter / ⌘↓ | open a folder (Quick Look for a file) |
| Click the center / ⌘↑ | go up one level |
| Backspace or ⌘[ / ⌘] | back / forward |
| Space | Quick Look the selection |
| ⌘F | search (matching items stay lit; everything else dims) |
| Esc | clear the search, then the selection |
| C, or drag onto the collector | collect items, then use **Move to Trash…** |

*On disk* shows allocated bytes, which is what deleting actually frees; hard
links count once. *Logical* shows file lengths, as Finder does. In a
whole-volume scan, two grey segments show *Purgeable space* and *Hidden space*:
container usage the file walk can't see, such as APFS snapshots, other
volumes' metadata and unreadable folders.

## Layout

```
crates/dscan-core   library, no GUI dependencies
  scan/bulk.rs      getattrlistbulk(2) directory reader (one syscall per ~hundreds of entries)
  scan/mod.rs       parallel walk (rayon), hard-link dedup, APFS firmlink/volume rules
  scan/volumes.rs   mounted volumes, container siblings, firmlinks, Full Disk Access probe
  tree.rs           compact breadth-first arena (≈40 B/node) with per-metric sort orders
  hidden.rs         container used / purgeable / hidden space
  layout.rs         sunburst geometry + O(log n) hit-testing
  search.rs         parallel case-insensitive name search
  actions.rs        Move to Trash + tree update
  examples/scan.rs  CLI for benchmarking: `cargo run -p dscan-core --release --example scan -- / --disk`
crates/dscan-gui    iced application
  app.rs            state, messages, update, subscriptions
  view.rs           start / scanning / browse screens
  sunburst.rs       canvas::Program for the chart
  theme.rs          colors by file kind
  quicklook.rs      qlmanage-based Quick Look
```

## Tests

```sh
cargo test --workspace
```

The core tests use real temporary file trees: hard links, sparse files,
symlinks and unreadable folders. The GUI tests drive the views headlessly with
`iced_test`: keyboard navigation, drag-to-collect and the metric toggle. They
also write screenshots to `target/iced-inspect/`.
