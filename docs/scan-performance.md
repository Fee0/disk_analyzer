# Whole-disk scan performance (macOS)

Measured on 2026-11-06 on an M1 Pro (10 cores, 32 GB RAM), macOS 26.6, with
`cargo run -p dscan-core --release --example scan -- / --disk`: 1.7 TB,
about 15.6 M files and 2.65 M folders. Warm runs, one after another. Run to
run spread is about ±5 % (95–106 s), so differences below that are noise.

## Where the time goes

| Phase | Time | Notes |
|---|---|---|
| Walk | ~95 s | 160–180 k files/s, steady from start to end (no long tail) |
| Tree build | ~3 s | single-threaded; sorting is a small part on this tree |
| CPU | 13 s user, ~500 s system | the work is in the kernel, not in our code |
| Disk | 30–60 k reads/s of 4 KB, ~13 GB per scan | APFS metadata does not stay cached between scans |

The scan is bound by APFS metadata I/O: each file costs an inode lookup, and
those reads are random. More threads only add kernel lock contention (system
time grows from 300 s at 10 threads to 615 s at 128).

## Experiments

| Change | Result | Drawback | Verdict |
|---|---|---|---|
| 10 threads (default is 4 × cores = 40) | 132 s, +32 % | | keep 40 |
| 20 threads | 133 s, +33 % | | keep 40 |
| 32 / 48 / 64 / 128 threads | 106 / 103 / 102 / 99 s, noise | more system time; +100 errors at 128 | keep 40 |
| `openat` relative to the parent folder instead of full paths | 99 s vs 100 s, noise | one open descriptor per active folder level | rejected |
| No clone attributes (`CLONEID`, `EXT_FLAGS`) | 109 s, noise | clones would count twice | rejected |
| Data fork sizes only (`DATALENGTH`, `DATAALLOCSIZE`) | 91–94 s vs 93 s, noise | resource forks uncounted | rejected |
| `getattrlistbulk` buffer 16 / 64 KB instead of 256 KB | 95.8 / 97.4 s vs 97.8 s (interleaved A/B), noise | | keep 256 KB |
| Scan threads at user-initiated QoS | 104 s, noise | | rejected |
| Visit subfolders in inode order | 97–106 s, noise | | rejected |
| Sort children during the walk instead of in the build | build 3.3 → 3.0 s | +200 MB peak memory | rejected |
| `searchfs` catalog enumeration instead of walking | 391 s, 3.9× slower | single-threaded, no parallelism possible | rejected |
| Skip the sealed System volume | ≤ 3 % (459 k of 18 M inodes) | `/System`, `/usr` no longer browsable | not worth it |

## What would help

Only avoiding the metadata reads helps: caching the previous scan and
re-reading only changed folders (FSEvents). The project deliberately does no
caching. Showing partial results while the scan runs would make it feel faster
without changing the total.

## Anomaly seen while measuring

Three of about 35 runs found 12.07 M files instead of 15.6 M. The missing
3.55 M files are `~/Library/Containers/com.apple.CoreDevice.CoreDeviceService`.
macOS denies access to another app's container from time to time, and the scan
then reports that one folder as unreadable, as intended.
