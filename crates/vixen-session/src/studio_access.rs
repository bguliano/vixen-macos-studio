//! Studio: what VoiceOver reads on the custom canvases, and how the keyboard reaches what they hold (UI roadmap R43).
//!
//! The timeline grid, the patching graph and the preview designer draw their contents themselves, so AppKit knows
//! nothing about the effects, shapes and props in them. Each canvas asks here for one accessibility element per visible
//! item, with the words to read and where it is, and for the keyboard's moves between items. The words are built here
//! once, so VoiceOver, the harness and agents (`sequence_effects`' `label`) all say the same thing.
//!
//! Keys, the same on every canvas: Tab and ⇧Tab select the next and previous item inside the canvas (Keynote's
//! convention for objects on a canvas), ⌃Tab and ⌃⇧Tab leave it (the convention for views that keep Tab). The timeline
//! adds ← and → along the active row (⇧ extends the selection) and ⌥→ and ⌥← to open and close the active row; the
//! inspector edits whatever the keyboard selects, and ⌃Tab reaches it. The graph's Return opens the selected shape's
//! menu, whose Connect To does what a drag from its output port does. Selection is the window's view, not the document,
//! so none of it is on Undo; the edits it leads to (nudge, delete, connect) are undone as they always were.
use crate::setup::graph::{GraphScene, LineKind, ShapeKind};
use crate::timeline::{ElementId, RowId, Timeline};
use vixen_core::time::{TICKS_PER_MIN, TICKS_PER_MS, TICKS_PER_SEC};
use vixen_core::TimeSpan;

/// A time as VoiceOver reads it and the ruler shows it: m:ss.fff.
pub fn format_time(ticks: i64) -> String {
    let negative = ticks < 0;
    let t = ticks.unsigned_abs() as i64;
    let (minutes, rest) = (t / TICKS_PER_MIN, t % TICKS_PER_MIN);
    let (seconds, rest) = (rest / TICKS_PER_SEC, rest % TICKS_PER_SEC);
    let ms = rest / TICKS_PER_MS;
    format!("{}{minutes}:{seconds:02}.{ms:03}", if negative { "-" } else { "" })
}

/// One effect's words: its type, where it shows, when, and its layer when that isn't the default one.
pub fn effect_label(type_name: &str, target: &str, start: TimeSpan, end: TimeSpan, layer: Option<&str>) -> String {
    let type_name = if type_name.is_empty() { "Effect" } else { type_name };
    let mut label = if target.is_empty() { type_name.to_string() } else { format!("{type_name} on {target}") };
    label.push_str(&format!(", {} to {}", format_time(start.ticks()), format_time(end.ticks())));
    if let Some(layer) = layer.filter(|l| !l.is_empty()) {
        label.push_str(&format!(", layer {layer}"));
    }
    label
}

