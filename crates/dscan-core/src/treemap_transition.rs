use std::sync::Arc;

use crate::layout::Segment;
use crate::transition::{SegmentStyle, StaggeredReveal, TransitionKind, reveal_scale, total_at};
use crate::treemap::{Tile, TileRect, Treemap};

const DEEPEST_STAGGERED_DEPTH: usize = 5;
const OPAQUE: f32 = 1.0;
const SMALLEST_VISIBLE_SIDE: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileLook {
    pub style: SegmentStyle,
    pub depth: usize,
    pub holds_tiles: bool,
}

impl TileLook {
    pub fn of(tile: &Tile) -> Self {
        TileLook {
            style: SegmentStyle { kind: tile.kind, branch: tile.branch, relevant: tile.relevant },
            depth: tile.depth,
            holds_tiles: tile.holds_tiles(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameTile {
    pub rect: TileRect,
    pub look: TileLook,
    pub opacity: f32,
}

impl FrameTile {
    fn of(tile: &Tile, rect: TileRect, opacity: f32) -> Self {
        FrameTile { rect, look: TileLook::of(tile), opacity }
    }

    fn is_visible(&self) -> bool {
        self.rect.width >= SMALLEST_VISIBLE_SIDE && self.rect.height >= SMALLEST_VISIBLE_SIDE && self.opacity > 0.0
    }
}

fn folders_painted_before_files(tiles: impl Iterator<Item = FrameTile>) -> Vec<FrameTile> {
    let (mut painted, files): (Vec<FrameTile>, Vec<FrameTile>) = tiles.partition(|t| t.look.holds_tiles);
    painted.extend(files);
    painted
}

pub fn tiles_at_rest(map: &Treemap) -> Vec<FrameTile> {
    folders_painted_before_files(map.tiles.iter().map(|t| FrameTile::of(t, t.rect, OPAQUE)))
}

#[derive(Clone, Debug)]
pub struct TreemapTransition {
    motion: Motion,
}

#[derive(Clone, Copy, Debug)]
struct Zoom {
    folder_contents: TileRect,
    inward: bool,
}

#[derive(Clone, Debug)]
enum Motion {
    Reveal(Arc<Treemap>),
    Crossfade { from: Arc<Treemap>, to: Arc<Treemap>, zoom: Option<Zoom> },
}

fn contents_of(folder: &TileRect) -> TileRect {
    folder.framed_contents().unwrap_or(*folder)
}

impl TreemapTransition {
    pub fn reveal(map: &Arc<Treemap>) -> Self {
        TreemapTransition { motion: Motion::Reveal(Arc::clone(map)) }
    }

    pub fn between(from: &Arc<Treemap>, to: &Arc<Treemap>) -> Self {
        let zoom = if from.focus == to.focus {
            None
        } else if let Some(folder) = from.find(Segment::Node(to.focus)) {
            Some(Zoom { folder_contents: contents_of(&folder.rect), inward: true })
        } else if let Some(folder) = to.find(Segment::Node(from.focus)) {
            Some(Zoom { folder_contents: contents_of(&folder.rect), inward: false })
        } else {
            return Self::reveal(to);
        };
        TreemapTransition { motion: Motion::Crossfade { from: Arc::clone(from), to: Arc::clone(to), zoom } }
    }

    pub fn kind(&self) -> TransitionKind {
        match &self.motion {
            Motion::Reveal(_) => TransitionKind::Reveal,
            Motion::Crossfade { zoom: None, .. } => TransitionKind::Morph,
            Motion::Crossfade { zoom: Some(zoom), .. } if zoom.inward => TransitionKind::ZoomIn,
            Motion::Crossfade { zoom: Some(_), .. } => TransitionKind::ZoomOut,
        }
    }

    pub fn total(&self, progress: f64) -> u64 {
        let (before, after) = match &self.motion {
            Motion::Reveal(map) => (0, map.total),
            Motion::Crossfade { from, to, .. } => (from.total, to.total),
        };
        total_at(before, after, progress)
    }

    pub fn frame(&self, progress: f64) -> Vec<FrameTile> {
        let progress = progress.clamp(0.0, 1.0);
        let (mut tiles, area) = match &self.motion {
            Motion::Reveal(map) => (reveal_frame(map, progress), map.area),
            Motion::Crossfade { from, to, zoom } => (crossfade_frame(from, to, *zoom, progress as f32), to.area),
        };
        for tile in &mut tiles {
            tile.rect = tile.rect.clipped_to(&area);
        }
        tiles.retain(FrameTile::is_visible);
        tiles
    }
}

struct Layer<'a> {
    map: &'a Treemap,
    seen_through: TileRect,
    shown_in: TileRect,
}

impl Layer<'_> {
    fn tiles(&self, opacity: f32) -> Vec<FrameTile> {
        let place = |t: &Tile| FrameTile::of(t, t.rect.mapped(&self.seen_through, &self.shown_in), opacity);
        folders_painted_before_files(self.map.tiles.iter().map(place))
    }
}

fn crossfade_frame(from: &Treemap, to: &Treemap, zoom: Option<Zoom>, t: f32) -> Vec<FrameTile> {
    let area = to.area;
    let ((under, over), over_opacity) = match zoom {
        None => {
            let unmoved = |map| Layer { map, seen_through: area, shown_in: area };
            ((unmoved(from), unmoved(to)), t)
        }
        Some(Zoom { folder_contents, inward }) => {
            let (outer, inner, zoomed_in) = if inward { (from, to, t) } else { (to, from, 1.0 - t) };
            let window = area.lerp(&folder_contents, zoomed_in);
            let outer_layer = Layer { map: outer, seen_through: window, shown_in: area };
            let inner_layer =
                Layer { map: inner, seen_through: area, shown_in: folder_contents.mapped(&window, &area) };
            ((outer_layer, inner_layer), zoomed_in)
        }
    };
    let over_covers_the_area = !over.map.tiles.is_empty();
    let under_opacity = if over_covers_the_area { OPAQUE } else { OPAQUE - over_opacity };
    let under_is_hidden = over_covers_the_area && over_opacity >= OPAQUE;
    let mut tiles = if under_is_hidden { Vec::new() } else { under.tiles(under_opacity) };
    tiles.extend(over.tiles(over_opacity));
    tiles
}

fn reveal_frame(map: &Treemap, t: f64) -> Vec<FrameTile> {
    let layer = |tile: &Tile| tile.depth.min(DEEPEST_STAGGERED_DEPTH);
    let stagger = StaggeredReveal::up_to(map.tiles.iter().map(layer).max().unwrap_or(0));
    let area_now = map.area.scaled_about_center(reveal_scale(t) as f32);
    let mut rects: Vec<TileRect> = Vec::with_capacity(map.tiles.len());
    for tile in &map.tiles {
        let grown = stagger.progress(t, layer(tile)) as f32;
        let (parent_at_rest, parent_now) = match tile.parent {
            Some(p) => (map.tiles[p].rect, rects[p]),
            None => (map.area, area_now),
        };
        rects.push(tile.rect.mapped(&parent_at_rest, &parent_now).scaled_about_center(grown));
    }
    folders_painted_before_files(map.tiles.iter().zip(rects).map(|(tile, rect)| FrameTile::of(tile, rect, OPAQUE)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::Kind;
    use crate::tree::test_util::{T::*, descend, tree};
    use crate::tree::{NodeId, Tree};

    fn map_of(t: &Tree, focus: NodeId, excluded: &[NodeId]) -> Arc<Treemap> {
        Arc::new(crate::treemap::tests::map_of(t, focus, excluded))
    }

    const SAME_PLACE: f32 = 1e-2;

    fn sample() -> Tree {
        tree(vec![
            D(
                "a",
                vec![F("x.mov", 600_000), D("deep", vec![F("y.mov", 150_000), F("z.rs", 50_000)]), F("w.png", 100_000)],
            ),
            D("b", vec![F("v.png", 500_000)]),
            F("note.txt", 100_000),
        ])
    }

    fn same_rect(a: &TileRect, b: &TileRect) -> bool {
        [(a.x, b.x), (a.y, b.y), (a.width, b.width), (a.height, b.height)]
            .iter()
            .all(|(p, q)| (p - q).abs() < SAME_PLACE)
    }

    fn assert_same_picture(actual: &[FrameTile], at_rest: &Treemap, what: &str) {
        assert!(actual.iter().all(|t| t.opacity == OPAQUE), "{what}: nothing is half faded");
        let mut unmatched: Vec<TileRect> = actual.iter().map(|t| t.rect).collect();
        for expected in tiles_at_rest(at_rest) {
            let found = unmatched.iter().position(|a| same_rect(a, &expected.rect));
            let i = found.unwrap_or_else(|| panic!("{what}: nothing drawn at {expected:?}\n{unmatched:#?}"));
            unmatched.swap_remove(i);
        }
        assert!(unmatched.is_empty(), "{what}: drawn where nothing rests: {unmatched:#?}");
    }

    fn rect_of(map: &Treemap, id: NodeId) -> TileRect {
        map.find(Segment::Node(id)).unwrap().rect
    }

    fn fading(frame: &[FrameTile]) -> impl Iterator<Item = &FrameTile> {
        frame.iter().filter(|f| f.opacity > 0.0 && f.opacity < OPAQUE)
    }

    #[test]
    fn reveal_grows_the_tiles_out_of_their_folders_and_counts_the_total_up() {
        let t = sample();
        let map = map_of(&t, t.root(), &[]);
        let reveal = TreemapTransition::reveal(&map);
        assert_eq!(reveal.kind(), TransitionKind::Reveal);
        assert!(reveal.frame(0.0).is_empty(), "nothing is drawn before the reveal starts");

        let halfway = reveal.frame(0.5);
        let area_now = map.area.scaled_about_center(reveal_scale(0.5) as f32);
        for tile in &halfway {
            assert!(tile.rect.area() < map.area.area(), "halfway every tile is still growing");
            let center = tile.rect.center();
            assert!(area_now.contains(center), "tiles grow inside the area");
        }
        assert_eq!(reveal.total(0.0), 0);
        assert_eq!(reveal.total(1.0), map.total);
        assert_same_picture(&reveal.frame(1.0), &map, "a finished reveal is the treemap");
    }

    #[test]
    fn zooming_into_a_folder_grows_it_into_the_whole_area_while_its_own_treemap_fades_in() {
        let t = sample();
        let a = descend(&t, &["a"]);
        let (outer, inner) = (map_of(&t, t.root(), &[]), map_of(&t, a, &[]));
        let zoom = TreemapTransition::between(&outer, &inner);
        assert_eq!(zoom.kind(), TransitionKind::ZoomIn);
        assert_same_picture(&zoom.frame(0.0), &outer, "the zoom starts from the current treemap");
        assert_same_picture(&zoom.frame(1.0), &inner, "the zoom ends on the folder's treemap");

        let x = descend(&t, &["a", "x.mov"]);
        let (x_before, x_after) = (rect_of(&outer, x).area(), rect_of(&inner, x).area());
        let halfway = zoom.frame(0.5);
        let x_fading_in =
            fading(&halfway).find(|f| f.look.style.kind == Kind::Video && f.look.depth == 0 && !f.look.holds_tiles);
        assert!(
            x_fading_in.is_some_and(|f| f.rect.area() > x_before && f.rect.area() < x_after),
            "the folder's own treemap fades in on its way to full size: {x_fading_in:?}"
        );
        let a_growing = halfway.iter().find(|f| f.opacity == OPAQUE && f.look.holds_tiles && f.look.depth == 0);
        let first_fading = halfway.iter().position(|f| f.opacity < OPAQUE).unwrap();
        assert!(
            halfway[..first_fading].iter().any(|f| Some(f) == a_growing),
            "the current treemap is painted under the one fading in"
        );
    }

    #[test]
    fn going_up_shrinks_the_folder_s_treemap_back_into_its_tile() {
        let t = sample();
        let a = descend(&t, &["a"]);
        let (outer, inner) = (map_of(&t, t.root(), &[]), map_of(&t, a, &[]));
        let zoom = TreemapTransition::between(&inner, &outer);
        assert_eq!(zoom.kind(), TransitionKind::ZoomOut);
        assert_same_picture(&zoom.frame(0.0), &inner, "going up starts from the folder's treemap");
        assert_same_picture(&zoom.frame(1.0), &outer, "going up ends on the parent's treemap");
        let halfway = zoom.frame(0.5);
        let a_contents = contents_of(&rect_of(&outer, a));
        assert!(
            fading(&halfway).all(|f| f.rect.area() <= outer.area.area() && f.rect.area() > 0.0),
            "the folder's treemap fades out while it shrinks"
        );
        assert!(fading(&halfway).any(|f| f.rect.area() > a_contents.area() / 4.0));
    }

    #[test]
    fn folder_backdrops_are_painted_before_any_file_so_the_renderer_batches_them() {
        let t = sample();
        let at_rest = tiles_at_rest(&map_of(&t, t.root(), &[]));
        let first_file = at_rest.iter().position(|f| !f.look.holds_tiles).unwrap();
        assert!(first_file > 0, "the sample holds folders");
        assert!(at_rest[first_file..].iter().all(|f| !f.look.holds_tiles), "no folder is painted over a file");
    }

    #[test]
    fn going_up_from_a_folder_too_thin_for_a_frame_still_draws_every_frame() {
        let t = sample();
        let a = descend(&t, &["a"]);
        let (outer, inner) = (map_of(&t, t.root(), &[]), map_of(&t, a, &[]));
        let thin_folder = TileRect::new(100.0, 100.0, 4.0, 20.0);
        let zoom = Some(Zoom { folder_contents: contents_of(&thin_folder), inward: false });
        for t in [0.0, 0.5, 1.0] {
            let frame = crossfade_frame(&inner, &outer, zoom, t);
            assert!(
                frame.iter().all(|f| f.rect.x.is_finite() && f.rect.width.is_finite()),
                "at {t} no tile is placed at infinity: {frame:?}"
            );
        }
    }

    #[test]
    fn jumping_between_unrelated_folders_reveals_the_new_treemap() {
        let t = sample();
        let (a, b) = (descend(&t, &["a"]), descend(&t, &["b"]));
        let jump = TreemapTransition::between(&map_of(&t, a, &[]), &map_of(&t, b, &[]));
        assert_eq!(jump.kind(), TransitionKind::Reveal);
    }

    #[test]
    fn collecting_a_file_fades_in_the_treemap_without_it_over_the_old_one() {
        let t = sample();
        let x = descend(&t, &["a", "x.mov"]);
        let (before, after) = (map_of(&t, t.root(), &[]), map_of(&t, t.root(), &[x]));
        let morph = TreemapTransition::between(&before, &after);
        assert_eq!(morph.kind(), TransitionKind::Morph);
        assert_same_picture(&morph.frame(0.0), &before, "the morph starts from the current treemap");
        assert_same_picture(&morph.frame(1.0), &after, "the morph ends without the collected file");
        assert_eq!((morph.total(0.0), morph.total(1.0)), (before.total, after.total));

        let halfway = morph.frame(0.5);
        let x_before = rect_of(&before, x);
        assert!(halfway.iter().any(|f| f.opacity == OPAQUE && same_rect(&f.rect, &x_before)), "the old picture stays");
        assert_eq!(fading(&halfway).count(), after.tiles.len(), "the new picture is half faded in");
    }

    #[test]
    fn collecting_everything_fades_the_old_treemap_out() {
        let t = tree(vec![F("only.bin", 1000)]);
        let only = descend(&t, &["only.bin"]);
        let morph = TreemapTransition::between(&map_of(&t, t.root(), &[]), &map_of(&t, t.root(), &[only]));
        assert!(morph.frame(0.5).iter().all(|f| f.opacity < OPAQUE), "{:?}", morph.frame(0.5));
        assert!(morph.frame(1.0).is_empty(), "nothing is left at the end");
    }

    #[test]
    fn uncollecting_fades_the_item_back_in() {
        let t = sample();
        let x = descend(&t, &["a", "x.mov"]);
        let (with_x_collected, restored) = (map_of(&t, t.root(), &[x]), map_of(&t, t.root(), &[]));
        let morph = TreemapTransition::between(&with_x_collected, &restored);
        assert_same_picture(&morph.frame(0.0), &with_x_collected, "starts without the file");
        assert_same_picture(&morph.frame(1.0), &restored, "ends with the file back");
    }
}
