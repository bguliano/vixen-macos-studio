//! Studio: quieter marks (UI roadmap R35). Marks draw as coloured ticks in the ruler and waveform. Lines cross the grid
//! only for the collections whose marks are selected (click a mark in the ruler or the marks bar), and for every
//! collection with lines while an effect drag snaps, so the marks it can snap to are in view. Nothing here changes the
//! document: which lines show is the window's view, as zoom is.
use crate::timeline::ruler::RulerMarkLine;
use crate::timeline::snap::{mark_lines, MarkLine, MarkSnapDetail};
use crate::timeline::view::{px, time_at};
use crate::timeline::{IRect, Timeline};
use uuid::Uuid;
use vixen_core::{Color, TimeSpan};

impl Timeline {
    /// The grid tiles' lines: only the selected marks' collections', restyled from their details at each time. Marks being
    /// dragged are left out, as `without_moving_marks` does.
    pub fn studio_tile_mark_lines(&self, lines: Vec<MarkLine>) -> Vec<MarkLine> {
        if self.marks.selected.is_empty() {
            return Vec::new();
        }
        self.studio_restyle(lines, true)
    }

    /// After the mark selection changed: repaints the grid when that changed which collections cross it.
    pub(crate) fn studio_refresh_lit_collections(&mut self) {
        let lit = self.studio_lit();
        if lit != self.studio_lit_collections {
            self.studio_lit_collections = lit;
            self.dirty.grid_all = true;
        }
    }

    /// The collections with a selected mark, sorted.
    fn studio_lit(&self) -> Vec<Uuid> {
        let mut lit: Vec<Uuid> = self.marks.selected.iter().filter_map(|id| self.marks.collection_of(*id)).map(|c| c.id).collect();
        lit.sort_unstable();
        lit.dedup();
        lit
    }

    /// While an effect drag snaps: the other collections' lines over `cover`, which the tiles leave out. Empty otherwise.
    pub fn studio_snapping_mark_lines(&self, cover: IRect) -> Vec<MarkLine> {
        if self.g.drag_snap.is_none() {
            return Vec::new();
        }
        let tpp = self.tpp();
        let lines = mark_lines(&self.snap_index, time_at(cover.x - 3, tpp), time_at(cover.right() + 3, tpp), &self.guides, tpp);
        self.studio_restyle(lines, false)
    }

    /// Ticks for the waveform over a span of columns: the same marks, colours and selection the ruler shows.
    pub fn studio_mark_ticks(&self, x: i32, width: i32, tpp: TimeSpan) -> Vec<RulerMarkLine> {
        let (t0, t1) = (time_at(x - 3, tpp), time_at(x + width + 3, tpp));
        let mut out = Vec::new();
        for c in &self.marks.collections {
            if !(c.show_grid_lines || c.show_marks) {
                continue;
            }
            let from = c.marks.partition_point(|m| m.start_time < t0);
            for m in c.marks[from..].iter().take_while(|m| m.start_time <= t1) {
                out.push(RulerMarkLine { x: px(m.start_time, tpp), color: c.decorator.color, width: if self.marks.is_selected(m.id) { 3 } else { 1 }, dash: 0 });
            }
        }
        out
    }

