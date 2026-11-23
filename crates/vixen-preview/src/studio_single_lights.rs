//! Studio: single bulbs, one per element (B60). Drawing with the Single Light tool while several elements, or a group, are selected
//! in the designer's element tree places one bulb per light, each linked to its light and named after it, laid out in a row from
//! where the mouse went down, as one Undo step. Upstream's tool links a bulb only when exactly one light is selected and leaves a
//! group's bulb unlinked. `studio_props` in vixen-session runs the same placement for MCP and the Single Bulbs sheet.
//! A child module of `session`, so it reaches the session's undo stack without widening it.
use super::{Change, DesignSession, Tool, UndoAction};
use crate::data::ShapeId;
use crate::point::PreviewPoint;
use crate::shape::{Sel, Shape};
use crate::tools::to_int32;
use std::collections::HashSet;
use uuid::Uuid;
use vixen_core::ElementTree;

/// The most bulbs one placement makes, as the templates' limit on lights.
pub const MAX_SINGLE_LIGHTS: usize = 4096;
/// Space between neighbouring bulbs, in preview units.
pub const SINGLE_LIGHT_SPACING: i32 = 40;
/// The bulbs' size: larger than a pixel string's lights, as a lamp or ceiling bulb is one light seen on its own.
pub const SINGLE_LIGHT_SIZE: i32 = 8;

/// The lights under these elements, each once, in the element tree's order (so a selection made in any click order lays out as
/// the tree reads): a light is itself, a group gives every light under it.
pub fn single_light_targets(tree: &ElementTree, nodes: &[Uuid]) -> Vec<Uuid> {
    let mut wanted = HashSet::new();
    for id in nodes {
        let Some(node) = tree.find(id) else { continue };
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if tree.is_leaf(n) {
                wanted.insert(tree.node(n).id);
            } else {
                stack.extend(tree.node(n).children.iter().copied());
            }
        }
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut groups = HashSet::new();
    let mut stack: Vec<_> = tree.roots.iter().rev().copied().collect();
    while let Some(n) = stack.pop() {
        if tree.is_leaf(n) {
            let uuid = tree.node(n).id;
            if wanted.contains(&uuid) && seen.insert(uuid) {
                out.push(uuid);
            }
        } else if groups.insert(n) {
            stack.extend(tree.node(n).children.iter().rev());
        }
    }
    out
}

/// Where a row of `count` new bulbs goes, in preview coordinates: right of the preview's props when the row fits before
/// `width_limit`, otherwise under them, so new bulbs never land on what's already there.
pub fn free_row_origin(data: &crate::data::PreviewData, count: usize, width_limit: i32) -> (i32, i32) {
    let pixels: Vec<_> = data.display_items.iter().flat_map(|i| i.shape.pixels_vec()).collect();
    let row = count as i32 * SINGLE_LIGHT_SPACING;
    match (pixels.iter().map(|p| p.x).max(), pixels.iter().map(|p| p.y).max()) {
        (Some(right), _) if right + 40 + row <= width_limit => (right + 40, 40),
        (Some(_), Some(bottom)) => (40, bottom + 40),
        _ => (40, 40),
    }
}

impl DesignSession {
    /// Whether the Single Light tool should place one bulb per light of the tree's selection rather than upstream's one bulb:
    /// the selection is more than one element, or a group. One selected light keeps upstream's path, which links it already.
    pub(super) fn studio_places_single_lights(&self) -> bool {
        if self.tool != Tool::Single {
            return false;
        }
        let Some(tree) = self.tree.as_deref() else { return false };
        match self.selected_nodes.as_slice() {
            [] => false,
            [one] => tree.find(one).is_some_and(|n| !tree.is_leaf(n)),
            _ => true,
        }
    }

    /// The Single Light tool's mouse-down with several elements selected: the bulbs, selected together, as one Undo step.
    pub(super) fn studio_draw_single_lights(&mut self, at: PreviewPoint, ch: &mut Change) {
        let nodes = self.selected_nodes.clone();
        self.deselect_selected_display_item(ch);
        let width = if self.view_size.0 > 0 { self.view_size.0 } else { self.data.width };
        let ids = self.studio_add_single_lights(at, &nodes, width, ch);
        if ids.is_empty() {
            return;
        }
        self.selected_items = ids;
        ch.selection_changed = true;
        self.runs_dirty = true;
        if !self.data.keep_insert_mode {
            self.tool = Tool::Select;
            ch.tool_changed = true;
        }
    }

    /// Edit ▸ Add ▸ Bulbs for Selected Elements: one linked bulb per light of the element tree's selection, in a row right of
    /// the preview's props (or under them), without choosing the tool. One Undo step; nothing when the selection holds no lights.
    pub fn studio_place_single_lights(&mut self) -> Change {
        let mut ch = Change::default();
        let count = self.tree.as_deref().map_or(0, |t| single_light_targets(t, &self.selected_nodes).len());
        let width = if self.data.width > 0 { self.data.width } else { 1920 };
        let (x, y) = free_row_origin(&self.data, count, width);
        // Bulbs are drawn from view points, which the shape divides by the zoom.
        let at = PreviewPoint::new(to_int32(x as f64 * self.zoom), to_int32(y as f64 * self.zoom));
        let tool = self.tool;
        let added = ch.shapes_added.len();
        self.studio_draw_single_lights(at, &mut ch);
        if ch.shapes_added.len() == added {
            ch.message = Some("Select one or more elements or a group in the Elements list first. Each light gets its own bulb, linked to it.".into());
        }
        self.tool = tool;
        ch.tool_changed = false;
        self.end_update(&mut ch);
        ch
    }

    /// One bulb per light under `nodes`, linked to it and named after it, in rows from `at` (top left) that wrap before `width`.
    /// One Undo step. Returns the new shapes, none when the nodes hold no lights.
    pub fn studio_add_single_lights(&mut self, at: PreviewPoint, nodes: &[Uuid], width: i32, ch: &mut Change) -> Vec<ShapeId> {
        let Some(tree) = self.tree.clone() else { return Vec::new() };
        let lights = single_light_targets(&tree, nodes);
        let mut ids = Vec::new();
        let (mut x, mut y) = (at.x, at.y);
        let limit = if width > 0 { width } else { i32::MAX };
        for light in lights.into_iter().take(MAX_SINGLE_LIGHTS) {
            if x > at.x && x + SINGLE_LIGHT_SPACING / 2 > limit {
                x = at.x;
                y += SINGLE_LIGHT_SPACING;
            }
            let mut shape = Shape::new_single(PreviewPoint::new(x, y), Sel::from_uuid(&tree, light), self.zoom);
            shape.name = Some(tree.node(tree.find(&light).expect("a target is in the tree")).name.clone());
            shape.set_pixel_size(SINGLE_LIGHT_SIZE);
            shape.end_add_new();
            let id = self.add_display_item(shape, ch);
            if let Some(i) = self.index_of(id) {
                self.data.display_items[i].shape.set_zoom_level(self.zoom);
                self.touch(i, ch);
            }
            ids.push(id);
            x += SINGLE_LIGHT_SPACING;
        }
        if !ids.is_empty() {
            self.push_undo(UndoAction::Added { ids: ids.clone(), parked: Vec::new(), paste: false }, ch);
        }
        ids
    }
}
