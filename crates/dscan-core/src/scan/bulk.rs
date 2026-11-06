//! Directory reader built on `getattrlistbulk(2)`.
//!
//! One syscall returns name, type, device, inode, link count, logical and
//! allocated size for many directory entries at once, which avoids the
//! per-file `lstat` that `std::fs::read_dir` based walkers need.

use std::ffi::CStr;
use std::io;

const ATTR_CMN_ERROR: u32 = 0x2000_0000;
const BUF_SIZE: usize = 256 * 1024;

/// `fsobj_type_t` values from `<sys/vnode.h>`.
pub const VREG: u32 = 1;
pub const VDIR: u32 = 2;
pub const VLNK: u32 = 5;

/// One directory entry as reported by the kernel.
#[derive(Debug, Clone, Copy)]
pub struct RawEntry<'a> {
    pub name: &'a [u8],
    /// Per-entry errno; when non-zero the other attributes may be missing.
    pub error: u32,
    pub dev: i32,
    pub objtype: u32,
    pub ino: u64,
    pub links: u32,
    pub alloc: u64,
    pub logical: u64,
}

thread_local! {
    static BUF: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(vec![0u8; BUF_SIZE]);
}

struct Fd(i32);
impl Drop for Fd {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

/// Open a directory (without following a trailing symlink).
fn open_dir(path: &CStr) -> io::Result<Fd> {
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 { Err(io::Error::last_os_error()) } else { Ok(Fd(fd)) }
}

#[inline]
fn rd_u32(b: &[u8], at: usize) -> u32 {
    u32::from_ne_bytes(b[at..at + 4].try_into().unwrap())
}
#[inline]
fn rd_i32(b: &[u8], at: usize) -> i32 {
    i32::from_ne_bytes(b[at..at + 4].try_into().unwrap())
}
#[inline]
fn rd_u64(b: &[u8], at: usize) -> u64 {
    u64::from_ne_bytes(b[at..at + 8].try_into().unwrap())
}

/// Read every entry of the directory at `path`, calling `f` for each.
pub fn read_dir(path: &CStr, mut f: impl FnMut(RawEntry<'_>)) -> io::Result<()> {
    let fd = open_dir(path)?;
    let mut attrs: libc::attrlist = unsafe { std::mem::zeroed() };
    attrs.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
    attrs.commonattr = libc::ATTR_CMN_RETURNED_ATTRS
        | ATTR_CMN_ERROR
        | libc::ATTR_CMN_NAME
        | libc::ATTR_CMN_DEVID
        | libc::ATTR_CMN_OBJTYPE
        | libc::ATTR_CMN_FILEID;
    attrs.fileattr =
        libc::ATTR_FILE_LINKCOUNT | libc::ATTR_FILE_TOTALSIZE | libc::ATTR_FILE_ALLOCSIZE;

    BUF.with_borrow_mut(|buf| {
        loop {
            let n = unsafe {
                libc::getattrlistbulk(
                    fd.0,
                    &mut attrs as *mut _ as *mut libc::c_void,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                    0,
                )
            };
            if n < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            if n == 0 {
                return Ok(());
            }
            let mut p = 0usize;
            for _ in 0..n {
                let len = rd_u32(buf, p) as usize;
                if len == 0 || p + len > buf.len() {
                    return Err(io::Error::other("malformed getattrlistbulk record"));
                }
                let rec = &buf[p..p + len];
                parse_entry(rec, &mut f);
                p += len;
            }
        }
    })
}

fn parse_entry(rec: &[u8], f: &mut impl FnMut(RawEntry<'_>)) {
    // length (4) + attribute_set_t (5 x u32)
    let common = rd_u32(rec, 4);
    let fileattr = rd_u32(rec, 4 + 12);
    let mut q = 4 + 20;
    let mut e = RawEntry {
        name: &[],
        error: 0,
        dev: 0,
        objtype: 0,
        ino: 0,
        links: 1,
        alloc: 0,
        logical: 0,
    };
    if common & ATTR_CMN_ERROR != 0 {
        // Entries with errors still carry a name.
        e.error = rd_u32(rec, q);
        q += 4;
    }
    if common & libc::ATTR_CMN_NAME != 0 {
        let off = rd_i32(rec, q);
        let nlen = rd_u32(rec, q + 4) as usize;
        let start = (q as isize + off as isize) as usize;
        // nlen includes the trailing NUL.
        if nlen > 0 && start + nlen <= rec.len() {
            e.name = &rec[start..start + nlen - 1];
        }
        q += 8;
    }
    if common & libc::ATTR_CMN_DEVID != 0 {
        e.dev = rd_i32(rec, q);
        q += 4;
    }
    if common & libc::ATTR_CMN_OBJTYPE != 0 {
        e.objtype = rd_u32(rec, q);
        q += 4;
    }
    if common & libc::ATTR_CMN_FILEID != 0 {
        e.ino = rd_u64(rec, q);
        q += 8;
    }
    if fileattr & libc::ATTR_FILE_LINKCOUNT != 0 {
        e.links = rd_u32(rec, q);
        q += 4;
    }
    if fileattr & libc::ATTR_FILE_TOTALSIZE != 0 {
        e.logical = rd_u64(rec, q);
        q += 8;
    }
    if fileattr & libc::ATTR_FILE_ALLOCSIZE != 0 {
        e.alloc = rd_u64(rec, q);
    }
    if !e.name.is_empty() {
        f(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn reads_entries_with_sizes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), vec![1u8; 10_000]).unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let c = CString::new(dir.path().as_os_str().as_bytes()).unwrap();
        let mut seen = Vec::new();
        read_dir(&c, |e| seen.push((e.name.to_vec(), e.objtype, e.logical, e.alloc))).unwrap();
        seen.sort();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, b"a.txt");
        assert_eq!(seen[0].1, VREG);
        assert_eq!(seen[0].2, 10_000);
        assert!(seen[0].3 >= 10_000);
        assert_eq!(seen[1].0, b"sub");
        assert_eq!(seen[1].1, VDIR);
    }
}
