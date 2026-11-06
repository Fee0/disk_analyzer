//! Quick Look preview via `qlmanage -p` (works from an unbundled binary).

use std::path::Path;
use std::process::{Child, Command, Stdio};

#[derive(Default)]
pub struct QuickLook {
    child: Option<Child>,
}

impl QuickLook {
    pub fn is_open(&mut self) -> bool {
        match &mut self.child {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Open a preview of `path`, or close the current one if it is showing.
    pub fn toggle(&mut self, path: Option<&Path>) {
        if self.is_open() {
            self.close();
            return;
        }
        if let Some(p) = path {
            self.child = Command::new("/usr/bin/qlmanage")
                .arg("-p")
                .arg(p)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .ok();
        }
    }

    pub fn close(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Drop for QuickLook {
    fn drop(&mut self) {
        self.close();
    }
}
