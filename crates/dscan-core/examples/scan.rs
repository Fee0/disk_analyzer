use std::time::{Duration, Instant};

use dscan_core::{CancelToken, Progress, ScanOptions, format_size, scan};

const LAYOUT_RUNS: usize = 25;
const TYPICAL_CHART_SIZE: (f32, f32) = (740.0, 640.0);

const USAGE: &str = "usage: cargo run -p dscan-core --release --example scan -- <path> [--disk] [--threads N]";

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut disk = false;
    let mut threads = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--disk" => disk = true,
            "--threads" => threads = args.next().and_then(|t| t.parse().ok()),
            "--help" => {
                println!("{USAGE}");
                return;
            }
            _ => path = Some(a),
        }
    }
    let path = path.unwrap_or_else(|| ".".into());
    let mut opts = if disk { ScanOptions::volume(&path, "Disk") } else { ScanOptions::folder(&path) };
    opts.threads = threads;
    let scan_start = Instant::now();
    let tree = scan(&opts, &Progress::default(), &CancelToken::default()).expect("scan failed");
    let scan_time = scan_start.elapsed();
    let root = tree.root();
    println!(
        "{}: {} ({} bytes) in {} files, {} dirs, {} nodes, {} errors — {:.2?}",
        path,
        format_size(tree.size(root)),
        tree.size(root),
        tree.stats.files,
        tree.stats.dirs,
        tree.len(),
        tree.stats.errors,
        scan_time
    );
    let exclusions = dscan_core::Exclusions::default();
    let layout = || dscan_core::sunburst(&tree, root, dscan_core::LayoutParams::default(), None, &exclusions);
    let (arcs, fastest, median) = fastest_and_median(layout);
    println!("layout: {} arcs, fastest {fastest:.2?}, median {median:.2?} of {LAYOUT_RUNS} runs", arcs.len());
    let treemap = || dscan_core::treemap(&tree, root, TYPICAL_CHART_SIZE, None, &exclusions);
    let (map, fastest, median) = fastest_and_median(treemap);
    println!("treemap: {} tiles, fastest {fastest:.2?}, median {median:.2?} of {LAYOUT_RUNS} runs", map.tiles.len());
    for q in ["readme", "readme", "a", "zzqx"] {
        let t = Instant::now();
        let r = dscan_core::search(&tree, q, &CancelToken::default());
        println!("search {q:?}: {} matches in {:.2?}", r.map_or(0, |r| r.matches), t.elapsed());
    }
    if disk {
        println!("hidden {}  purgeable {}", format_size(tree.stats.hidden), format_size(tree.stats.purgeable));
    }
    for c in tree.children(root).take(15) {
        use std::io::Write;
        if writeln!(std::io::stdout(), "  {:>10}  {}", format_size(tree.size(c)), tree.name(c)).is_err() {
            break;
        }
    }
}

fn fastest_and_median<T>(run: impl Fn() -> T) -> (T, Duration, Duration) {
    let timed = || {
        let t = Instant::now();
        let result = std::hint::black_box(run());
        (result, t.elapsed())
    };
    let (result, first) = timed();
    let mut times: Vec<Duration> = std::iter::once(first).chain((1..LAYOUT_RUNS).map(|_| timed().1)).collect();
    times.sort();
    (result, times[0], times[LAYOUT_RUNS / 2])
}
