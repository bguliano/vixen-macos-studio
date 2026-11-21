//! Studio: the preview designer's props as VoiceOver reads them, and Tab and ⇧Tab moving the selection between them (UI
//! roadmap R43; the timeline and graph are in `vixen_session::studio_access`). A child module of `session`, so it
//! selects a shape the way a click does without widening the session.
use super::{Change, DesignSession};
use crate::data::ShapeId;
use std::collections::BTreeSet;
use uuid::Uuid;

/// One prop as an accessibility element. `x`, `y`, `w`, `h` are in the designer's view coordinates (y down), as its
/// mouse events are.
#[derive(Clone, Debug, PartialEq)]
pub struct AccessItem {
    pub id: ShapeId,
    pub label: String,
    pub selected: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl DesignSession {
    /// A prop's words: its kind and name, how many lights it has and what they're linked to, and whether it's locked.
    pub fn studio_access_label(&self, id: ShapeId) -> Option<String> {
        let shape = &self.item(id)?.shape;
        let mut label = shape.type_name().to_string();
        if let Some(name) = shape.name().filter(|n| !n.is_empty()) {
            label = format!("{name}, {}", label.to_lowercase());
        }
        let lights = shape.pixel_count();
        if lights > 0 {
            label.push_str(&format!(", {lights} {}", if lights == 1 { "light" } else { "lights" }));
        }
        let mut nodes = BTreeSet::new();
        shape.for_each_pixel(&mut |p| {
            if !p.node_id.is_nil() {
                nodes.insert(p.node_id);
            }
        });
        if shape.is_light_shape() {
            label.push_str(&match self.studio_linked_name(&nodes) {
                Some(name) => format!(", linked to {name}"),
                None => ", not linked".to_string(),
            });
        }
        if shape.locked() {
            label.push_str(", locked");
        }
        Some(label)
    }

    /// The element the lights belong to: the one node, else the parent every node shares, else the first and a count.
    fn studio_linked_name(&self, nodes: &BTreeSet<Uuid>) -> Option<String> {
        let first = *nodes.iter().next()?;
        let Some(tree) = self.tree.as_deref() else { return Some(format!("{} elements", nodes.len())) };
        let name = |id: &Uuid| tree.find(id).map(|n| tree.node(n).name.clone());
        if nodes.len() == 1 {
            return name(&first);
        }
        let parents = |id: &Uuid| tree.find(id).map(|n| tree.node(n).parents.clone()).unwrap_or_default();
        let shared = parents(&first).into_iter().find(|p| nodes.iter().all(|id| parents(id).contains(p)));
        if let Some(parent) = shared {
            return Some(tree.node(parent).name.clone());
        }
        Some(format!("{} and {} more", name(&first).unwrap_or_else(|| "an element".into()), nodes.len() - 1))
    }

    /// Every prop the canvas shows, in stacking order (the order Tab visits them), with where it is in the view.
    pub fn studio_access_items(&self) -> Vec<AccessItem> {
        let (zoom, (sx, sy)) = (self.zoom, self.scroll());
        let view = |v: i32, scroll: i32| (v as f64 * zoom).round() as i32 - scroll;
        self.data
            .display_items
            .iter()
            .filter(|i| self.visible(&i.shape))
            .filter_map(|i| {
                let s = &i.shape;
                let (x, y) = (view(s.left(), sx), view(s.top(), sy));
                let (r, b) = (view(s.right(), sx), view(s.bottom(), sy));
                Some(AccessItem { id: i.id, label: self.studio_access_label(i.id)?, selected: self.is_selected(i.id), x, y, w: (r - x).max(1), h: (b - y).max(1) })
            })
            .collect()
    }

    /// Selects one prop alone with its handles and properties, as a click on it does. Locked props, which a click can't
    /// pick, can't be picked here either.
    pub fn studio_access_select(&mut self, id: ShapeId) -> (Change, Option<String>) {
        let mut ch = Change::default();
        if !self.item(id).is_some_and(|i| self.visible(&i.shape) && !i.shape.locked()) {
            return (ch, None);
        }
        self.deselect_selected_display_item(&mut ch);
        self.selected_items.clear();
        self.selected_item = Some(id);
        self.select_shape(id, &mut ch);
        self.on_select_display_item(id, &mut ch);
        ch.selection_changed = true;
        ch.overlay_changed = true;
        self.runs_dirty = true;
        self.end_update(&mut ch);
        let label = self.studio_access_label(id);
        (ch, label)
    }

    /// Tab and ⇧Tab: the next or previous unlocked prop after the selected one, wrapping around, or the first or last
    /// when nothing is selected.
    pub fn studio_access_step(&mut self, direction: i32) -> (Change, Option<String>) {
        let pickable: Vec<ShapeId> = self.data.display_items.iter().filter(|i| self.visible(&i.shape) && !i.shape.locked()).map(|i| i.id).collect();
        if pickable.is_empty() {
            return (Change::default(), None);
        }
        let current = self.selected_item.or_else(|| self.selected_items.last().copied());
        let n = pickable.len();
        let next = match current.and_then(|c| pickable.iter().position(|p| *p == c)) {
            Some(at) if direction < 0 => (at + n - 1) % n,
            Some(at) => (at + 1) % n,
            None if direction < 0 => n - 1,
            None => 0,
        };
        self.studio_access_select(pickable[next])
    }
}
