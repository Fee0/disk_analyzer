use std::cmp::Reverse;

use crate::exclusions::{Exclusions, Step};
use crate::tree::{NodeId, Tree};

const LONGEST_KNOWN_EXTENSION_LEN: usize = b"photoslibrary".len();

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
        LowercaseExtension::of(name).map_or(Kind::Other, |extension| Self::of_ext(extension.bytes()))
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
            | b"jar" | b"whl" | b"crate" | b"apk" | b"ipa" | b"aab" | b"deb" | b"rpm" | b"cab" | b"msi" | b"msix"
            | b"msu" | b"flatpak" | b"snap" => Kind::Archive,
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
            b"dylib" | b"so" | b"a" | b"framework" | b"bundle" | b"dll" | b"tbd" | b"lib" | b"sys" => Kind::Library,
            b"exe" | b"appimage" => Kind::App,
            b"cache" | b"tmp" | b"log" => Kind::Cache,
            _ => Kind::Other,
        }
    }

    pub fn of_dir(name: &[u8]) -> Option<Kind> {
        match name {
            b"Caches" | b".cache" | b"DerivedData" | b"CachedData" | b"Cache" | b"cache" => Some(Kind::Cache),
            _ => match LowercaseExtension::of(name)?.bytes() {
                b"app" | b"xcarchive" => Some(Kind::App),
                b"framework" => Some(Kind::Library),
                b"photoslibrary" => Some(Kind::Image),
                b"sparsebundle" => Some(Kind::DiskImage),
                _ => None,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KindSet(u16);

const _: () = assert!(Kind::COUNT <= u16::BITS as usize, "every kind needs a bit of a KindSet");

impl KindSet {
    pub(crate) fn of(kind: Kind) -> KindSet {
        KindSet(1 << kind as u16)
    }

    pub(crate) fn contains(self, kind: Kind) -> bool {
        self.0 & KindSet::of(kind).0 != 0
    }

    pub(crate) fn union(self, other: KindSet) -> KindSet {
        KindSet(self.0 | other.0)
    }

    pub(crate) fn only(self) -> Option<Kind> {
        (self.0.count_ones() == 1).then(|| Kind::from_u8(self.0.trailing_zeros() as u8))
    }
}

impl FromIterator<KindSet> for KindSet {
    fn from_iter<I: IntoIterator<Item = KindSet>>(sets: I) -> KindSet {
        sets.into_iter().fold(KindSet::default(), KindSet::union)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KindBreakdown {
    bytes: [u64; Kind::COUNT],
}

impl KindBreakdown {
    pub fn bytes(&self, kind: Kind) -> u64 {
        self.bytes[kind as usize]
    }

    pub fn total(&self) -> u64 {
        self.bytes.iter().sum()
    }

    pub fn largest_first(&self) -> Vec<(Kind, u64)> {
        let mut present: Vec<(Kind, u64)> =
            Kind::ALL.into_iter().map(|kind| (kind, self.bytes(kind))).filter(|&(_, bytes)| bytes > 0).collect();
        present.sort_by_key(|&(_, bytes)| Reverse(bytes));
        present
    }
}

pub fn kind_breakdown(tree: &Tree, focus: NodeId, excluded: &Exclusions) -> KindBreakdown {
    let mut breakdown = KindBreakdown::default();
    excluded.walk_below(tree, focus, |child, size| match tree.kinds_inside(child).only() {
        Some(kind) => {
            breakdown.bytes[kind as usize] += size;
            Step::Stop
        }
        None => Step::Descend,
    });
    breakdown
}

struct LowercaseExtension {
    buffer: [u8; LONGEST_KNOWN_EXTENSION_LEN],
    len: usize,
}

impl LowercaseExtension {
    fn of(name: &[u8]) -> Option<Self> {
        let dot = name.iter().rposition(|&b| b == b'.')?;
        let extension = &name[dot + 1..];
        let mut buffer = [0u8; LONGEST_KNOWN_EXTENSION_LEN];
        if extension.is_empty() || extension.len() > buffer.len() {
            return None;
        }
        for (lower, byte) in buffer.iter_mut().zip(extension) {
            *lower = byte.to_ascii_lowercase();
        }
        Some(LowercaseExtension { buffer, len: extension.len() })
    }

    fn bytes(&self) -> &[u8] {
        &self.buffer[..self.len]
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
        assert_eq!(Kind::of_dir(b"Photos.PhotosLibrary"), Some(Kind::Image), "bundle folders match in any case");
        assert_eq!(Kind::of_file(b"setup.exe"), Kind::App);
        assert_eq!(Kind::of_file(b"package.deb"), Kind::Archive);
        for (i, k) in Kind::ALL.iter().enumerate() {
            assert_eq!(*k as usize, i, "Kind::ALL must list kinds in discriminant order for from_u8");
        }
    }

    #[test]
    fn a_kind_set_holds_any_mix_of_kinds_and_names_its_kind_only_when_it_holds_one() {
        let videos_and_images: KindSet = [KindSet::of(Kind::Video), KindSet::of(Kind::Image)].into_iter().collect();
        assert!(videos_and_images.contains(Kind::Video) && videos_and_images.contains(Kind::Image));
        assert!(!videos_and_images.contains(Kind::Other));
        assert_eq!(videos_and_images.only(), None);
        assert_eq!(KindSet::of(Kind::SmallItems).only(), Some(Kind::SmallItems), "the last kind fits too");
        assert_eq!(KindSet::default().only(), None);
    }

    #[test]
    fn the_breakdown_counts_files_by_kind_and_a_bundle_whole_as_its_own_kind() {
        use crate::tree::test_util::{T::*, descend, tree};
        let t = tree(vec![
            D("Movies", vec![F("a.mov", 500), F("b.mov", 300), F("notes.txt", 20)]),
            D("Safari.app", vec![F("Safari", 100), D("Web.framework", vec![F("Web", 60)])]),
            F("photo.jpg", 300),
        ]);
        let all = kind_breakdown(&t, t.root(), &Exclusions::default());
        assert_eq!(all.bytes(Kind::Video), 800);
        assert_eq!(all.bytes(Kind::App), 160, "the framework inside the app counts as the app");
        assert_eq!(all.bytes(Kind::Library), 0);
        assert_eq!(all.total(), t.size(t.root()), "every byte is counted once");
        assert_eq!(
            all.largest_first(),
            [(Kind::Video, 800), (Kind::Image, 300), (Kind::App, 160), (Kind::Document, 20)],
            "ties would keep the order of Kind::ALL"
        );
        let inside_the_app = kind_breakdown(&t, descend(&t, &["Safari.app"]), &Exclusions::default());
        assert_eq!(inside_the_app.largest_first(), [(Kind::App, 160)], "inside a bundle it is all the bundle's kind");
        let without_a_movie = Exclusions::new(&t, &[descend(&t, &["Movies", "a.mov"])]);
        assert_eq!(kind_breakdown(&t, t.root(), &without_a_movie).bytes(Kind::Video), 300, "collected items leave");
    }
}
