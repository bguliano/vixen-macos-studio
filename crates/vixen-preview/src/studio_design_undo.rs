//! Studio: the designer edits that upstream commits through OK/Cancel dialogs (Linked Elements, a fixture's Link Element,
//! Location Offset, Resize Background) are live in Studio, so each is a step on the designer's own undo stack. A child module
//! of `session`, so it reaches the session's undo stack and shape swapping without widening them.
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
}

impl StudioUndo {
    pub(super) fn description(&self) -> String {
        match self {
            StudioUndo::Shapes { label, .. } => (*label).to_string(),
            StudioUndo::LocationOffset { .. } => "Location Offset".to_string(),
            StudioUndo::Background { .. } => "Resize Background".to_string(),
        }
    }
}

impl DesignSession {
    pub(super) fn apply_studio_undo(&mut self, action: &mut StudioUndo, ch: &mut Change) {
        match action {
            StudioUndo::Shapes { saved, .. } => self.swap_places(saved, ch),
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
