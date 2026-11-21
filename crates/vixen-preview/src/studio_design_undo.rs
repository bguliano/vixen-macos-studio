//! Studio: the designer edits that upstream commits through OK/Cancel dialogs (Linked Elements, a fixture's Link Element,
//! Location Offset, Resize Background) are live in Studio, so each is a step on the designer's own undo stack, as is a run of
//! arrow-key nudges. A child module of `session`, so it reaches the session's undo stack and shape swapping without widening
//! them.
use super::{Change, DesignSession, UndoAction};
use crate::data::{ShapeId, Vector3D};
use crate::shape::Shape;
use crate::xml_util::DoubleText;

#[derive(Clone, Debug)]
pub(super) enum StudioUndo {
    /// Shapes edited in place, as they were. Undo and redo swap them back and forth, as a move does.
    Shapes { saved: Vec<(ShapeId, Shape)>, label: &'static str },
    /// The location offset beforehand.
    LocationOffset { saved: Vector3D },
    /// The background image's file name and size beforehand, and every shape if the resize scaled them.
    Background { file: Option<String>, size: Option<(i32, i32)>, saved: Vec<(ShapeId, Shape)> },
    /// A run of arrow-key nudges: the shapes as they were before the first, and as saved after the last, so the next nudge
    /// joins the run only when nothing else has changed them since.
    Nudge { saved: Vec<(ShapeId, Shape)>, after: Vec<String> },
}

impl StudioUndo {
    pub(super) fn description(&self) -> String {
        match self {
            StudioUndo::Shapes { label, .. } => (*label).to_string(),
            StudioUndo::LocationOffset { .. } => "Location Offset".to_string(),
            StudioUndo::Background { .. } => "Resize Background".to_string(),
            StudioUndo::Nudge { saved, .. } => format!("Move {} ", saved.len()),
        }
    }
}

impl DesignSession {
    pub(super) fn apply_studio_undo(&mut self, action: &mut StudioUndo, ch: &mut Change) {
        match action {
            StudioUndo::Shapes { saved, .. } | StudioUndo::Nudge { saved, .. } => self.swap_places(saved, ch),
            StudioUndo::LocationOffset { saved } => std::mem::swap(&mut self.data.location_offset, saved),
            StudioUndo::Background { file, size, saved } => {
                std::mem::swap(&mut self.data.background_file_name, file);
                std::mem::swap(&mut self.background_size, size);
                self.swap_places(saved, ch);
                self.update_scroll_range();
                self.end_update(ch);
            }
        }
    }

    /// Runs `edit` on one shape and records the shape as it was, as one undo step called `label`. Nothing is recorded for a
    /// shape that isn't there or an edit that changed nothing.
    pub fn studio_edit_shape_undoable<E>(&mut self, id: ShapeId, label: &'static str, edit: impl FnOnce(&mut Self) -> Result<Change, E>) -> Result<Change, E> {
        let Some(before) = self.item(id).map(|i| i.shape.clone()) else { return edit(self) };
        let before_xml = shape_xml(&before);
        let mut ch = edit(self)?;
        if self.item(id).is_some_and(|i| shape_xml(&i.shape) != before_xml) {
            self.push_undo(UndoAction::Studio(StudioUndo::Shapes { saved: vec![(id, before)], label }), &mut ch);
        }
        Ok(ch)
    }