    /// Keeps the lines with a detail on the wanted side (a selected mark's collection or not), choosing among those details
    /// as `mark_lines` does: the first, unless a later one has a higher level and a colour.
    fn studio_restyle(&self, lines: Vec<MarkLine>, lit: bool) -> Vec<MarkLine> {
        let collections = self.studio_lit();
        let is_lit = |mark: Uuid| self.marks.collection_of(mark).is_some_and(|c| collections.binary_search(&c.id).is_ok());
        lines
            .into_iter()
            .filter_map(|line| {
                let details = self.snap_index.details_at(line.time);
                let rest: Vec<&MarkSnapDetail> = details.iter().filter(|d| !self.marks.is_moving(d.mark) && is_lit(d.mark) == lit).collect();
                let mut chosen = *rest.first()?;
                for d in &rest {
                    if d.level > chosen.level && d.color != Color::EMPTY {
                        chosen = d;
                    }
                }
                Some(MarkLine { color: chosen.color, width: if chosen.bold { 3 } else { 1 }, dash: if chosen.solid { 0 } else { chosen.level.max(1) }, ..line })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::timeline::{IRect, Timeline};
    use vixen_core::marks::{Mark, MarkCollection};
    use vixen_core::{Color, TimeSpan};

    fn collection(name: &str, color: Color, seconds: &[f64]) -> MarkCollection {
        let mut c = MarkCollection::default();
        c.name = name.into();
        c.show_grid_lines = true;
        c.decorator.color = color;
        c.marks = seconds.iter().map(|s| Mark::new(TimeSpan::from_ms(s * 1000.0))).collect();
        c
    }

    fn timeline() -> Timeline {
        let mut t = Timeline::new();
        t.info.total = TimeSpan::from_ms(20_000.0);
        t.load_marks(vec![collection("Beats", Color::rgb(255, 0, 0), &[1.0, 2.0, 3.0, 4.0]), collection("Bars", Color::rgb(0, 0, 255), &[1.0, 5.0])]);
        t
    }

    #[test]
    fn tiles_draw_only_the_selected_marks_collections() {
        let mut t = timeline();
        let all = |t: &Timeline| crate::timeline::snap::mark_lines(&t.snap_index, TimeSpan::ZERO, TimeSpan::from_ms(20_000.0), &[], t.tpp());
        assert_eq!(all(&t).len(), 5, "beats at 1-4 and a bar at 5, sharing 1");
        assert!(t.studio_tile_mark_lines(all(&t)).is_empty(), "nothing selected: no lines across the grid");

        t.dirty.grid_all = false;
        let bar = t.marks.collections[1].marks[1].id;
        t.marks_select(bar);
        assert!(t.dirty.grid_all, "selecting into a new collection repaints the grid");
        let tiles = t.studio_tile_mark_lines(all(&t));
        let times: Vec<f64> = tiles.iter().map(|l| l.time.total_ms()).collect();
        assert_eq!(times, vec![1000.0, 5000.0], "every mark of the bar's collection");
        assert!(tiles.iter().all(|l| l.color == Color::rgb(0, 0, 255)), "the shared time takes the bar's colour");

        t.dirty.grid_all = false;
        t.marks_select(t.marks.collections[1].marks[0].id);
        assert!(!t.dirty.grid_all, "another mark of the same collection repaints no tile");
        t.marks_clear_selected();
        assert!(t.dirty.grid_all && t.studio_tile_mark_lines(all(&t)).is_empty());
    }

    #[test]
    fn snapping_shows_the_rest_only_during_a_snapping_drag() {
        let mut t = timeline();
        let cover = IRect::new(-100, 0, 100_000, 100);
        assert!(t.studio_snapping_mark_lines(cover).is_empty(), "no drag, no lines");
        t.g.drag_snap = Some(Default::default());
        let times: Vec<f64> = t.studio_snapping_mark_lines(cover).iter().map(|l| l.time.total_ms()).collect();
        assert_eq!(times, vec![1000.0, 2000.0, 3000.0, 4000.0, 5000.0], "with nothing selected, every line the tiles leave out");
        t.marks_select(t.marks.collections[1].marks[0].id);
        let times: Vec<f64> = t.studio_snapping_mark_lines(cover).iter().map(|l| l.time.total_ms()).collect();
        assert_eq!(times, vec![1000.0, 2000.0, 3000.0, 4000.0], "the bars are in the tiles already");
    }

    #[test]
    fn ticks_follow_visibility_and_selection() {
        let mut t = timeline();
        let tpp = t.tpp();
        assert_eq!(t.studio_mark_ticks(-10, 100_000, tpp).len(), 6);
        t.marks.collections[0].show_grid_lines = false;
        t.marks.collections[0].show_marks = false;
        let ticks = t.studio_mark_ticks(-10, 100_000, tpp);
        assert_eq!(ticks.len(), 2, "a hidden collection draws no ticks");
        assert!(ticks.iter().all(|l| l.dash == 0 && l.width == 1));
    }
}
