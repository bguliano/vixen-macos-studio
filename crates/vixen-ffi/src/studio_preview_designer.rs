//! Studio: the preview designer's dialogs without OK/Cancel. Linked Elements and a fixture's Link Element change the shape as
//! they're used, Location Offset applies as it's typed, and Resize Background is one step; each is undone from the designer's
//! Edit ▸ Undo (`studio_design_undo` in vixen-preview). A child module of `preview`, so it reaches the designer and linker
//! without widening them.
use super::{DesignerChange, DesignerPropertyValue, PreviewDesigner, PreviewLinker};
use crate::core::FfiError;
use vixen_session::preview::background_path;
use vixen_session::preview_design::{load_background_checked, resize_background_image, BackgroundLoad};
use vixen_session::render_support::design;

/// The live Linked Elements sheet's shape as it was when the sheet opened, and where its undo step is on the stack.
#[derive(Default)]
pub(super) struct LiveLink {
    before: Option<vixen_session::studio_design::Shape>,
    /// The undo count right after this sheet's step went on, so later changes join it. Anything else moving the count (an
    /// Undo meanwhile) starts a new step from the shape as it is then.
    pushed_at: Option<usize>,
}

impl PreviewDesigner {
    /// Loads the background image again when an undo or redo put back another file name.
    fn reload_background_if_changed(&self, before: Option<String>) {
        let host = self.host.lock();
        let now = host.session.data().background_file_name.clone().filter(|n| !n.is_empty());
        if now == before {
            return;
        }
        // Resize Background only ever swaps one image file for another.
        if let Some(BackgroundLoad::Image { rgba, width, height }) = now.map(|name| load_background_checked(&background_path(&self.core.paths, &name))) {
            host.renderer().set_background(&rgba, width, height);
        }
    }

    fn background_file(&self) -> Option<String> {
        self.host.lock().session.data().background_file_name.clone().filter(|n| !n.is_empty())
    }
}

#[uniffi::export]
impl PreviewDesigner {
    /// Edit ▸ Undo in the designer, which may load the background image again (Resize Background).
    pub fn studio_undo_many(&self, steps: u32) -> DesignerChange {
        let file = self.background_file();
        let change = self.run(|s| s.undo_many(steps as usize));
        self.reload_background_if_changed(file);
        change
    }

    /// Edit ▸ Redo in the designer.
    pub fn studio_redo_many(&self, steps: u32) -> DesignerChange {
        let file = self.background_file();
        let change = self.run(|s| s.redo_many(steps as usize));
        self.reload_background_if_changed(file);
        change
    }

    /// Edit ▸ Add ▸ Bulbs for Selected Elements: one Single Light per light of the element tree's selection, each linked to its
    /// light and named after it, as one undo step (`studio_single_lights` in vixen-preview; B60).
    pub fn studio_place_single_lights(&self) -> DesignerChange {
        self.run(|s| s.studio_place_single_lights())
    }

    /// The Location Offset sheet's fields, as they're committed: one undo step each, none when nothing changed.
    pub fn studio_set_location_offset(&self, x: f64, y: f64) -> DesignerChange {
        self.run(|s| s.studio_set_location_offset(x, y))
    }

    /// Resize Background as one undo step: the resized image is written beside the old one, which Undo goes back to.
    pub fn studio_resize_background(&self, width: i32, height: i32, scale_shapes: bool) -> Result<DesignerChange, FfiError> {
        let mut file = None;
        if let Some(name) = self.background_file() {
            let resized = resize_background_image(&self.core.paths, &name, width as u32, height as u32)?;
            if let BackgroundLoad::Image { rgba, width, height } = load_background_checked(&background_path(&self.core.paths, &resized)) {
                self.host.lock().renderer().set_background(&rgba, width, height);
            }
            file = Some(resized);
        }
        Ok(self.run(|s| s.studio_resize_background(file, width, height, scale_shapes)))
    }

