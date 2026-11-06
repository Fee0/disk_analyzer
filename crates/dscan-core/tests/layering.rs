use std::path::Path;

const CORE: &str = env!("CARGO_MANIFEST_DIR");

const UI_OR_ASYNC_RUNTIME_CRATES: &[&str] = &["iced", "winit", "wgpu", "rfd", "tokio", "mimalloc", "dscan-gui"];

const CORE_ONLY_APIS: &[&str] = &["std::fs", "libc::", "objc2", "getattrlist", "statfs", "trash::"];

const OS_SPECIFIC_APIS: &[&str] = &[
    "libc::",
    "objc2",
    "windows_sys",
    "std::os::unix",
    "std::os::windows",
    "target_os",
    "cfg(unix)",
    "cfg(windows)",
    "trash::macos",
];
const PLATFORM_LAYER: &str = "platform";

const HEADLESS_UI_TEST_SCAFFOLDING: &[&str] = &["iced_inspect.rs", "inspect_tests.rs"];
const UNIT_TEST_MODULE_START: &str = "\n#[cfg(test)]\nmod tests";

fn dependency_names(manifest: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_deps = line.contains("dependencies");
            continue;
        }
        if in_deps && let Some((key, _)) = line.split_once('=') {
            let name_before_dotted_workspace_key = key.split('.').next().unwrap();
            deps.push(name_before_dotted_workspace_key.trim().to_string());
        }
    }
    deps
}

#[test]
fn core_has_no_ui_dependencies() {
    let manifest = std::fs::read_to_string(Path::new(CORE).join("Cargo.toml")).unwrap();
    let bad: Vec<String> = dependency_names(&manifest)
        .into_iter()
        .filter(|d| UI_OR_ASYNC_RUNTIME_CRATES.iter().any(|ui| d == ui || d.starts_with(&format!("{ui}_"))))
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
        if !file.ends_with(".rs") || HEADLESS_UI_TEST_SCAFFOLDING.contains(&file.as_str()) {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        let app_code_before_unit_tests_that_may_build_fixtures_on_disk = code_before_unit_tests(&source);
        for (n, line) in app_code_before_unit_tests_that_may_build_fixtures_on_disk.lines().enumerate() {
            let is_comment_line = line.trim_start().starts_with("//");
            if is_comment_line {
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

fn rust_files_below(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(rust_files_below(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files
}

fn code_before_unit_tests(source: &str) -> &str {
    source.split(UNIT_TEST_MODULE_START).next().unwrap()
}

#[test]
fn os_specific_code_lives_only_in_the_platform_layer() {
    let core_src = Path::new(CORE).join("src");
    let mut violations = Vec::new();
    for path in rust_files_below(&core_src) {
        let relative = path.strip_prefix(&core_src).unwrap();
        if relative.starts_with(PLATFORM_LAYER) {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        for (n, line) in code_before_unit_tests(&source).lines().enumerate() {
            if let Some(api) = OS_SPECIFIC_APIS.iter().find(|api| line.contains(*api)) {
                violations.push(format!("{}:{}: uses `{api}`: {}", relative.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "OS-specific code belongs in dscan-core/src/platform/<os>/ behind the Platform trait (see CLAUDE.md):\n{}",
        violations.join("\n")
    );
}

#[test]
fn every_platform_folder_implements_the_platform_trait() {
    let platform = Path::new(CORE).join("src").join(PLATFORM_LAYER);
    for os in ["macos", "linux"] {
        let sources: String =
            rust_files_below(&platform.join(os)).iter().map(|f| std::fs::read_to_string(f).unwrap()).collect();
        assert!(sources.contains("impl Platform for"), "platform/{os} must implement the Platform trait");
    }
}

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
fn parses_dependency_names_from_versioned_inline_table_and_dotted_workspace_entries() {
    let manifest = "[package]\nname = \"x\"\n[dependencies]\nlibc.workspace = true\niced_core = \"0.14\"\nrfd = { workspace = true }\n[dev-dependencies]\ntempfile = \"3\"\n";
    assert_eq!(dependency_names(manifest), ["libc", "iced_core", "rfd", "tempfile"]);
}
