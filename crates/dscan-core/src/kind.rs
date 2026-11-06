const LONGEST_KNOWN_EXTENSION_LEN: usize = b"sparsebundle".len();

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Kind {
    #[default]
    Other = 0,
    Video,
    Image,
    Audio,
    Archive,
    DiskImage,
    Document,
    Code,
    App,
    Library,
    Cache,
    System,
    Hidden,
    Purgeable,
    SmallItems,
}

impl Kind {
    pub const COUNT: usize = 15;

    pub const ALL: [Kind; Kind::COUNT] = [
        Kind::Other,
        Kind::Video,
        Kind::Image,
        Kind::Audio,
        Kind::Archive,
        Kind::DiskImage,
        Kind::Document,
        Kind::Code,
        Kind::App,
        Kind::Library,
        Kind::Cache,
        Kind::System,
        Kind::Hidden,
        Kind::Purgeable,
        Kind::SmallItems,
    ];

    pub fn is_synthetic(self) -> bool {
        matches!(self, Kind::Hidden | Kind::Purgeable | Kind::SmallItems)
    }

    pub fn from_u8(v: u8) -> Kind {
        Kind::ALL.get(v as usize).copied().unwrap_or(Kind::Other)
    }

    pub fn of_file(name: &[u8]) -> Kind {
        let Some(dot) = name.iter().rposition(|&b| b == b'.') else {
            return Kind::Other;
        };
        let ext = &name[dot + 1..];
        let mut lower = [0u8; LONGEST_KNOWN_EXTENSION_LEN];
        if ext.is_empty() || ext.len() > lower.len() {
            return Kind::Other;
        }
        for (d, s) in lower.iter_mut().zip(ext) {
            *d = s.to_ascii_lowercase();
        }
        Self::of_ext(&lower[..ext.len()])
    }

    fn of_ext(ext: &[u8]) -> Kind {
        match ext {
            b"mov" | b"mp4" | b"m4v" | b"mkv" | b"avi" | b"webm" | b"wmv" | b"flv" | b"mpg" | b"mpeg" | b"mts"
            | b"m2ts" | b"3gp" | b"braw" | b"r3d" | b"prproj" => Kind::Video,
            b"jpg" | b"jpeg" | b"png" | b"gif" | b"heic" | b"heif" | b"tif" | b"tiff" | b"bmp" | b"webp" | b"raw"
            | b"cr2" | b"cr3" | b"nef" | b"arw" | b"dng" | b"psd" | b"svg" | b"ico" | b"icns" | b"avif" | b"exr" => {
                Kind::Image
            }
            b"mp3" | b"m4a" | b"aac" | b"wav" | b"aif" | b"aiff" | b"flac" | b"ogg" | b"opus" | b"alac" | b"caf"
            | b"mid" | b"logicx" => Kind::Audio,
            b"zip" | b"gz" | b"tgz" | b"bz2" | b"xz" | b"zst" | b"7z" | b"rar" | b"tar" | b"lz4" | b"xip" | b"pkg"
            | b"jar" | b"whl" | b"crate" | b"apk" | b"ipa" | b"aab" => Kind::Archive,
            b"dmg" | b"iso" | b"img" | b"vmdk" | b"vdi" | b"qcow2" | b"sparseimage" | b"sparsebundle" | b"hdd"
            | b"vhd" | b"vhdx" | b"utm" | b"band" => Kind::DiskImage,
            b"pdf" | b"doc" | b"docx" | b"xls" | b"xlsx" | b"ppt" | b"pptx" | b"key" | b"pages" | b"numbers"
            | b"txt" | b"md" | b"rtf" | b"csv" | b"epub" | b"odt" | b"ods" | b"odp" | b"eml" | b"emlx" => {
                Kind::Document
            }
            b"rs" | b"c" | b"h" | b"cc" | b"cpp" | b"hpp" | b"m" | b"mm" | b"swift" | b"py" | b"js" | b"ts"
            | b"tsx" | b"jsx" | b"java" | b"kt" | b"go" | b"rb" | b"php" | b"cs" | b"json" | b"toml" | b"yaml"
            | b"yml" | b"xml" | b"html" | b"css" | b"sh" | b"lua" | b"zig" | b"o" | b"rlib" | b"rmeta" | b"d"
            | b"pyc" | b"class" | b"wasm" | b"pdb" | b"ll" | b"bc" => Kind::Code,
            b"dylib" | b"so" | b"a" | b"framework" | b"bundle" | b"dll" | b"tbd" => Kind::Library,
            b"cache" | b"tmp" | b"log" => Kind::Cache,
            _ => Kind::Other,
        }
    }

    pub fn of_dir(name: &[u8]) -> Option<Kind> {
        if name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(b".app") {
            return Some(Kind::App);
        }
        match name {
            b"Caches" | b".cache" | b"DerivedData" | b"CachedData" | b"Cache" | b"cache" => Some(Kind::Cache),
            _ => {
                let lower_ext = name.iter().rposition(|&b| b == b'.').map(|d| &name[d + 1..]);
                match lower_ext {
                    Some(b"framework") => Some(Kind::Library),
                    Some(b"photoslibrary") => Some(Kind::Image),
                    Some(b"sparsebundle") => Some(Kind::DiskImage),
                    Some(b"xcarchive") => Some(Kind::App),
                    _ => None,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_files_by_case_insensitive_extension_and_bundle_folders_by_name() {
        assert_eq!(Kind::of_file(b"IMG_7114.MOV"), Kind::Video);
        assert_eq!(Kind::of_file(b"photo.HEIC"), Kind::Image);
        assert_eq!(Kind::of_file(b"Makefile"), Kind::Other);
        assert_eq!(Kind::of_file(b"lib.rs"), Kind::Code);
        assert_eq!(Kind::of_file(b"Backup.sparseimage"), Kind::DiskImage);
        assert_eq!(Kind::of_file(b"Old.SparseBundle"), Kind::DiskImage);
        assert_eq!(Kind::of_dir(b"Safari.app"), Some(Kind::App));
        assert_eq!(Kind::of_dir(b"Caches"), Some(Kind::Cache));
        assert_eq!(Kind::of_dir(b"Documents"), None);
        for (i, k) in Kind::ALL.iter().enumerate() {
            assert_eq!(*k as usize, i, "Kind::ALL must list kinds in discriminant order for from_u8");
        }
    }
}