    /// A moving head's Link Element list, as a row is chosen (None unlinks it): one undo step, "Link Element".
    pub fn studio_set_fixture_node(&self, shape: u32, node: Option<String>) -> Result<DesignerChange, FfiError> {
        let mut host = self.host.lock();
        let value: design::PropertyValue = DesignerPropertyValue::LinkedElement { node }.into();
        let change = host
            .session
            .studio_edit_shape_undoable(Self::shape_id(shape), "Link Element", |s| s.set_shape_property(Self::shape_id(shape), "Node", value))
            .map_err(|e| FfiError::new(e.to_string()))?;
        Ok(host.apply(change).into())
    }
}

#[uniffi::export]
impl PreviewLinker {
    /// The live Linked Elements sheet, after each change (a drop, a double-click, a context-menu command, Set): the shape takes
    /// every link the sheet holds now. All the changes made while the sheet is open are one undo step, "Link Elements".
    pub fn studio_apply_live(&self) -> DesignerChange {
        let Some(linker) = self.linker.lock().clone() else { return DesignerChange::default() };
        let shape_id = self.shape;
        let mut live = self.studio_live.lock();
        let mut host = self.designer.host.lock();
        let Some(current) = host.session.item(shape_id).map(|i| i.shape.clone()) else { return DesignerChange::default() };
        let before = live.before.get_or_insert_with(|| current.clone()).clone();
        let mut shape = before.clone();
        linker.apply(&mut shape);
        // A change that leaves the links as they are (Reverse twice, Set to the same count) adds no step.
        if format!("{:?}", shape.to_xml()) == format!("{:?}", current.to_xml()) {
            return DesignerChange::default();
        }
        let joins = live.pushed_at == Some(host.session.num_undoable());
        let undo_from = (!joins).then(|| (current, "Link Elements"));
        let change = host.session.studio_replace_shape(shape_id, shape, undo_from);
        live.pushed_at = Some(host.session.num_undoable());
        host.apply(change).into()
    }
}

/// A linker's live state starts empty; the first change records the shape.
pub(super) fn new_live_link() -> parking_lot::Mutex<LiveLink> {
    parking_lot::Mutex::new(LiveLink::default())
}

// Studio R43: the designer's props for VoiceOver, and Tab between them (`studio_access` in vixen-preview).

/// One prop as an accessibility element, in the canvas's view coordinates (y down).
#[derive(uniffi::Record, Clone)]
pub struct DesignerStudioAccessItem {
    pub shape: u32,
    pub label: String,
    pub selected: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// A keyboard or VoiceOver selection: what changed, and what VoiceOver should say (None when nothing was picked).
#[derive(uniffi::Record)]
pub struct DesignerStudioAccessMove {
    pub change: DesignerChange,
    pub announcement: Option<String>,
}

#[uniffi::export]
impl PreviewDesigner {
    /// Every prop the canvas shows, in the order Tab visits them.
    pub fn studio_access_items(&self) -> Vec<DesignerStudioAccessItem> {
        let host = self.host.lock();
        host.session
            .studio_access_items()
            .into_iter()
            .map(|i| DesignerStudioAccessItem { shape: i.id.0, label: i.label, selected: i.selected, x: i.x, y: i.y, w: i.w, h: i.h })
            .collect()
    }

    /// Tab (1) and ⇧Tab (-1): the next or previous unlocked prop, wrapping around.
    pub fn studio_access_step(&self, direction: i32) -> DesignerStudioAccessMove {
        let mut host = self.host.lock();
        let (change, said) = host.session.studio_access_step(direction);
        DesignerStudioAccessMove { change: host.apply(change).into(), announcement: said }
    }

    /// VoiceOver's press on a prop: selects it alone, as a click does.
    pub fn studio_access_select(&self, shape: u32) -> DesignerStudioAccessMove {
        let mut host = self.host.lock();
        let (change, said) = host.session.studio_access_select(design::ShapeId(shape));
        DesignerStudioAccessMove { change: host.apply(change).into(), announcement: said }
    }
}
