use std::time::Instant;

use dscan_core::{CancelToken, Metric, Progress, ScanOptions, format_size, scan};

const USAGE: &str =
    "usage: cargo run -p dscan-core --release --example scan -- <path> [--disk] [--threads N] [--logical]";

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut disk = false;
    let mut threads = None;
    let mut metric = Metric::Allocated;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--disk" => disk = true,
            "--logical" => metric = Metric::Logical,
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
        format_size(tree.size(root, metric)),
        tree.size(root, metric),
        tree.stats.files,
        tree.stats.dirs,
        tree.len(),
        tree.stats.errors,
        scan_time
    );
    let t = Instant::now();
    let l = dscan_core::sunburst(&tree, root, metric, dscan_core::LayoutParams::default(), None);
    println!("layout: {} arcs in {:.2?}", l.len(), t.elapsed());
    for q in ["readme", "readme", "a", "zzqx"] {
        let t = Instant::now();
        let r = dscan_core::search(&tree, q, &CancelToken::default());
        println!("search {q:?}: {} matches in {:.2?}", r.map_or(0, |r| r.matches), t.elapsed());
    }
    if disk {
        println!("hidden {}  purgeable {}", format_size(tree.stats.hidden), format_size(tree.stats.purgeable));
    }
    for c in tree.children(root, metric).take(15) {
        use std::io::Write;
        if writeln!(std::io::stdout(), "  {:>10}  {}", format_size(tree.size(c, metric)), tree.name(c)).is_err() {
            break;
        }
    }
}