    /// Replaces one shape with `shape` (rebuilt and redrawn). With `undo_from`, the shape as it was before a run of such
    /// replacements goes on the undo stack as one step called `label`; without it, the replacement joins the step already there.
    pub fn studio_replace_shape(&mut self, id: ShapeId, shape: Shape, undo_from: Option<(Shape, &'static str)>) -> Change {
        let mut ch = self.edit_shape(id, |s| *s = shape);
        if let Some((before, label)) = undo_from {
            self.push_undo(UndoAction::Studio(StudioUndo::Shapes { saved: vec![(id, before)], label }), &mut ch);
        }
        ch
    }

    /// An arrow-key nudge (upstream records none). The first nudge of the selection goes on the undo stack as a Move step;
    /// the next ones join it while the same shapes are selected and nothing else has changed them in between.
    pub(super) fn studio_nudge(&mut self, dx: i32, dy: i32, ch: &mut Change) {
        let ids = self.studio_nudge_targets();
        let joins = match self.undoable.front() {
            Some(UndoAction::Studio(StudioUndo::Nudge { saved, after })) => {
                saved.iter().map(|(id, _)| *id).eq(ids.iter().copied()) && self.shapes_xml(&ids) == *after
            }
            _ => false,
        };
        let saved: Option<Vec<(ShapeId, Shape)>> =
            (!joins).then(|| ids.iter().filter_map(|id| self.item(*id).map(|i| (*id, i.shape.clone()))).collect());
        self.nudge_selected(dx, dy, ch);
        let after = self.shapes_xml(&ids);
        match saved {
            Some(saved) => {
                if saved.iter().map(|(_, s)| shape_xml(s)).ne(after.iter().cloned()) {
                    self.push_undo(UndoAction::Studio(StudioUndo::Nudge { saved, after }), ch);
                }
            }
            None => {
                if let Some(UndoAction::Studio(StudioUndo::Nudge { after: last, .. })) = self.undoable.front_mut() {
                    *last = after;
                }
            }
        }
    }

    /// The shapes `nudge_selected` moves: the selected item, or every visible shape sharing bounds with the multi-selection.
    fn studio_nudge_targets(&self) -> Vec<ShapeId> {
        if let Some(id) = self.selected_item.filter(|id| self.index_of(*id).is_some()) {
            return vec![id];
        }
        if self.selected_items.is_empty() {
            return Vec::new();
        }
        let keys = self.selected_keys();
        self.data.display_items.iter().filter(|i| keys.contains(&i.shape.bounds_key()) && self.visible(&i.shape)).map(|i| i.id).collect()
    }

    fn shapes_xml(&self, ids: &[ShapeId]) -> Vec<String> {
        ids.iter().filter_map(|id| self.item(*id)).map(|i| shape_xml(&i.shape)).collect()
    }

    /// The Location Offset fields, each change one undo step. The same values add nothing.
    pub fn studio_set_location_offset(&mut self, x: f64, y: f64) -> Change {
        let mut ch = Change::default();
        let offset = &self.data.location_offset;
        if offset.x.value == x && offset.y.value == y {
            return ch;
        }
        let z = offset.z.clone();
        let saved = std::mem::replace(&mut self.data.location_offset, Vector3D { x: DoubleText::new(x), y: DoubleText::new(y), z });
        self.push_undo(UndoAction::Studio(StudioUndo::LocationOffset { saved }), &mut ch);
        ch
    }

    /// Resize Background as one undo step: the host has already written the resized image as `file` (None keeps the current
    /// one, as for a blank background). Undo puts back the old file name and size; the host reloads the image when the name
    /// differs afterwards.
    pub fn studio_resize_background(&mut self, file: Option<String>, width: i32, height: i32, scale_shapes: bool) -> Change {
        let saved_file = self.data.background_file_name.clone();
        let saved_size = self.background_size;
        let saved: Vec<(ShapeId, Shape)> =
            if scale_shapes { self.data.display_items.iter().map(|i| (i.id, i.shape.clone())).collect() } else { Vec::new() };
        if let Some(file) = file {
            self.data.background_file_name = Some(file);
        }
        let mut ch = self.resize_background(width, height, scale_shapes);
        self.push_undo(UndoAction::Studio(StudioUndo::Background { file: saved_file, size: saved_size, saved }), &mut ch);
        ch
    }
}

/// A shape as it's saved, to tell whether an edit changed it.
fn shape_xml(shape: &Shape) -> String {
    format!("{:?}", shape.to_xml())
}