/// A visible effect on the grid, as one accessibility element: `x`, `y`, `w`, `h` in the grid's client coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessEffect {
    pub id: ElementId,
    pub row: RowId,
    pub label: String,
    pub selected: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Timeline {
    fn studio_effect_label(&self, id: ElementId, row: RowId) -> Option<String> {
        let e = self.elements.get(&id)?;
        let layer = (e.desc.layer_level > 0).then(|| e.desc.layer_name.as_ref());
        Some(effect_label(&e.desc.type_name, &self.rows.row(row).name, e.start, e.end(), layer))
    }

    /// One element per effect occurrence at least partly inside the grid's client area, rows top down, then by time.
    pub fn studio_access_effects(&self) -> Vec<AccessEffect> {
        let (scroll_x, top) = (self.scroll_x(), self.vertical_offset);
        let (left, right) = (self.info.visible_start.ticks(), self.vis_end().ticks());
        let mut out = Vec::new();
        for r in self.rows.visible() {
            let Some(row_top) = self.rows.row_top(*r) else { continue };
            let row = self.rows.row(*r);
            if row_top + row.height <= top || row_top >= top + self.grid_height {
                continue;
            }
            for occ in row.occurrences.iter().filter(|o| o.end > left && o.start < right) {
                let Some(rect) = self.occurrence_rect_in_view(occ.id, *r).map(|o| o.rect()) else { continue };
                let Some(label) = self.studio_effect_label(occ.id, *r) else { continue };
                let selected = self.elements.get(&occ.id).is_some_and(|e| e.selected);
                out.push(AccessEffect { id: occ.id, row: *r, label, selected, x: rect.x - scroll_x, y: rect.y - top, w: rect.w, h: rect.h });
            }
        }
        out
    }

    /// The grid's value: where the keyboard is and what is selected.
    pub fn studio_access_summary(&mut self) -> String {
        let selected = self.selected_list();
        let row = self.active_row().map(|r| self.rows.row(r).name.clone());
        let mut parts = vec![match row {
            Some(name) => format!("Row {name}"),
            None => "No active row".to_string(),
        }];
        match selected.len() {
            0 => parts.push("no effect selected".into()),
            1 => {
                let row = self.elements.get(&selected[0]).and_then(|e| e.rows.first().copied());
                parts.extend(row.and_then(|r| self.studio_effect_label(selected[0], r)));
            }
            n => parts.push(format!("{n} effects selected")),
        }
        parts.push(format!("cursor at {}", format_time(self.cursor.ticks())));
        parts.join(", ")
    }

    /// The active row, or the first visible row made active when there is none, so the keyboard always has a row.
    fn studio_keyboard_row(&mut self) -> Option<RowId> {
        if let Some(r) = self.active_row() {
            return Some(r);
        }
        let first = self.rows.row_at(self.vertical_offset).or_else(|| self.rows.visible().first().copied())?;
        self.set_row_active(first, true);
        self.studio_reveal_row(first);
        Some(first)
    }

    /// `EnsureRowVisible`: just enough vertical scroll to show the row.
    fn studio_reveal_row(&mut self, r: RowId) {
        let Some(top) = self.rows.row_top(r) else { return };
        let bottom = top + self.rows.row(r).height;
        if top < self.vertical_offset {
            self.set_vertical_offset(top);
        } else if bottom > self.vertical_offset + self.grid_height {
            self.set_vertical_offset(bottom - self.grid_height);
        }
    }

    /// Just enough horizontal scroll to show the effect's start, and as much of it as fits.
    fn studio_reveal_time(&mut self, start: TimeSpan, end: TimeSpan) {
        let (left, right) = (self.info.visible_start, self.vis_end());
        if start < left {
            self.set_visible_start(start);
        } else if end > right {
            let span = self.span();
            let to = if end - start >= span { start } else { end - span };
            self.set_visible_start(to);
        }
    }

    /// ↑ and ↓ on the grid. Like `move_active_row`, but with no active row the first row in view becomes active, so the
    /// first press after the grid takes focus lands somewhere. False when nothing moved.
    pub fn studio_access_move_row(&mut self, direction: i32) -> bool {
        if self.active_row().is_none() {
            return self.studio_keyboard_row().is_some();
        }
        self.move_active_row(direction)
    }

    /// ←, →, Tab and ⇧Tab: the next or previous effect in the active row, from the last selected effect in it, or from
    /// the cursor when none is. `extend` (⇧ with an arrow) adds it to the selection instead of replacing it. Returns
    /// the new effect's words for VoiceOver, or None at either end of the row.
    pub fn studio_access_step(&mut self, direction: i32, extend: bool) -> Option<String> {
        let r = self.studio_keyboard_row()?;
        let row = self.rows.row(r);
        let occurrences = &row.occurrences;
        let from = row.selected_elements.iter().rev().find_map(|id| occurrences.iter().position(|o| o.id == *id));
        let cursor = self.cursor.ticks();
        let target = match (from, direction > 0) {
            (Some(i), true) => i + 1,
            (Some(i), false) => i.checked_sub(1)?,
            (None, true) => occurrences.iter().position(|o| o.start >= cursor).or_else(|| occurrences.len().checked_sub(1))?,
            (None, false) => occurrences.iter().rposition(|o| o.start < cursor).or_else(|| (!occurrences.is_empty()).then_some(0))?,
        };
        let id = occurrences.get(target)?.id;
        if !extend {
            self.clear_selected_quietly();
        }
        self.select_element(id);
        let (start, end) = self.elements.get(&id).map(|e| (e.start, e.end()))?;
        self.studio_reveal_time(start, end);
        self.studio_reveal_row(r);
        self.studio_effect_label(id, r)
    }

    /// VoiceOver's press on an effect: selects it alone, as a click does, and makes its row active.
    pub fn studio_access_select(&mut self, id: ElementId, row: RowId) -> Option<String> {
        self.elements.get(&id)?;
        self.clear_selected_quietly();
        self.clear_active_rows(Some(row));
        self.set_row_active(row, true);
        self.select_element(id);
        self.studio_effect_label(id, row)
    }

    /// ⌥→ and ⌥←: opens or closes the active row's children. False when it has none or is already so.
    pub fn studio_access_open_row(&mut self, open: bool) -> bool {
        let Some(r) = self.studio_keyboard_row() else { return false };
        let row = self.rows.row(r);
        if row.children.is_empty() || row.tree_open == open {
            return false;
        }
        self.rows.set_tree_open(r, open);
        self.rows.relayout();
        self.rows_changed();
        self.studio_reveal_row(r);
        true
    }

    /// The active row's words, for VoiceOver after a row move.
    pub fn studio_access_row_label(&self) -> Option<String> {
        let r = self.active_row()?;
        let row = self.rows.row(r);
        let mut label = row.name.clone();
        if !row.children.is_empty() {
            label.push_str(if row.tree_open { ", expanded" } else { ", collapsed" });
        }
        let count = row.occurrences.len();
        label.push_str(&match count {
            0 => ", no effects".to_string(),
            1 => ", 1 effect".to_string(),
            n => format!(", {n} effects"),
        });
        Some(label)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The patching graph
// ---------------------------------------------------------------------------------------------------------------------

/// The graph's shapes as VoiceOver reads them and Tab visits them: `ids[i]` reads `labels[i]`, in reading order
/// (columns left to right, then top down; nested shapes after their group). Hidden shapes, inside a collapsed group,
/// are left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphAccess {
    pub ids: Vec<u64>,
    pub labels: Vec<String>,
}

fn kind_word(kind: ShapeKind) -> &'static str {
    match kind {
        ShapeKind::ElementGroup => "group",
        ShapeKind::ElementLeaf => "element",
        ShapeKind::Filter => "filter",
        ShapeKind::Controller => "controller",
        ShapeKind::Output => "output",
    }
}

