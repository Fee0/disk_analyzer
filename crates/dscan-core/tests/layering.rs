//! Enforces the core/GUI split described in CLAUDE.md:
//! core has no UI dependencies, and the GUI does no file-system or OS work.

use std::path::Path;

const CORE: &str = env!("CARGO_MANIFEST_DIR");

/// Crates that would tie core to a UI or an async runtime.
const UI_CRATES: &[&str] = &["iced", "winit", "wgpu", "rfd", "tokio", "mimalloc", "dscan-gui"];

/// APIs only core may use. GUI code goes through core's public API instead.
const CORE_ONLY_APIS: &[&str] = &["std::fs", "libc::", "objc2", "getattrlist", "statfs", "trash::"];

/// Dependency names declared in a Cargo.toml (any `*dependencies*` table).
fn dependencies(manifest: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_deps = line.contains("dependencies");
            continue;
        }
        if in_deps && let Some((key, _)) = line.split_once('=') {
            // `name = "1"`, `name = { … }` or `name.workspace = true`
            deps.push(key.split('.').next().unwrap().trim().to_string());
        }
    }
    deps
}

#[test]
fn core_has_no_ui_dependencies() {
    let manifest = std::fs::read_to_string(Path::new(CORE).join("Cargo.toml")).unwrap();
    let bad: Vec<String> = dependencies(&manifest)
        .into_iter()
        .filter(|d| UI_CRATES.iter().any(|ui| d == ui || d.starts_with(&format!("{ui}_"))))
        .collect();
    assert!(bad.is_empty(), "dscan-core must not depend on UI crates: {bad:?} (see CLAUDE.md)");
}

#[test]
fn gui_does_no_file_system_or_os_work() {
    let gui_src = Path::new(CORE).join("../dscan-gui/src");
    let mut violations = Vec::new();
    for entry in std::fs::read_dir(&gui_src).unwrap() {
        let path = entry.unwrap().path();
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        // Headless UI test scaffolding, not app code.
        if !file.ends_with(".rs") || file == "iced_inspect.rs" || file == "inspect_tests.rs" {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        // Unit tests at the end of a file may build fixtures on disk.
        let app_code = source.split("\n#[cfg(test)]\nmod tests").next().unwrap();
        for (n, line) in app_code.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if let Some(api) = CORE_ONLY_APIS.iter().find(|api| line.contains(*api)) {
                violations.push(format!("{file}:{}: uses `{api}`: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "GUI code must go through dscan-core's API (see CLAUDE.md):\n{}",
        violations.join("\n")
    );
}

/// Lines of the `*dependencies*` tables in a manifest.
fn dependency_lines(manifest: &str) -> Vec<&str> {
    let mut in_deps = false;
    let mut out = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_deps = line.contains("dependencies");
        } else if in_deps && line.contains('=') {
            out.push(line);
        }
    }
    out
}

#[test]
fn dependencies_are_declared_in_the_workspace_without_defaults() {
    let root = Path::new(CORE).join("../..");
    let mut problems = Vec::new();
    for member in ["crates/dscan-core", "crates/dscan-gui"] {
        let manifest = std::fs::read_to_string(root.join(member).join("Cargo.toml")).unwrap();
        for line in dependency_lines(&manifest) {
            if !line.contains(".workspace = true") || line.contains('{') {
                problems.push(format!("{member}: `{line}` should be `name.workspace = true`"));
            }
        }
    }
    let workspace = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let table = workspace.split("[workspace.dependencies]").nth(1).expect("no [workspace.dependencies]");
    let table = table.split("\n[").next().unwrap();
    for line in
        table.lines().map(str::trim).filter(|l| !l.starts_with('#') && (l.contains("version") || l.contains("path")))
    {
        if !line.contains("default-features = false") {
            problems.push(format!("workspace: `{line}` needs default-features = false"));
        }
    }
    assert!(problems.is_empty(), "dependency rules (see CLAUDE.md):\n{}", problems.join("\n"));
}

#[test]
fn parses_dependency_tables() {
    let manifest = "[package]\nname = \"x\"\n[dependencies]\nlibc.workspace = true\niced_core = \"0.14\"\nrfd = { workspace = true }\n[dev-dependencies]\ntempfile = \"3\"\n";
    assert_eq!(dependencies(manifest), ["libc", "iced_core", "rfd", "tempfile"]);
}
