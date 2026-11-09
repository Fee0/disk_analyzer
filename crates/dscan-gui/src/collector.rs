use std::ops::Deref;

use dscan_core::{Collection, NodeId, Refusal, Tree};

#[derive(Default)]
pub struct Collector {
    collection: Collection,
    list_open: bool,
    confirming_trash: bool,
}

impl Deref for Collector {
    type Target = Collection;

    fn deref(&self) -> &Collection {
        &self.collection
    }
}

impl Collector {
    pub fn list_open(&self) -> bool {
        self.list_open
    }

    pub fn confirming_trash(&self) -> bool {
        self.confirming_trash
    }

    pub fn add(&mut self, tree: &Tree, id: NodeId) -> Result<(), Refusal> {
        self.collection.add(tree, id)
    }

    pub fn remove(&mut self, tree: &Tree, id: NodeId) {
        self.collection.remove(tree, id);
        self.close_once_empty();
    }

    pub fn clear(&mut self) {
        *self = Collector::default();
    }

    pub fn prune(&mut self, tree: &Tree) {
        self.collection.prune(tree);
        self.close_once_empty();
    }

    pub fn toggle_list(&mut self) {
        self.list_open = !self.list_open && !self.collection.is_empty();
    }

    pub fn ask_to_trash(&mut self) {
        self.confirming_trash = !self.collection.is_empty();
    }

    pub fn stop_asking_to_trash(&mut self) {
        self.confirming_trash = false;
    }

    pub fn close_the_innermost_panel(&mut self) -> bool {
        let open = if self.confirming_trash { &mut self.confirming_trash } else { &mut self.list_open };
        std::mem::replace(open, false)
    }

    fn close_once_empty(&mut self) {
        if self.collection.is_empty() {
            *self = Collector::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use dscan_core::ScanOptions;

    use super::*;
    use crate::app::test_util::{descend, scan_result};
    use crate::inspect_tests::write_files;

    fn collector_holding(names: &[&str]) -> (tempfile::TempDir, Tree, Collector) {
        let dir = tempfile::tempdir().unwrap();
        write_files(dir.path(), &[("a.bin", 3000), ("b.bin", 2000)], 1);
        let tree = scan_result(&ScanOptions::folder(dir.path())).unwrap();
        let mut collector = Collector::default();
        for name in names {
            collector.add(&tree, descend(&tree, &[name])).unwrap();
        }
        (dir, tree, collector)
    }

    #[test]
    fn emptying_the_collector_closes_its_list_and_its_trash_question() {
        let (_dir, tree, mut c) = collector_holding(&["a.bin", "b.bin"]);
        c.toggle_list();
        c.ask_to_trash();
        c.remove(&tree, descend(&tree, &["a.bin"]));
        assert!(c.list_open() && c.confirming_trash(), "one item left keeps both open");
        c.remove(&tree, descend(&tree, &["b.bin"]));
        assert!(!c.list_open() && !c.confirming_trash(), "nothing left to list or to move");
        c.add(&tree, descend(&tree, &["a.bin"])).unwrap();
        assert!(!c.confirming_trash(), "collecting again does not bring back the old question");
    }

    #[test]
    fn an_empty_collector_opens_neither_its_list_nor_a_trash_question() {
        let (_dir, _tree, mut c) = collector_holding(&[]);
        c.toggle_list();
        c.ask_to_trash();
        assert!(!c.list_open() && !c.confirming_trash());
    }

    #[test]
    fn escape_closes_the_trash_question_before_the_list() {
        let (_dir, _tree, mut c) = collector_holding(&["a.bin"]);
        c.toggle_list();
        c.ask_to_trash();
        assert!(c.close_the_innermost_panel() && c.list_open() && !c.confirming_trash());
        assert!(c.close_the_innermost_panel() && !c.list_open());
        assert!(!c.close_the_innermost_panel(), "nothing was left open");
    }
}