fn names(scene: &GraphScene, shapes: &mut Vec<u32>) -> String {
    shapes.sort_unstable();
    shapes.dedup();
    let list: Vec<&str> = shapes.iter().filter_map(|i| scene.shapes.get(*i as usize)).map(|s| s.title.as_str()).collect();
    match list.len() {
        0 => String::new(),
        1..=3 => list.join(", "),
        n => format!("{} and {} more", list[..2].join(", "), n - 2),
    }
}

/// Reading order: columns by left edge (shapes within 8 points share a column), then top down.
pub fn graph_order(scene: &GraphScene) -> Vec<u32> {
    let mut order: Vec<u32> = (0..scene.shapes.len() as u32).filter(|i| !scene.shapes[*i as usize].hidden).collect();
    order.sort_by(|a, b| {
        let (a, b) = (&scene.shapes[*a as usize].rect, &scene.shapes[*b as usize].rect);
        let column = |x: f32| (x / 8.0).round() as i64;
        column(a.x).cmp(&column(b.x)).then(a.y.total_cmp(&b.y))
    });
    order
}

/// Each visible shape's words: title, kind, collapsed state, and what it takes from and feeds.
pub fn graph_access(scene: &GraphScene) -> GraphAccess {
    let order = graph_order(scene);
    let mut out = GraphAccess::default();
    for i in order {
        let shape = &scene.shapes[i as usize];
        let mut label = format!("{}, {}", shape.title, kind_word(shape.kind));
        if shape.expandable {
            label.push_str(if shape.collapsed { ", collapsed" } else { ", expanded" });
        }
        let (mut from, mut to) = (Vec::new(), Vec::new());
        let mut stubs = (0, 0);
        for line in &scene.lines {
            if line.to_shape == Some(i) {
                match (line.kind, line.from_shape) {
                    (LineKind::Real, Some(s)) => from.push(s),
                    _ => stubs.0 += 1,
                }
            }
            if line.from_shape == Some(i) {
                match (line.kind, line.to_shape) {
                    (LineKind::Real, Some(s)) => to.push(s),
                    _ => stubs.1 += 1,
                }
            }
        }
        let (from, to) = (names(scene, &mut from), names(scene, &mut to));
        if !from.is_empty() {
            label.push_str(&format!(", from {from}"));
        } else if stubs.0 > 0 {
            label.push_str(", from a source not shown");
        } else if shape.inputs > 0 && matches!(shape.kind, ShapeKind::Filter | ShapeKind::Output) {
            label.push_str(", not connected");
        }
        if !to.is_empty() {
            label.push_str(&format!(", to {to}"));
        } else if stubs.1 > 0 {
            label.push_str(", to outputs not shown");
        }
        if shape.outputs > 1 {
            label.push_str(&format!(", {} outputs", shape.outputs));
        }
        out.ids.push(shape.id);
        out.labels.push(label);
    }
    out
}

