use crate::protection::PathSyntax;

pub(crate) const UNIX_PATHS: PathSyntax = PathSyntax { components, same_name: |a, b| a == b };

fn components(path: &str) -> Vec<&str> {
    path.split('/').filter(|c| !c.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_ignore_separators_at_either_end_and_the_root_has_none() {
        assert_eq!((UNIX_PATHS.components)("/usr/local/"), ["usr", "local"]);
        assert!((UNIX_PATHS.components)("/").is_empty());
    }
}
