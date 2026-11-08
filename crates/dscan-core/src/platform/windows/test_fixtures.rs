use std::io;
use std::path::Path;

use windows_sys::Win32::Foundation::GENERIC_WRITE;
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
use windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT;

use super::handle::{Handle, wide_path};
use super::paths;

const NT_NAMESPACE_PREFIX: &str = r"\??\";
const UTF16_NUL_BYTES: usize = 2;
const NAME_OFFSETS_BYTES: u16 = 8;

fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn mount_point_reparse_data(target: &Path) -> Vec<u8> {
    let target = paths::without_verbatim_prefix(&target.to_string_lossy()).into_owned();
    let substitute_name = utf16le(&format!("{NT_NAMESPACE_PREFIX}{target}"));
    let print_name = utf16le(&target);
    let names_bytes = substitute_name.len() + UTF16_NUL_BYTES + print_name.len() + UTF16_NUL_BYTES;
    let substitute_name_offset = 0u16;
    let print_name_offset = (substitute_name.len() + UTF16_NUL_BYTES) as u16;

    let mut data = Vec::new();
    data.extend(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    data.extend((NAME_OFFSETS_BYTES + names_bytes as u16).to_le_bytes());
    data.extend(0u16.to_le_bytes());
    for field in [substitute_name_offset, substitute_name.len() as u16, print_name_offset, print_name.len() as u16] {
        data.extend(field.to_le_bytes());
    }
    for name in [substitute_name, print_name] {
        data.extend(name);
        data.extend([0u8; UTF16_NUL_BYTES]);
    }
    data
}

pub(crate) fn make_junction(junction: &Path, target: &Path) -> io::Result<()> {
    std::fs::create_dir(junction)?;
    let folder = Handle::open_without_following_links(&wide_path(junction), GENERIC_WRITE)?;
    let data = mount_point_reparse_data(target);
    let mut bytes_returned = 0u32;
    let made = unsafe {
        DeviceIoControl(
            folder.0,
            FSCTL_SET_REPARSE_POINT,
            data.as_ptr().cast(),
            data.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
    };
    if made == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}