/// The shapes a connection from `source` can go to, in reading order: what "Connect To" lists for the keyboard.
pub fn graph_connect_targets(scene: &GraphScene, source: u64, candidate: impl Fn(crate::setup::flow::Cid, crate::setup::flow::Cid) -> bool) -> Vec<u64> {
    let Some(from) = scene.shapes.iter().find(|s| s.id == source).and_then(|s| s.component) else { return Vec::new() };
    graph_order(scene)
        .into_iter()
        .map(|i| &scene.shapes[i as usize])
        .filter(|s| s.inputs > 0 && s.component.is_some_and(|to| candidate(from, to)))
        .map(|s| s.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::graph::{GraphLine, GraphShape, Rect};
    use crate::setup::flow::Cid;

    #[test]
    fn times_read_as_minutes_seconds_and_milliseconds() {
        assert_eq!(format_time(0), "0:00.000");
        assert_eq!(format_time(TimeSpan::from_ms_i(62_505).ticks()), "1:02.505");
        assert_eq!(format_time(-TimeSpan::from_ms_i(1_500).ticks()), "-0:01.500");
    }

    #[test]
    fn an_effect_reads_its_type_target_times_and_layer() {
        let (start, end) = (TimeSpan::from_ms_i(12_500), TimeSpan::from_ms_i(13_000));
        assert_eq!(effect_label("Pulse", "Mega Tree", start, end, None), "Pulse on Mega Tree, 0:12.500 to 0:13.000");
        assert_eq!(effect_label("Pulse", "Mega Tree", start, end, Some("Top")), "Pulse on Mega Tree, 0:12.500 to 0:13.000, layer Top");
        assert_eq!(effect_label("", "", start, end, Some("")), "Effect, 0:12.500 to 0:13.000");
    }

    use crate::timeline::model::ElementDescription;
    use crate::timeline::ElementTimes;
    use std::sync::Arc;
    use vixen_core::ElementTree;

    /// Rows "Roof", "Tree" (holding "Star") and "Yard" at 20 points, 10 ms per pixel, a 1000 by 400 grid (10 s in
    /// view). Roof has effects at 1-2 s, 5-6 s and 50-51 s (off screen); Yard has none.
    fn grid() -> Timeline {
        let mut tree = ElementTree::new();
        let roof = tree.add_element("Roof");
        let group = tree.add_node(uuid::Uuid::new_v4(), "Tree", None);
        let star = tree.add_element("Star");
        tree.add_child(group, star);
        tree.add_element("Yard");
        let mut t = Timeline::new();
        t.info.time_per_pixel = TimeSpan::from_ms_i(10);
        t.info.total = TimeSpan::from_ms_i(60_000);
        t.rows.load_rows(&tree, &[], 20);
        t.set_viewport(1000, 400, 1000);
        let desc = ElementDescription { type_id: uuid::Uuid::nil(), type_name: Arc::from("Pulse"), layer_level: 0, layer_name: Arc::from("Default") };
        for (id, (a, b)) in [(1, (1_000, 2_000)), (2, (5_000, 6_000)), (3, (50_000, 51_000))] {
            t.add_element(ElementId(id), ElementTimes::new(TimeSpan::from_ms_i(a), TimeSpan::from_ms_i(b - a)), Some(roof), desc.clone());
        }
        t.flush_rows();
        t.take_outcome();
        t
    }

    #[test]
    fn effects_in_view_read_their_type_row_and_times_where_they_are() {
        let t = grid();
        let effects = t.studio_access_effects();
        assert_eq!(effects.len(), 2, "the effect at 50 s is out of view");
        assert_eq!(effects[0].label, "Pulse on Roof, 0:01.000 to 0:02.000");
        assert_eq!((effects[0].x, effects[0].y, effects[0].w), (100, 0, 100));
        assert_eq!(effects[1].x, 500);
        assert!(effects.iter().all(|e| !e.selected));
    }

    #[test]
    fn arrows_step_through_the_active_row_and_scroll_to_what_they_select() {
        let mut t = grid();
        assert_eq!(t.active_row(), None);
        // With no active row the first row in view becomes active, and → picks its first effect at or after the cursor.
        assert_eq!(t.studio_access_step(1, false).as_deref(), Some("Pulse on Roof, 0:01.000 to 0:02.000"));
        let roof = t.active_row().unwrap();
        assert_eq!(t.rows.row(roof).name, "Roof");
        assert_eq!(t.selected_list(), vec![ElementId(1)]);
        assert_eq!(t.studio_access_step(1, false).as_deref(), Some("Pulse on Roof, 0:05.000 to 0:06.000"));
        // ⇧→ extends; the third effect is off screen, so the grid scrolls to show it.
        assert!(t.studio_access_step(1, true).is_some());
        assert_eq!(t.selected_list(), vec![ElementId(2), ElementId(3)]);
        assert!(t.info.visible_start <= TimeSpan::from_ms_i(50_000) && t.vis_end() >= TimeSpan::from_ms_i(51_000));
        assert_eq!(t.studio_access_step(1, false), None, "no wrap past the end");
        assert_eq!(t.studio_access_step(-1, false).as_deref(), Some("Pulse on Roof, 0:05.000 to 0:06.000"));
        assert_eq!(t.selected_list(), vec![ElementId(2)]);
        assert!(t.studio_access_summary().starts_with("Row Roof, Pulse on Roof, 0:05.000"));
    }

    #[test]
    fn rows_move_open_and_read_out_from_the_keyboard() {
        let mut t = grid();
        assert!(t.studio_access_move_row(1), "the first press gives the grid a row");
        assert_eq!(t.studio_access_row_label().as_deref(), Some("Roof, 3 effects"));
        assert!(t.studio_access_move_row(1));
        assert_eq!(t.studio_access_row_label().as_deref(), Some("Tree, collapsed, no effects"));
        assert!(t.studio_access_open_row(true));
        assert!(!t.studio_access_open_row(true), "already open");
        assert!(t.studio_access_move_row(1));
        assert_eq!(t.studio_access_row_label().as_deref(), Some("Star, no effects"), "its child is next once it's open");
        assert!(!t.studio_access_open_row(true), "a row with no children doesn't open");
        // An empty row has nothing to step to.
        assert_eq!(t.studio_access_step(1, false), None);
        // VoiceOver's press selects that effect alone and makes its row active.
        let roof = t.rows.visible()[0];
        assert!(t.studio_access_select(ElementId(2), roof).is_some());
        assert_eq!((t.active_row(), t.selected_list()), (Some(roof), vec![ElementId(2)]));
    }

    fn shape(id: u64, kind: ShapeKind, title: &str, x: f32, y: f32, component: Option<u32>) -> GraphShape {
        GraphShape {
            id,
            kind,
            title: title.into(),
            rect: Rect { x, y, w: 100.0, h: 20.0 },
            z: 1,
            hidden: false,
            inputs: u32::from(!matches!(kind, ShapeKind::ElementLeaf | ShapeKind::ElementGroup)),
            outputs: 1,
            component: component.map(Cid),
            parent: None,
            expandable: false,
            collapsed: false,
        }
    }

    fn line(from: Option<u32>, to: Option<u32>, kind: LineKind) -> GraphLine {
        GraphLine { id: 0, kind, from_shape: from, from_port: 0, to_shape: to, x1: 0.0, y1: 0.0, x2: 0.0, y2: 0.0, destination: None }
    }

    #[test]
    fn graph_shapes_read_in_columns_with_their_connections() {
        let scene = GraphScene {
            shapes: vec![
                shape(30, ShapeKind::Output, "Output 1", 400.0, 0.0, Some(3)),
                shape(10, ShapeKind::ElementLeaf, "Arch 1", 0.0, 30.0, Some(1)),
                shape(11, ShapeKind::ElementLeaf, "Arch 2", 3.0, 0.0, Some(2)),
                shape(20, ShapeKind::Filter, "Color Breakdown", 200.0, 0.0, Some(4)),
            ],
            lines: vec![line(Some(1), Some(3), LineKind::Real), line(Some(3), Some(0), LineKind::Real), line(Some(2), None, LineKind::DestinationStub)],
            content_width: 500.0,
            content_height: 60.0,
        };
        let access = graph_access(&scene);
        assert_eq!(access.ids, vec![11, 10, 20, 30], "column by column, top down");
        assert_eq!(access.labels[0], "Arch 2, element, to outputs not shown");
        assert_eq!(access.labels[1], "Arch 1, element, to Color Breakdown");
        assert_eq!(access.labels[2], "Color Breakdown, filter, from Arch 1, to Output 1");
        assert_eq!(access.labels[3], "Output 1, output, from Color Breakdown");
        // Elements take no input, so only the filter and the output are offered, and never the source itself.
        let targets = graph_connect_targets(&scene, 10, |from, to| from != to);
        assert_eq!(targets, vec![20, 30]);
    }
}
