use crate::chart_geometry::ChartPoint;
use crate::exclusions::Exclusions;
use crate::kind::Kind;
use crate::layout::{Piece, Segment, SmallItems};
use crate::search::{SearchResult, is_relevant_under};
use crate::tree::{NodeId, Tree};

const FOCUS_BAR_HEIGHT: f32 = 30.0;
const FOCUS_BAR_GAP: f32 = 8.0;
const FOLDER_FRAME: f32 = 2.0;
const FOLDER_HEADER_HEIGHT: f32 = 17.0;
const SMALLEST_FRAMED_WIDTH: f32 = 24.0;
const SMALLEST_FRAMED_CONTENTS_HEIGHT: f32 = 8.0;
const SMALLEST_TILE_AREA: f64 = 36.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TileRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl TileRect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        TileRect { x, y, width, height }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn area(&self) -> f32 {
        self.width * self.height
    }

    pub fn center(&self) -> ChartPoint {
        ChartPoint { x: self.x + self.width / 2.0, y: self.y + self.height / 2.0 }
    }

    pub fn contains(&self, p: ChartPoint) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    pub fn inset(&self, by: f32) -> Self {
        TileRect::new(self.x + by, self.y + by, (self.width - 2.0 * by).max(0.0), (self.height - 2.0 * by).max(0.0))
    }

    pub(crate) fn framed_contents(&self) -> Option<TileRect> {
        let contents = TileRect::new(
            self.x + FOLDER_FRAME,
            self.y + FOLDER_HEADER_HEIGHT,
            self.width - 2.0 * FOLDER_FRAME,
            self.height - FOLDER_HEADER_HEIGHT - FOLDER_FRAME,
        );
        let fits = self.width >= SMALLEST_FRAMED_WIDTH && contents.height >= SMALLEST_FRAMED_CONTENTS_HEIGHT;
        fits.then_some(contents)
    }

    pub(crate) fn lerp(&self, to: &TileRect, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        TileRect::new(mix(self.x, to.x), mix(self.y, to.y), mix(self.width, to.width), mix(self.height, to.height))
    }

    pub(crate) fn scaled_about_center(&self, scale: f32) -> Self {
        let ChartPoint { x, y } = self.center();
        let (width, height) = (self.width * scale, self.height * scale);
        TileRect::new(x - width / 2.0, y - height / 2.0, width, height)
    }

    pub(crate) fn clipped_to(&self, bounds: &TileRect) -> Self {
        let (x, y) = (self.x.max(bounds.x), self.y.max(bounds.y));
        let (right, bottom) = (self.right().min(bounds.right()), self.bottom().min(bounds.bottom()));
        TileRect::new(x, y, (right - x).max(0.0), (bottom - y).max(0.0))
    }

    pub(crate) fn mapped(&self, from: &TileRect, to: &TileRect) -> Self {
        let (scale_x, scale_y) = (to.width / from.width, to.height / from.height);
        TileRect::new(
            to.x + (self.x - from.x) * scale_x,
            to.y + (self.y - from.y) * scale_y,
            self.width * scale_x,
            self.height * scale_y,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub segment: Segment,
    pub rect: TileRect,
    pub depth: usize,
    pub size: u64,
    pub kind: Kind,
    pub branch: u32,
    pub relevant: bool,
    pub(crate) parent: Option<usize>,
    descendants: usize,
}

impl Tile {
    pub fn holds_tiles(&self) -> bool {
        self.descendants > 0
    }

    pub fn header(&self) -> Option<TileRect> {
        let r = self.rect;
        self.holds_tiles().then_some(TileRect::new(r.x, r.y, r.width, FOLDER_HEADER_HEIGHT))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Treemap {
    pub focus: NodeId,
    pub total: u64,
    pub focus_bar: TileRect,
    pub area: TileRect,
    pub tiles: Vec<Tile>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TreemapHit<'a> {
    FocusBar,
    Tile(&'a Tile),
    Nothing,
}

impl Treemap {
    pub fn hit(&self, p: ChartPoint) -> TreemapHit<'_> {
        if self.focus_bar.contains(p) {
            return TreemapHit::FocusBar;
        }
        let (mut i, mut end, mut deepest) = (0, self.tiles.len(), None);
        while i < end {
            let tile = &self.tiles[i];
            let subtree_end = i + 1 + tile.descendants;
            if tile.rect.contains(p) {
                deepest = Some(tile);
                end = subtree_end;
                i += 1;
            } else {
                i = subtree_end;
            }
        }
        deepest.map_or(TreemapHit::Nothing, TreemapHit::Tile)
    }

    pub fn find(&self, segment: Segment) -> Option<&Tile> {
        self.tiles.iter().find(|t| t.segment == segment)
    }
}

pub fn treemap(
    tree: &Tree,
    focus: NodeId,
    (width, height): (f32, f32),
    search: Option<&SearchResult>,
    excluded: &Exclusions,
) -> Treemap {
    let below_the_bar = FOCUS_BAR_HEIGHT + FOCUS_BAR_GAP;
    let mut map = Treemap {
        focus,
        total: excluded.size(tree, focus),
        focus_bar: TileRect::new(0.0, 0.0, width, FOCUS_BAR_HEIGHT),
        area: TileRect::new(0.0, below_the_bar, width, (height - below_the_bar).max(0.0)),
        tiles: Vec::new(),
    };
    let mut tiler = Tiler { tree, search, excluded, tiles: Vec::new() };
    tiler.place_children(focus, map.area, 0, None, None);
    map.tiles = tiler.tiles;
    map
}

struct Tiler<'a> {
    tree: &'a Tree,
    search: Option<&'a SearchResult>,
    excluded: &'a Exclusions,
    tiles: Vec<Tile>,
}

impl Tiler<'_> {
    fn is_relevant(&self, id: NodeId) -> bool {
        is_relevant_under(self.search, id)
    }

    fn piece(&self, id: NodeId, size: u64) -> Piece {
        Piece { segment: Segment::Node(id), size, kind: self.tree.node(id).kind(), relevant: self.is_relevant(id) }
    }

    fn pieces(&self, folder: NodeId, content_area: f64, is_the_focus: bool) -> Vec<Piece> {
        let total = self.excluded.size(self.tree, folder);
        if total == 0 || content_area <= 0.0 {
            return Vec::new();
        }
        let area_per_byte = content_area / total as f64;
        let mut pieces = Vec::new();
        let mut small = SmallItems::default();
        for (child, size) in self.excluded.children_with_sizes(self.tree, folder) {
            if size == 0 {
                continue;
            }
            if small.is_empty() && size as f64 * area_per_byte >= SMALLEST_TILE_AREA {
                pieces.push(self.piece(child, size));
            } else {
                small.add(child, size, self.is_relevant(child));
            }
        }
        let too_small_to_show_anything_inside = pieces.is_empty() && !is_the_focus;
        if !too_small_to_show_anything_inside {
            pieces.extend(small.piece(self.tree, folder));
        }
        pieces
    }

    fn place_children(
        &mut self,
        folder: NodeId,
        content: TileRect,
        depth: usize,
        parent: Option<usize>,
        branch: Option<u32>,
    ) {
        let pieces = self.pieces(folder, f64::from(content.area()), parent.is_none());
        let rects = squarify(&pieces.iter().map(|p| p.size as f64).collect::<Vec<_>>(), content);
        for (i, (piece, rect)) in pieces.iter().zip(rects).enumerate() {
            let index = self.tiles.len();
            let branch = branch.unwrap_or(i as u32);
            self.tiles.push(Tile {
                segment: piece.segment,
                rect,
                depth,
                size: piece.size,
                kind: piece.kind,
                branch,
                relevant: piece.relevant,
                parent,
                descendants: 0,
            });
            if let Segment::Node(child) = piece.segment
                && self.tree.node(child).is_dir()
                && let Some(inner) = rect.framed_contents()
            {
                self.place_children(child, inner, depth + 1, Some(index), Some(branch));
            }
            self.tiles[index].descendants = self.tiles.len() - index - 1;
        }
    }
}

#[derive(Clone, Copy)]
struct Row {
    area: f64,
    largest: f64,
    smallest: f64,
}

impl Row {
    fn of(area: f64) -> Self {
        Row { area, largest: area, smallest: area }
    }

    fn with(self, area: f64) -> Self {
        Row { area: self.area + area, largest: self.largest.max(area), smallest: self.smallest.min(area) }
    }

    fn worst_aspect_ratio(&self, side: f64) -> f64 {
        let (side_squared, area_squared) = (side * side, self.area * self.area);
        (side_squared * self.largest / area_squared).max(area_squared / (side_squared * self.smallest))
    }
}

struct FreeSpace {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl FreeSpace {
    fn lay_row(&mut self, areas: &[f64], row_area: f64, fills_the_rest: bool, out: &mut Vec<TileRect>) {
        let stacked_top_to_bottom = self.width >= self.height;
        let (length, depth_left) =
            if stacked_top_to_bottom { (self.height, self.width) } else { (self.width, self.height) };
        let thickness = if fills_the_rest || length <= 0.0 { depth_left } else { (row_area / length).min(depth_left) };
        let mut offset = 0.0;
        for (i, area) in areas.iter().enumerate() {
            let is_last = i + 1 == areas.len();
            let extent = if is_last || thickness <= 0.0 { length - offset } else { area / thickness };
            let (x, y, width, height) = if stacked_top_to_bottom {
                (self.x, self.y + offset, thickness, extent)
            } else {
                (self.x + offset, self.y, extent, thickness)
            };
            out.push(TileRect::new(x as f32, y as f32, width as f32, height as f32));
            offset += extent;
        }
        if stacked_top_to_bottom {
            self.x += thickness;
            self.width -= thickness;
        } else {
            self.y += thickness;
            self.height -= thickness;
        }
    }
}

fn squarify(sizes: &[f64], within: TileRect) -> Vec<TileRect> {
    let total: f64 = sizes.iter().sum();
    let mut rects = Vec::with_capacity(sizes.len());
    if total <= 0.0 {
        return rects;
    }
    let area_per_unit = f64::from(within.area()) / total;
    let areas: Vec<f64> = sizes.iter().map(|s| s * area_per_unit).collect();
    let mut free = FreeSpace {
        x: f64::from(within.x),
        y: f64::from(within.y),
        width: f64::from(within.width),
        height: f64::from(within.height),
    };
    let mut start = 0;
    while start < areas.len() {
        let short_side = free.width.min(free.height);
        let mut row = Row::of(areas[start]);
        let mut end = start + 1;
        while end < areas.len() {
            let grown = row.with(areas[end]);
            if grown.worst_aspect_ratio(short_side) > row.worst_aspect_ratio(short_side) {
                break;
            }
            row = grown;
            end += 1;
        }
        free.lay_row(&areas[start..end], row.area, end == areas.len(), &mut rects);
        start = end;
    }
    rects
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tree::test_util::{T::*, descend, tree};

    pub(crate) const SIZE: (f32, f32) = (800.0, 600.0);
    const PIXEL: f32 = 1e-2;

    pub(crate) fn map_of(t: &Tree, focus: NodeId, excluded: &[NodeId]) -> Treemap {
        treemap(t, focus, SIZE, None, &Exclusions::new(t, excluded))
    }

    fn sample() -> Tree {
        tree(vec![
            D(
                "Movies",
                vec![
                    F("a.mov", 400_000),
                    F("b.mov", 200_000),
                    D("clips", (0..6).map(|_| F("c.mp4", 60_000)).collect()),
                ],
            ),
            D("Music", vec![F("song.mp3", 150_000), F("other.mp3", 50_000)]),
            F("disk.iso", 300_000),
            F("notes.txt", 100),
        ])
    }

    fn children_of(map: &Treemap, parent: Option<usize>) -> Vec<&Tile> {
        map.tiles.iter().filter(|t| t.parent == parent).collect()
    }

    fn encloses(outer: &TileRect, inner: &TileRect) -> bool {
        inner.x >= outer.x - PIXEL
            && inner.y >= outer.y - PIXEL
            && inner.right() <= outer.right() + PIXEL
            && inner.bottom() <= outer.bottom() + PIXEL
    }

    fn overlap(a: &TileRect, b: &TileRect) -> f32 {
        a.clipped_to(b).area()
    }

    #[test]
    fn the_focus_bar_sits_on_top_and_the_tiles_fill_the_rest() {
        let t = sample();
        let map = map_of(&t, t.root(), &[]);
        assert_eq!(map.focus_bar, TileRect::new(0.0, 0.0, SIZE.0, FOCUS_BAR_HEIGHT));
        assert!(map.area.y > map.focus_bar.bottom(), "a gap separates the bar from the tiles");
        assert_eq!(map.area.bottom(), SIZE.1);
        let top_level_area: f32 = children_of(&map, None).iter().map(|t| t.rect.area()).sum();
        assert!((top_level_area - map.area.area()).abs() < 1.0, "the focus's children tile the whole area");
    }

    #[test]
    fn every_tile_s_area_matches_its_share_of_the_size() {
        let t = sample();
        let map = map_of(&t, t.root(), &[]);
        let top = children_of(&map, None);
        let sizes: u64 = top.iter().map(|t| t.size).sum();
        for tile in top {
            let share_of_area = tile.rect.area() / map.area.area();
            let share_of_size = tile.size as f32 / sizes as f32;
            assert!((share_of_area - share_of_size).abs() < 1e-3, "{tile:?}: {share_of_area} vs {share_of_size}");
        }
    }

    #[test]
    fn children_sit_inside_their_folder_s_frame_below_its_header_and_siblings_never_overlap() {
        let t = sample();
        let map = map_of(&t, t.root(), &[]);
        let movies = map.tiles.iter().position(|tile| tile.segment == Segment::Node(descend(&t, &["Movies"]))).unwrap();
        assert!(map.tiles[movies].holds_tiles(), "a big folder shows what it holds");
        let content = map.tiles[movies].rect.framed_contents().unwrap();
        let header = map.tiles[movies].header().expect("a folder showing its contents has a header");
        for child in children_of(&map, Some(movies)) {
            assert!(encloses(&content, &child.rect), "{child:?} lies inside {content:?}");
            assert!(child.rect.y >= header.bottom() - PIXEL, "{child:?} sits below the header {header:?}");
            assert_eq!(child.depth, 1);
            assert_eq!(child.branch, map.tiles[movies].branch, "children keep their top-level branch");
        }
        for parent in std::iter::once(None).chain((0..map.tiles.len()).map(Some)) {
            let siblings = children_of(&map, parent);
            for (i, a) in siblings.iter().enumerate() {
                for b in &siblings[i + 1..] {
                    assert!(overlap(&a.rect, &b.rect) < 1.0, "{a:?} overlaps {b:?}");
                }
            }
        }
    }

    #[test]
    fn squarified_tiles_stay_close_to_square() {
        let t = tree((0..12).map(|_| F("same.bin", 1000)).collect());
        let map = map_of(&t, t.root(), &[]);
        assert_eq!(map.tiles.len(), 12);
        for tile in &map.tiles {
            let aspect = tile.rect.width.max(tile.rect.height) / tile.rect.width.min(tile.rect.height);
            assert!(aspect < 2.5, "{tile:?} is a sliver: aspect {aspect}");
        }
    }

    #[test]
    fn items_too_small_to_draw_share_one_tile_that_keeps_their_total() {
        let mut kids = vec![F("big.bin", 10_000_000)];
        kids.extend((0..400).map(|_| F("crumb", 10)));
        let t = tree(kids);
        let map = map_of(&t, t.root(), &[]);
        assert_eq!(map.tiles.len(), 2);
        assert!(matches!(map.tiles[1].segment, Segment::Small { count: 400, .. }));
        assert_eq!(map.tiles[1].kind, Kind::SmallItems);
        assert_eq!(map.tiles[1].size, 4000);
    }

    #[test]
    fn a_focus_holding_only_crumbs_still_shows_them_as_one_small_items_tile() {
        let t = tree((0..20_000).map(|_| F("crumb", 10)).collect());
        let map = map_of(&t, t.root(), &[]);
        assert_eq!(map.tiles.len(), 1, "the focus is never left blank");
        assert!(matches!(map.tiles[0].segment, Segment::Small { count: 20_000, .. }), "{:?}", map.tiles[0]);
    }

    #[test]
    fn a_folder_whose_contents_are_all_too_small_stays_one_solid_tile() {
        let mut kids = vec![F("big.bin", 10_000_000)];
        kids.push(D("crumbs", (0..50).map(|_| F("crumb", 400)).collect()));
        let t = tree(kids);
        let map = map_of(&t, t.root(), &[]);
        let crumbs = map.find(Segment::Node(descend(&t, &["crumbs"]))).unwrap();
        assert!(!crumbs.holds_tiles(), "{crumbs:?}");
        assert_eq!(crumbs.header(), None, "a solid tile has no header");
        assert_eq!(map.tiles.len(), 2, "no lone small-items tile fills the folder");
    }

    #[test]
    fn a_point_hits_the_focus_bar_the_deepest_tile_under_it_or_nothing() {
        let t = sample();
        let map = map_of(&t, t.root(), &[]);
        assert_eq!(map.hit(map.focus_bar.center()), TreemapHit::FocusBar);
        for tile in map.tiles.iter().filter(|t| !t.holds_tiles()) {
            assert_eq!(map.hit(tile.rect.center()), TreemapHit::Tile(tile), "a leaf's center hits it");
        }
        let movies = map.find(Segment::Node(descend(&t, &["Movies"]))).unwrap();
        let in_the_frame = ChartPoint { x: movies.rect.x + FOLDER_FRAME / 2.0, y: movies.rect.center().y };
        assert_eq!(map.hit(in_the_frame), TreemapHit::Tile(movies), "a folder's frame belongs to the folder");
        let on_the_header = movies.header().unwrap().center();
        assert_eq!(map.hit(on_the_header), TreemapHit::Tile(movies), "a folder's header belongs to the folder");
        let between_bar_and_tiles = ChartPoint { x: 10.0, y: FOCUS_BAR_HEIGHT + 1.0 };
        assert_eq!(map.hit(between_bar_and_tiles), TreemapHit::Nothing);
        assert_eq!(map.hit(ChartPoint { x: SIZE.0 + 5.0, y: 300.0 }), TreemapHit::Nothing);
    }

    #[test]
    fn excluded_items_leave_the_map_and_the_rest_fills_their_space() {
        let t = sample();
        let iso = descend(&t, &["disk.iso"]);
        let map = map_of(&t, t.root(), &[iso]);
        assert_eq!(map.total, t.size(t.root()) - t.size(iso));
        assert!(map.find(Segment::Node(iso)).is_none(), "an excluded file has no tile");
        let top_level_area: f32 = children_of(&map, None).iter().map(|t| t.rect.area()).sum();
        assert!((top_level_area - map.area.area()).abs() < 1.0);
    }

    #[test]
    fn a_search_dims_what_does_not_match_but_keeps_buckets_holding_a_match_lit() {
        let mut kids = vec![D("big", vec![F("x.mov", 10_000_000)])];
        kids.extend((0..50).map(|_| F("noise", 10)));
        kids.push(F("needle", 10));
        let t = tree(kids);
        let s = crate::search::search(&t, "needle", &Default::default()).unwrap();
        let map = treemap(&t, t.root(), SIZE, Some(&s), &Exclusions::default());
        assert!(!map.tiles[0].relevant);
        let bucket = map.tiles.iter().find(|t| matches!(t.segment, Segment::Small { .. })).unwrap();
        assert!(bucket.relevant, "the bucket holding the match stays lit");
    }

    #[test]
    fn nothing_is_tiled_without_room_or_without_bytes() {
        let t = sample();
        assert!(treemap(&t, t.root(), (0.0, 0.0), None, &Exclusions::default()).tiles.is_empty());
        let empty = tree(vec![D("empty", vec![])]);
        assert!(map_of(&empty, empty.root(), &[]).tiles.is_empty());
    }
}
