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
    pub listed_location_of: &'static [OtherLocation],
}

pub(crate) struct OtherLocation {
    pub found_at: &'static str,
    pub listed_as: &'static str,
}

impl ProtectedLocations {
    pub fn protects(&self, path: &str) -> bool {
        let path = self.as_listed((self.syntax.components)(path));
        let matches_any = |patterns: &[&str], whole: bool| {
            patterns.iter().any(|pattern| self.pattern_matches(&(self.syntax.components)(pattern), &path, whole))
        };
        matches_any(self.exact, true)
            || (matches_any(self.trees, false) && !matches_any(self.user_trees_inside_system_trees, false))
    }

    fn as_listed<'p>(&self, path: Vec<&'p str>) -> Vec<&'p str> {
        for other in self.listed_location_of {
            let found_at = (self.syntax.components)(other.found_at);
            if self.pattern_matches(&found_at, &path, false) {
                let listed_as = (self.syntax.components)(other.listed_as);
                return listed_as.into_iter().chain(path[found_at.len()..].iter().copied()).collect();
            }
        }
        path
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
        listed_location_of: &[OtherLocation { found_at: "/var/home", listed_as: "/home" }],
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
    fn a_folder_found_at_another_location_is_protected_like_the_listed_one() {
        assert!(!SAMPLE.protects("/var/home"), "/home is not listed, so neither is its other location");
        assert!(SAMPLE.protects("/var/home/alex"));
        assert!(SAMPLE.protects("/var/home/alex/Documents"));
        assert!(!SAMPLE.protects("/var/home/alex/Documents/notes.txt"));
        assert!(!SAMPLE.protects("/var/homework"), "components are compared whole");
    }

    #[test]
    fn windows_paths_match_case_insensitively_with_either_separator_and_verbatim_prefixes() {
        let windows = ProtectedLocations {
            syntax: WINDOWS_PATHS,
            exact: &["*", "*/Users/*"],
            trees: &["*/Windows"],
            user_trees_inside_system_trees: &[],
            listed_location_of: &[],
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
