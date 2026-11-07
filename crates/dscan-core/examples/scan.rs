use std::time::Instant;

use dscan_core::{CancelToken, Progress, ScanOptions, format_size, scan};

const LAYOUT_RUNS: usize = 25;

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
    let l = layout();
    let mut layout_times = Vec::with_capacity(LAYOUT_RUNS);
    for _ in 0..LAYOUT_RUNS {
        let t = Instant::now();
        std::hint::black_box(layout());
        layout_times.push(t.elapsed());
    }
    layout_times.sort();
    println!(
        "layout: {} arcs, fastest {:.2?}, median {:.2?} of {LAYOUT_RUNS} runs",
        l.len(),
        layout_times[0],
        layout_times[LAYOUT_RUNS / 2]
    );
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
