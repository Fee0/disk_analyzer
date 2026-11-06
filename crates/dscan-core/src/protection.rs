use std::path::Path;

use crate::platform::{Current, Platform};

const ANY_ONE_COMPONENT: &str = "*";

pub(crate) struct PathSyntax {
    pub components: fn(&str) -> Vec<&str>,
    pub same_name: fn(&str, &str) -> bool,
}

pub(crate) struct ProtectedLocations {
    pub syntax: PathSyntax,
    pub exact: &'static [&'static str],
    pub trees: &'static [&'static str],
    pub user_trees_inside_system_trees: &'static [&'static str],
}

impl ProtectedLocations {
    pub fn protects(&self, path: &str) -> bool {
        let path = (self.syntax.components)(path);
        let matches_any = |patterns: &[&str], whole: bool| {
            patterns.iter().any(|pattern| self.pattern_matches(&(self.syntax.components)(pattern), &path, whole))
        };
        matches_any(self.exact, true)
            || (matches_any(self.trees, false) && !matches_any(self.user_trees_inside_system_trees, false))
    }

    fn pattern_matches(&self, pattern: &[&str], path: &[&str], whole_path: bool) -> bool {
        let long_enough = if whole_path { path.len() == pattern.len() } else { path.len() >= pattern.len() };
        long_enough
            && pattern
                .iter()
                .zip(path)
                .all(|(wanted, actual)| *wanted == ANY_ONE_COMPONENT || (self.syntax.same_name)(wanted, actual))
    }
}

pub fn is_protected(path: &Path) -> bool {
    Current::PROTECTED.protects(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::unix_syntax::UNIX_PATHS;
    use crate::platform::windows::paths::WINDOWS_PATHS;

    const SAMPLE: ProtectedLocations = ProtectedLocations {
        syntax: UNIX_PATHS,
        exact: &["/", "/home/*", "/home/*/Documents"],
        trees: &["/usr"],
        user_trees_inside_system_trees: &["/usr/local"],
    };

    #[test]
    fn a_star_matches_exactly_one_component_and_exact_patterns_do_not_cover_their_contents() {
        assert!(SAMPLE.protects("/"));
        assert!(SAMPLE.protects("/home/alex"));
        assert!(SAMPLE.protects("/home/alex/"), "a trailing separator names the same folder");
        assert!(SAMPLE.protects("/home/alex/Documents"));
        assert!(!SAMPLE.protects("/home/alex/Documents/notes.txt"));
        assert!(!SAMPLE.protects("/home/alex/code"));
        assert!(!SAMPLE.protects("/home"), "not listed");
    }

    #[test]
    fn trees_cover_everything_inside_except_user_trees() {
        assert!(SAMPLE.protects("/usr"));
        assert!(SAMPLE.protects("/usr/lib/libc.so"));
        assert!(!SAMPLE.protects("/usr/local"));
        assert!(!SAMPLE.protects("/usr/local/bin/tool"));
        assert!(!SAMPLE.protects("/usrx"), "components are compared whole, not as string prefixes");
    }

    #[test]
    fn windows_paths_match_case_insensitively_with_either_separator_and_verbatim_prefixes() {
        let windows = ProtectedLocations {
            syntax: WINDOWS_PATHS,
            exact: &["*", "*/Users/*"],
            trees: &["*/Windows"],
            user_trees_inside_system_trees: &[],
        };
        assert!(windows.protects(r"C:\"));
        assert!(windows.protects(r"D:"));
        assert!(windows.protects(r"c:\users\alex"));
        assert!(windows.protects(r"\\?\C:\WINDOWS\System32\kernel32.dll"));
        assert!(windows.protects("C:/Windows"));
        assert!(!windows.protects(r"C:\Users\alex\Downloads"));
        assert!(!windows.protects(r"C:\Games"));
    }
}
