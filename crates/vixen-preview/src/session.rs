//! Port of the design surface's behaviour in VixenPreviewControl.cs and VixenPreviewSetup3.cs: the mouse
//! state machine, selection, the rubber band, handles, nudging, alignment, the clipboard, grouping and undo.
//!
//! Events that show a dialog or a property grid in the C# come back as fields of [`Change`] for the host.
//!
//! Selection works as in the C#: one shape can be the single selected item (with handles), or several can be
//! the multi-selection (no handles, moved together). `List.Contains` and `List.Remove` on display items go by
//! `DisplayItem.Equals`, which compares bounds, so two shapes with identical bounds count as the same one
//! for selection. That is deliberate, as in the C#. Removing from the preview itself goes by that rule too,
//! except that the undo stack holds whichever shape really went.

use crate::data::{DisplayItem, PreviewData, ShapeId, Vector3D};
use crate::overlay::{self, build_light_runs, LightRun, Overlay, RunContext};
use crate::point::PreviewPoint;
use crate::scene::{Scene, SceneDelta};
use crate::shape::properties::{PropertyError, PropertyValue, ShapeProperty};
use crate::shape::{Modifiers, SelectArg, Sel, Shape, ShapeKind};
use crate::tools::{to_int32, Rect};
use crate::xml_util::{DoubleText, SHAPES_NS};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use uuid::Uuid;
use vixen_core::element::{property_ids, PropertyData, PropertyInstance};
use vixen_core::{ElementTree, NodeId};
use vixen_xml::XElement;

// STUDIO-BEGIN designer-live-undo: Studio's live designer edits (Linked Elements, Link Element, Location Offset, Resize Background) on the designer's undo stack
#[path = "studio_design_undo.rs"]
mod studio_design_undo;
// STUDIO-END designer-live-undo

/// `VixenPreviewControl.Tools`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    String,
    Arch,
    Rectangle,
    Ellipse,
    Single,
    Triangle,
    MegaTree,
    Net,
    Flood,
    Star,
    Cane,
    PixelGrid,
    StarBurst,
    Icicle,
    PolyLine,
    MultiString,
    MovingHead,
}

impl Tool {
    /// `VixenPreviewSetup3.DrawShape`, set with the toolbar button: the name an emptied Name property takes.
    fn draw_shape_name(self) -> &'static str {
        match self {
            Tool::Select | Tool::Flood => "",
            Tool::Single => "Pixel",
            Tool::String => "Line",
            Tool::Arch => "Arch",
            Tool::Rectangle => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Triangle => "Triangle",
            Tool::Net => "Net",
            Tool::Cane => "Candy Cane",
            Tool::Star => "Star",
            Tool::StarBurst => "Star Burst",
            Tool::MegaTree => "Mega Tree",
            Tool::PixelGrid => "Grid",
            Tool::Icicle => "Icicle",
            Tool::PolyLine => "PolyLine",
            Tool::MultiString => "Multi String",
            Tool::MovingHead => "Intelligent Fixture",
        }
    }

    /// `ShowElementCreateTemplateForCurrentTool`: the element template offered for a shape with no element.
    fn element_template(self) -> Option<&'static str> {
        match self {
            Tool::Arch => Some("Arch"),
            Tool::Cane => Some("Candy Cane"),
            Tool::Star => Some("Star"),
            Tool::StarBurst => Some("Starburst"),
            Tool::MegaTree => Some("Megatree"),
            Tool::PixelGrid => Some("Pixel Grid / Matrix"),
            Tool::Icicle => Some("Icicles"),
            Tool::Rectangle | Tool::Triangle | Tool::Ellipse | Tool::String => Some("Generic Numbered Group"),
            Tool::MovingHead => Some("Intelligent Fixture"),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// The keys `OnKeyDown` handles.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Delete,
    C,
    X,
    V,
    Z,
    Y,
    L,
    U,
    N,
    Up,
    Down,
    Left,
    Right,
    Escape,
    Plus,
    Minus,
}

/// The cursor the C# would set.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Cursor {
    #[default]
    Default,
    /// Over a select point.
    Cross,
    /// Over a shape that would move.
    SizeAll,
    /// Over a rotate handle.
    Rotate,
    /// A drawing tool is active.
    InsertMode,
}

/// An item of the right-click menu.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Ungroup,
    Group,
    CreateTemplate,
    Cut,
    Copy,
    Paste,
    Delete,
    Undo,
    Redo,
    Lock,
    Unlock,
    /// The "Set Z Location to" submenu, 0 to 9.
    SetZ,
}

/// A location for an element, as `SaveLocations` writes it. See [`apply_location_updates`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LocationUpdate {
    pub node: Uuid,
    pub x: i32,
    pub y: i32,
}

/// What an operation changed.
#[derive(Clone, Debug, Default)]
pub struct Change {
    /// Lights to send to the GPU.
    pub scene: Vec<SceneDelta>,
    /// The overlay is different, so read [`DesignSession::overlay`] again.
    pub overlay_changed: bool,
    /// The overlay's edit colours are different too. A drag leaves this unset, so that a host only has to fill its
    /// colour buffer when a light has actually changed colour.
    pub light_colors_changed: bool,
    pub selection_changed: bool,
    pub shapes_added: Vec<ShapeId>,
    pub shapes_removed: Vec<ShapeId>,
    /// Set when the C# would update the cursor. None leaves it as it is.
    pub cursor: Option<Cursor>,
    /// `OnSelectDisplayItem`: show this shape's properties.
    pub show_properties: Option<ShapeId>,
    /// `OnDeSelectDisplayItem`: clear the properties.
    pub clear_properties: bool,
    /// The element tree's selection was cleared, as selecting a shape does.
    pub elements_deselected: bool,
    /// `ShowElementCreateTemplateForCurrentTool`: offer this element template for the shape just drawn. Pass what
    /// it creates to [`DesignSession::apply_element_template`].
    pub element_template: Option<(ShapeId, &'static str)>,
    /// The right-click menu to show.
    pub context_menu: Option<Vec<MenuItem>>,
    pub zoom_changed: Option<f64>,
    pub scroll_changed: bool,
    pub tool_changed: bool,
    /// Text put on the clipboard. Also kept in the session for [`DesignSession::paste`].
    pub clipboard: Option<String>,
    /// A message the C# shows in a dialog.
    pub message: Option<String>,
    /// The "How to Unlock a Prop" hint, shown once.
    pub lock_hint: bool,
    /// Element locations to write, from `SetZForSelectedItems`.
    pub location_updates: Vec<LocationUpdate>,
    /// The undo or redo stacks changed.
    pub undo_changed: bool,
}

/// `modifyType`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
enum ModifyType {
    #[default]
    None,
    Resize,
    Move,
    AddNew,
}

/// `Common.Controls.UndoAction` subclasses from the Undo folder.
#[derive(Clone, Debug)]
enum UndoAction {
    /// `PreviewItemsAddedUndoAction` and `PreviewItemsPasteUndoAction`.
    Added { ids: Vec<ShapeId>, parked: Vec<DisplayItem>, paste: bool },
    /// `PreviewItemsRemovedUndoAction`.
    Removed { ids: Vec<ShapeId>, parked: Vec<DisplayItem> },
    /// `PreviewItemsMoveUndoAction` and `PreviewItemsResizeUndoAction`: the shapes as they were, swapped back in
    /// and out. The C# keeps them as XML, so a restored shape behaves as one just deserialized.
    MoveResize { saved: Vec<(ShapeId, Shape)>, resize: bool },
    /// `PreviewItemsLockUndoAction`: bounds and the lock state beforehand.
    Lock { items: Vec<((i32, i32, i32, i32), bool)> },
    /// `PreviewItemPixelSizeChangeUndoAction`. None holds the live selection list: the C# keeps a reference to
    /// `SelectedDisplayItems`, so undoing a multi-select size change resizes whatever is selected then.
    /// Deliberate.
    PixelSize { ids: Option<Vec<ShapeId>>, change: i32 },
    /// `PreviewItemsGroupAddedUndoAction`. The C# finds the items again by hit testing a point on them, and these
    /// ids avoid picking up whatever else is under that point. In the normal case that is the same items.
    GroupAdded { group: ShapeId, children: Vec<ShapeId> },
    Studio(studio_design_undo::StudioUndo), // STUDIO: designer-live-undo
}

impl UndoAction {
    fn description(&self) -> String {
        match self {
            UndoAction::Added { ids, parked, paste } => {
                let n = ids.len().max(parked.len());
                if *paste {
                    format!("Paste {n} ")
                } else {
                    format!("Added {n} ")
                }
            }
            UndoAction::Removed { ids, parked } => format!("Removed {} ", ids.len().max(parked.len())),
            UndoAction::MoveResize { saved, resize } => {
                if *resize {
                    "Resize 1".to_string()
                } else {
                    format!("Move {} ", saved.len())
                }
            }
            UndoAction::Lock { items } => format!("Lock {} props", items.len()),
            UndoAction::PixelSize { ids, .. } => format!("Pixel size on {} items.", ids.as_ref().map_or(0, Vec::len)),
            UndoAction::GroupAdded { .. } => "Added Group".to_string(),
            UndoAction::Studio(action) => action.description(), // STUDIO: designer-live-undo
        }
    }
}

/// `UndoManager.DefaultMaxItems`.
const UNDO_MAX_ITEMS: usize = 1000;

/// What was found and done by [`DesignSession::setup_open`].
#[derive(Clone, Debug, Default)]
pub struct VerifyReport {
    /// Shapes moved back from outside the preview.
    pub moved: Vec<ShapeId>,
    /// Shapes with no size or no strings, removed as added by mistake.
    pub removed: Vec<ShapeId>,
    /// The dialog the C# shows if anything moved.
    pub message: Option<String>,
}

/// Everything `SetButtonEnabledState` computes, and what `UpdateHideLockedCommandState` adds, as one comparable
/// value. Upstream only reruns those on some events, so its buttons can go stale. A host recomputes this after each change.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ToolbarState {
    /// Bulb size buttons: a single item or anything in the multi-selection.
    pub bulb: bool,
    pub lock: bool,
    pub unlock: bool,
    pub unlock_all: bool,
    /// The Align group: more than one in the multi-selection.
    pub align: bool,
    pub hide_locked_enabled: bool,
    /// Locked shapes are hidden, so the button reads Show Locked.
    pub hide_locked_active: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub tool: Tool,
}

/// See the [module documentation](self).
pub struct DesignSession {
    data: PreviewData,
    scene: Scene,
    tree: Option<Arc<ElementTree>>,
    zoom: f64,
    tool: Tool,
    selected_item: Option<ShapeId>,
    selected_items: Vec<ShapeId>,
    /// Display items with select points, so the overlay does not look at every shape.
    handle_items: Vec<ShapeId>,
    drag_start: (i32, i32),
    band_rect: Rect,
    mouse_captured: bool,
    banding: bool,
    panning: bool,
    modify_type: ModifyType,
    move_info: Option<Vec<(ShapeId, Shape)>>,
    highlighted: HashSet<Uuid>,
    highlight_generation: u64,
    hide_locked: bool,
    show_info: bool,
    selected_node: Option<Uuid>,
    selected_nodes: Vec<Uuid>,
    element_selected: Option<Uuid>,
    properties_shape: Option<ShapeId>,
    h_scroll: i32,
    v_scroll: i32,
    h_scroll_max: i32,
    v_scroll_max: i32,
    view_size: (i32, i32),
    background_size: Option<(i32, i32)>,
    zoom_to: (i32, i32),
    mouse_client: (i32, i32),
    cursor: Cursor,
    undoable: VecDeque<UndoAction>,
    redoable: Vec<UndoAction>,
    clipboard: Option<String>,
    clipboard_populated: bool,
    item_name: String,
    item_index: i32,
    item_bulb_size: i32,
    draw_shape: String,
    lock_warning_shown: bool,
    runs: Arc<Vec<LightRun>>,
    runs_dirty: bool,
}

impl DesignSession {
    /// A session on a preview. Call [`DesignSession::setup_open`] if this is the Preview Setup window opening.
    pub fn new(data: PreviewData, tree: Option<Arc<ElementTree>>) -> DesignSession {
        let scene = Scene::build(&data, tree.as_deref());
        let mut s = DesignSession {
            data,
            scene,
            tree,
            zoom: 1.0,
            tool: Tool::Select,
            selected_item: None,
            selected_items: Vec::new(),
            handle_items: Vec::new(),
            drag_start: (0, 0),
            band_rect: Rect::default(),
            mouse_captured: false,
            banding: false,
            panning: false,
            modify_type: ModifyType::None,
            move_info: None,
            highlighted: HashSet::new(),
            highlight_generation: 0,
            hide_locked: false,
            show_info: false,
            selected_node: None,
            selected_nodes: Vec::new(),
            element_selected: None,
            properties_shape: None,
            h_scroll: 0,
            v_scroll: 0,
            h_scroll_max: 0,
            v_scroll_max: 0,
            view_size: (0, 0),
            background_size: None,
            zoom_to: (0, 0),
            mouse_client: (0, 0),
            cursor: Cursor::Default,
            undoable: VecDeque::new(),
            redoable: Vec::new(),
            clipboard: None,
            clipboard_populated: false,
            item_name: String::new(),
            item_index: 0,
            item_bulb_size: 0,
            draw_shape: String::new(),
            lock_warning_shown: false,
            runs: Arc::new(Vec::new()),
            runs_dirty: true,
        };
        s.rebuild_runs();
        s
    }

    // ----- access

    #[inline]
    pub fn data(&self) -> &PreviewData {
        &self.data
    }

    /// Hands the preview back, for saving.
    pub fn into_data(self) -> PreviewData {
        self.data
    }

    /// The renderer's input. Its `lights` always reflect the last operation.
    #[inline]
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    #[inline]
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    #[inline]
    pub fn tool(&self) -> Tool {
        self.tool
    }

    #[inline]
    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// (horizontal, vertical) scroll offset, which is what the renderer's pan is the negative of.
    #[inline]
    pub fn scroll(&self) -> (i32, i32) {
        (self.h_scroll, self.v_scroll)
    }

    /// `_selectedDisplayItem`: the one shape showing handles.
    #[inline]
    pub fn selected_item(&self) -> Option<ShapeId> {
        self.selected_item
    }

    /// `SelectedDisplayItems`.
    #[inline]
    pub fn selected_items(&self) -> &[ShapeId] {
        &self.selected_items
    }

    /// Whether the shape draws as selected: `displayItem.Shape.Selected || SelectedDisplayItems.Contains(displayItem)`.
    pub fn is_selected(&self, id: ShapeId) -> bool {
        self.item(id).is_some_and(|i| i.shape.selected) || self.selected_contains(id)
    }

    /// `HighlightedElements`.
    pub fn highlighted_elements(&self) -> &HashSet<Uuid> {
        &self.highlighted
    }

    pub fn num_undoable(&self) -> usize {
        self.undoable.len()
    }

    pub fn num_redoable(&self) -> usize {
        self.redoable.len()
    }

    /// The labels for the undo dropdown, newest first.
    pub fn undo_descriptions(&self) -> Vec<String> {
        self.undoable.iter().map(UndoAction::description).collect()
    }

    /// The labels for the redo dropdown, newest first.
    pub fn redo_descriptions(&self) -> Vec<String> {
        self.redoable.iter().rev().map(UndoAction::description).collect()
    }

    pub fn item(&self, id: ShapeId) -> Option<&DisplayItem> {
        self.index_of(id).map(|i| &self.data.display_items[i])
    }

    /// The display item with this id, in constant time: the scene's shape order is the item order.
    #[inline]
    fn index_of(&self, id: ShapeId) -> Option<usize> {
        let i = self.scene.index_of(id)?;
        (self.data.display_items.get(i)?.id == id).then_some(i)
    }

    #[inline]
    fn visible(&self, shape: &Shape) -> bool {
        // PreviewCanvasVisibility.IsDisplayItemVisible
        !self.hide_locked || !shape.locked
    }

    // ----- element tree

    /// Replaces the element tree, which changes what a discrete element's lights are.
    pub fn set_tree(&mut self, tree: Option<Arc<ElementTree>>) -> Change {
        self.tree = tree;
        let mut ch = Change::default();
        self.rebuild_scene(&mut ch);
        self.end_update(&mut ch);
        ch
    }

    pub fn tree(&self) -> Option<&Arc<ElementTree>> {
        self.tree.as_ref()
    }

    /// The element tree's selection: `elementsForm.SelectedNode` is the first, and all are highlighted.
    /// `UpdateSelectedDisplayItems`.
    pub fn set_selected_nodes(&mut self, nodes: &[Uuid]) -> Change {
        let mut ch = Change::default();
        self.selected_nodes = nodes.to_vec();
        self.selected_node = nodes.first().copied();
        self.highlighted.clear();
        self.highlight_generation += 1;
        self.runs_dirty = true;
        for n in nodes {
            self.highlight_node(*n, &mut ch);
        }
        if self.selected_items.is_empty() {
            self.properties_shape = None;
            ch.clear_properties = true;
        }
        self.end_update(&mut ch);
        ch
    }

    /// `HighlightNode`: the node and everything under it, and the selected shape is deselected.
    fn highlight_node(&mut self, node: Uuid, ch: &mut Change) {
        // Is this a group? If so, iterate through children and highlight them
        if let Some(t) = self.tree.clone() {
            if let Some(n) = t.find(&node) {
                for c in t.descendants(n) {
                    self.highlighted.insert(t.node(c).id);
                }
            }
        }
        // Finally, highlight the node passed to us
        self.highlighted.insert(node);
        self.highlight_generation += 1;
        self.runs_dirty = true;
        self.deselect_selected_display_item(ch);
    }

    /// `elementsForm.SelectedNode`, as seen by a `Sel`.
    fn selected_sel(&self) -> Option<Sel<'_>> {
        Sel::from_uuid(self.tree.as_deref()?, self.selected_node?)
    }

    /// Shape.Reconfigure etc. after the host has run the element template the session asked for:
    /// `MouseUp`'s path when `ShowElementCreateTemplateForCurrentTool` succeeds.
    pub fn apply_element_template(&mut self, id: ShapeId, nodes: &[Uuid]) -> Change {
        let mut ch = Change::default();
        let Some(tree) = self.tree.clone() else { return ch };
        let Some(first) = nodes.first().and_then(|n| Sel::from_uuid(&tree, *n)) else { return ch };
        let Some(i) = self.index_of(id) else { return ch };
        // Associate the node with the shape and transfer any constraints from the node to the shape
        self.data.display_items[i].shape.reconfigure(Some(first));
        // Give the shape the opportunity to adjust the shape coordinates
        self.data.display_items[i].shape.end_add_new();
        self.rebuild_item(i, &mut ch);
        // Create additional shapes if requested by the template
        self.create_additional_shapes(i, nodes, &tree, &mut ch);
        self.end_update(&mut ch);
        ch
    }

    /// `CreateAdditionalShapes`: one more shape per further node, laid out in rows.
    fn create_additional_shapes(&mut self, i: usize, nodes: &[Uuid], tree: &ElementTree, ch: &mut Change) {
        if nodes.len() <= 1 {
            return;
        }
        let shape = &self.data.display_items[i].shape;
        let width = shape.right().wrapping_sub(shape.left()).wrapping_abs();
        let height = shape.bottom().wrapping_sub(shape.top()).wrapping_abs();
        // A cloned shape by re-hydrating, so the copies do not carry selection state.
        let mut template = shape.clone();
        template.roundtrip_state();
        // Initialize the position of the additional shape to the top left corner of the preview
        let (mut left, mut top) = (0, 0);
        let preview_width = self.view_size.0;
        for node in &nodes[1..] {
            let mut clone = template.clone();
            // Configure the zoom (otherwise the border does not render)
            clone.set_zoom_level(1.0);
            // Update the position of the cloned shape
            clone.set_top(top);
            clone.set_left(left);
            // Increment the position of the next shape
            left = left.wrapping_add(width);
            // If the next shape would extend past the edge of the preview then...
            if left.wrapping_add(width) > preview_width {
                // Move to the next row
                top = top.wrapping_add(height);
                // Start back at the left hand side of the preview
                left = 0;
            }
            // Only a moving head takes the further node, deliberately as in the C#.
            if matches!(clone.kind(), ShapeKind::MovingHead(_)) {
                clone.reconfigure(Sel::from_uuid(tree, *node));
            }
            self.add_display_item(clone, ch);
        }
    }

    /// The host's whole sequence once the wizard that [`Change::element_template`] asked for has made its elements: the tree with them in it, the drawn shape
    /// linked to them, and the tree's selection as the wizard left it. `shape` is `None` for the Elements panel's own Add button, which only adds elements.
    pub fn element_template_finished(&mut self, tree: Arc<ElementTree>, shape: Option<ShapeId>, nodes: &[Uuid], selection: &[Uuid]) -> Change {
        let mut ch = Change::default();
        self.absorb(&mut ch, |s| s.set_tree(Some(tree)));
        if let Some(shape) = shape {
            self.absorb(&mut ch, |s| s.apply_element_template(shape, nodes));
        }
        self.absorb(&mut ch, |s| s.set_selected_nodes(selection));
        ch
    }

    /// The host's half of `AddPropToPreviewAsync`, once `build_custom_prop` has put the prop's elements in `tree`: the root is selected as adding
    /// it to upstream's tree selects it, so the new prop's lights come up highlighted.
    pub fn custom_prop_imported(&mut self, tree: Arc<ElementTree>, root: Uuid, prop: Shape, x: i32, y: i32) -> Change {
        let mut ch = Change::default();
        self.absorb(&mut ch, |s| s.set_tree(Some(tree)));
        self.absorb(&mut ch, |s| s.set_selected_nodes(&[root]));
        self.absorb(&mut ch, |s| s.add_custom_prop(prop, x, y));
        ch
    }

    /// The element tree was edited: the new one, with links to nodes that are no longer in it dropped, as upstream's tree does before each delete
    /// (`UnlinkNodesFromDisplayItem`). A selected node that's gone leaves the selection.
    pub fn tree_replaced(&mut self, tree: Arc<ElementTree>) -> Change {
        let mut ch = Change::default();
        let kept: Vec<Uuid> = self.selected_nodes.iter().copied().filter(|n| tree.find(n).is_some()).collect();
        let selection_changed = kept.len() != self.selected_nodes.len();
        self.tree = Some(tree);
        self.absorb(&mut ch, |s| s.reload());
        if selection_changed {
            self.absorb(&mut ch, |s| s.set_selected_nodes(&kept));
        }
        ch
    }

    /// `Reload`: links to elements that no longer exist are dropped.
    pub fn reload(&mut self) -> Change {
        let mut ch = Change::default();
        if let Some(tree) = self.tree.clone() {
            for item in &mut self.data.display_items {
                if let ShapeKind::MovingHead(d) = &mut item.shape.kind {
                    // Validate the linked node still exists.
                    if !d.node_id.is_nil() && tree.find(&d.node_id).is_none() {
                        d.node_id = Uuid::nil();
                    }
                } else {
                    item.shape.for_each_pixel_mut(&mut |p| {
                        //Validate the linked node still exists.
                        if !p.node_id.is_nil() && tree.find(&p.node_id).is_none() {
                            p.node_id = Uuid::nil();
                        }
                    });
                }
            }
        }
        self.rebuild_scene(&mut ch);
        self.end_update(&mut ch);
        ch
    }

    /// `UnlinkNodesFromDisplayItem`: these elements are being deleted, so nothing may link to them. Give the
    /// node and its descendants.
    pub fn unlink_nodes(&mut self, nodes: &[Uuid]) -> Change {
        let mut ch = Change::default();
        let gone: HashSet<Uuid> = nodes.iter().copied().collect();
        for item in &mut self.data.display_items {
            if let ShapeKind::MovingHead(d) = &mut item.shape.kind {
                if gone.contains(&d.node_id) {
                    d.node_id = Uuid::nil();
                }
            } else {
                item.shape.for_each_pixel_mut(&mut |p| {
                    if gone.contains(&p.node_id) {
                        p.node_id = Uuid::nil();
                    }
                });
            }
        }
        self.rebuild_scene(&mut ch);
        self.end_update(&mut ch);
        ch
    }

    /// `HasInValidElementMapping`, across the preview: shapes linked to a node with no element.
    pub fn invalid_element_links(&self) -> Vec<ShapeId> {
        let Some(tree) = self.tree.as_deref() else { return Vec::new() };
        self.data
            .display_items
            .iter()
            .filter(|item| {
                item.shape.is_light_shape() && item.shape.any_pixel(&mut |p| !p.node_id.is_nil() && tree.find(&p.node_id).is_some_and(|n| tree.node(n).element_id.is_none()))
            })
            .map(|item| item.id)
            .collect()
    }

    /// `CheckForIntelligentFixturesAndOpenGL`: fixtures need the GPU preview, which is all this port has, so
    /// this only says whether the saved flag should be turned on.
    pub fn fixtures_without_opengl(&self) -> bool {
        !self.data.use_open_gl && self.data.display_items.iter().any(|i| matches!(i.shape.kind(), ShapeKind::MovingHead(_)))
    }

    // ----- plumbing

    /// `AddDisplayItem`, plus the scene.
    fn add_display_item(&mut self, shape: Shape, ch: &mut Change) -> ShapeId {
        let id = self.data.add_shape(shape);
        self.scene_push_last(ch);
        ch.shapes_added.push(id);
        id
    }

    fn scene_push_last(&mut self, ch: &mut Change) {
        if let Some(item) = self.data.display_items.last() {
            let delta = self.scene.add_shape(item, self.tree.as_deref());
            ch.scene.push(delta);
        }
        self.runs_dirty = true;
        ch.overlay_changed = true;
    }

    /// Puts a whole display item back, keeping its id.
    fn restore_display_item(&mut self, item: DisplayItem, ch: &mut Change) {
        ch.shapes_added.push(item.id);
        self.data.display_items.push(item);
        self.scene_push_last(ch);
    }

    /// `RemoveDisplayItem`. `List.Remove` goes by `DisplayItem.Equals`, so the first shape with the same bounds is
    /// the one that goes. Deliberate, as in the C#. Unlike the C#, its undo restores that same shape.
    fn remove_display_item(&mut self, id: ShapeId, ch: &mut Change) -> Option<DisplayItem> {
        let key = self.item(id)?.shape.bounds_key();
        let i = self.data.display_items.iter().position(|x| x.shape.bounds_key() == key)?;
        self.remove_at(i, ch)
    }

    fn remove_exact(&mut self, id: ShapeId, ch: &mut Change) -> Option<DisplayItem> {
        let i = self.index_of(id)?;
        self.remove_at(i, ch)
    }

    fn remove_at(&mut self, i: usize, ch: &mut Change) -> Option<DisplayItem> {
        if i >= self.data.display_items.len() {
            return None;
        }
        let item = self.data.display_items.remove(i);
        if let Some(delta) = self.scene.remove_shape(item.id) {
            ch.scene.push(delta);
        }
        ch.shapes_removed.push(item.id);
        // The C# carries on selecting an object no longer in the list. There is nothing useful it can do then.
        if self.selected_item == Some(item.id) {
            self.selected_item = None;
            ch.selection_changed = true;
        }
        self.selected_items.retain(|s| *s != item.id);
        self.handle_items.retain(|s| *s != item.id);
        self.runs_dirty = true;
        ch.overlay_changed = true;
        Some(item)
    }

    /// A shape's lights may have moved. This is the path a mouse move takes.
    #[inline]
    fn touch(&mut self, i: usize, ch: &mut Change) {
        let tree = self.tree.as_deref();
        let item = &mut self.data.display_items[i];
        item.invalidate_hit();
        if item.shape.select_points.is_some() && !self.handle_items.contains(&item.id) {
            self.handle_items.push(item.id);
        }
        if let Some(delta) = self.scene.update_shape(item, tree) {
            if delta.resized || delta.links_changed {
                item.runs = None;
                self.runs_dirty = true;
            }
            ch.scene.push(delta);
        }
        ch.overlay_changed = true;
    }

    /// A shape's links or light count may have changed.
    fn rebuild_item(&mut self, i: usize, ch: &mut Change) {
        let tree = self.tree.as_deref();
        let item = &mut self.data.display_items[i];
        item.invalidate_hit();
        item.runs = None;
        if item.shape.select_points.is_some() && !self.handle_items.contains(&item.id) {
            self.handle_items.push(item.id);
        }
        if let Some(delta) = self.scene.rebuild_shape(item, tree) {
            ch.scene.push(delta);
        }
        self.runs_dirty = true;
        ch.overlay_changed = true;
    }

    fn rebuild_scene(&mut self, ch: &mut Change) {
        self.scene.rebuild_all(&self.data, self.tree.as_deref());
        for item in &mut self.data.display_items {
            item.invalidate_hit();
            item.runs = None;
        }
        ch.scene.push(SceneDelta { shape: None, range: 0..self.scene.light_count(), links_changed: true, resized: true });
        self.runs_dirty = true;
        ch.overlay_changed = true;
    }

    /// `EndUpdate`, without the drawing: what `RenderInForeground` changes or needs in place before a frame.
    fn end_update(&mut self, ch: &mut Change) {
        // DrawSelectPoints moves the rotate handles and can run the transform's side effect.
        let mut n = 0;
        while n < self.handle_items.len() {
            let id = self.handle_items[n];
            match self.index_of(id) {
                Some(i) if self.data.display_items[i].shape.select_points.is_some() => {
                    self.data.display_items[i].shape.prepare_overlay();
                    n += 1;
                }
                _ => {
                    self.handle_items.swap_remove(n);
                }
            }
        }
        if self.runs_dirty {
            self.rebuild_runs();
            ch.overlay_changed = true;
            ch.light_colors_changed = true;
        }
    }

    fn selected_keys(&self) -> HashSet<(i32, i32, i32, i32)> {
        self.selected_items.iter().filter_map(|id| self.item(*id)).map(|i| i.shape.bounds_key()).collect()
    }

    fn rebuild_runs(&mut self) {
        let selected_keys = self.selected_keys();
        let mut ctx = RunContext {
            data: &mut self.data,
            scene: &self.scene,
            selected_keys,
            highlighted: &self.highlighted,
            highlight_generation: self.highlight_generation,
            hide_locked: self.hide_locked,
        };
        self.runs = Arc::new(build_light_runs(&mut ctx));
        self.runs_dirty = false;
    }

    /// What to draw over the lights. Costs nothing per light: the runs are shared, and only change when
    /// selection, links or highlights do.
    pub fn overlay(&self) -> Overlay {
        let mut o = Overlay { light_runs: Arc::clone(&self.runs), ..Overlay::default() };
        for id in &self.handle_items {
            if let Some(item) = self.item(*id) {
                if self.visible(&item.shape) {
                    overlay::add_select_points(&item.shape, &mut o);
                }
            }
        }
        for f in &self.scene.fixtures {
            if let Some(item) = self.item(f.shape) {
                if self.visible(&item.shape) {
                    let selected = item.shape.selected || self.selected_contains(f.shape);
                    overlay::add_fixture(f.shape, &item.shape, selected, &self.highlighted, &mut o);
                }
            }
        }
        if self.show_info {
            for item in &self.data.display_items {
                if self.visible(&item.shape) {
                    overlay::add_info(&item.shape, &mut o);
                }
            }
        }
        // Finally, are we drawing a banded rectangle?
        if self.mouse_captured && self.selected_item.is_none() {
            o.rubber_band = overlay::rubber_band(self.band_rect);
        }
        o.take_off_zoom(self.zoom as f32);
        o
    }

    // ----- selection helpers

    /// `SelectedDisplayItems.Contains(item)`.
    fn selected_contains(&self, id: ShapeId) -> bool {
        if self.selected_items.is_empty() {
            return false;
        }
        let Some(key) = self.item(id).map(|i| i.shape.bounds_key()) else { return false };
        self.selected_items.iter().any(|s| self.item(*s).is_some_and(|i| i.shape.bounds_key() == key))
    }

    /// `SelectedDisplayItems.Remove(item)`.
    fn selected_remove(&mut self, id: ShapeId) {
        let Some(key) = self.item(id).map(|i| i.shape.bounds_key()) else { return };
        if let Some(pos) = self.selected_items.iter().position(|s| self.item(*s).is_some_and(|i| i.shape.bounds_key() == key)) {
            self.selected_items.remove(pos);
        }
    }

    /// `RemoveHiddenDisplayItemsFromSelection`.
    fn remove_hidden_from_selection(&mut self, ch: &mut Change) {
        if let Some(id) = self.selected_item {
            if self.item(id).is_some_and(|i| !self.visible(&i.shape)) {
                self.deselect_selected_display_item(ch);
            }
        }
        let hide = self.hide_locked;
        let data = &self.data;
        let scene = &self.scene;
        self.selected_items.retain(|id| {
            let Some(i) = scene.index_of(*id) else { return true };
            !(hide && data.display_items[i].shape.locked)
        });
    }

    /// `DeSelectSelectedDisplayItem`.
    fn deselect_selected_display_item(&mut self, ch: &mut Change) {
        if let Some(id) = self.selected_item {
            // OnDeSelectDisplayItem
            self.properties_shape = None;
            ch.clear_properties = true;
            if let Some(i) = self.index_of(id) {
                self.data.display_items[i].shape.deselect();
            }
            self.selected_item = None;
            ch.selection_changed = true;
            self.runs_dirty = true;
            ch.overlay_changed = true;
        }
    }

    /// `OnSelectDisplayItem` in VixenPreviewSetup3: picking another shape clears the element tree's selection.
    fn on_select_display_item(&mut self, id: ShapeId, ch: &mut Change) {
        if self.properties_shape != Some(id) {
            self.selected_nodes.clear();
            self.selected_node = None;
            if !self.highlighted.is_empty() {
                self.highlighted.clear();
                self.highlight_generation += 1;
                self.runs_dirty = true;
            }
            ch.elements_deselected = true;
            self.properties_shape = Some(id);
            ch.show_properties = Some(id);
        }
    }

    /// `DisplayItemAtPoint`.
    fn display_item_at_point(&mut self, point: PreviewPoint, select_override: bool) -> Option<ShapeId> {
        for i in 0..self.data.display_items.len() {
            if !self.visible(&self.data.display_items[i].shape) {
                continue;
            }
            // PointInShape first, since it is the cheap test here and has no side effects.
            if !self.data.display_items[i].point_in_shape(point) {
                continue;
            }
            let item = &self.data.display_items[i];
            let selected = if self.selected_items.is_empty() { item.shape.selected } else { self.selected_contains(item.id) };
            if selected || select_override || !item.shape.locked {
                return Some(item.id);
            }
        }
        None
    }

    /// `MouseOverSelectedDisplayItems`.
    fn mouse_over_selected_display_items(&mut self, x: i32, y: i32) -> bool {
        for n in 0..self.selected_items.len() {
            let Some(i) = self.index_of(self.selected_items[n]) else { continue };
            if self.visible(&self.data.display_items[i].shape) && self.data.display_items[i].point_in_shape(PreviewPoint::new(x, y)) {
                return true;
            }
        }
        false
    }

    /// `SelectItemUnderPoint`.
    fn select_item_under_point(&mut self, point: PreviewPoint, add_to_selection: bool, select_override: bool, ch: &mut Change) {
        if self.mouse_captured {
            return;
        }
        let was = (self.selected_item, self.selected_items.len());
        // First, see if we have an item already selected, but want to add to it
        if add_to_selection {
            if let Some(id) = self.selected_item {
                self.selected_items.push(id);
            }
            let item = self.display_item_at_point(point, select_override);
            self.deselect_selected_display_item(ch);
            if let Some(id) = item {
                self.selected_items.push(id);
            }
        } else {
            let temp = self.display_item_at_point(point, select_override);
            self.deselect_selected_display_item(ch);
            self.selected_item = temp;
            if let Some(id) = temp {
                self.select_shape(id, ch);
                self.on_select_display_item(id, ch);
            }
        }
        ch.selection_changed = true;
        if was != (self.selected_item, self.selected_items.len()) {
            self.runs_dirty = true;
            ch.overlay_changed = true;
        }
    }

    /// `Shape.Select(true)`.
    fn select_shape(&mut self, id: ShapeId, ch: &mut Change) {
        if let Some(i) = self.index_of(id) {
            self.data.display_items[i].shape.select(true);
            if !self.handle_items.contains(&id) {
                self.handle_items.push(id);
            }
            self.runs_dirty = true;
            ch.overlay_changed = true;
        }
    }

    /// `SelectItems`: replaces the selection with these shapes, as the invalid links warning does.
    pub fn select_items(&mut self, ids: &[ShapeId]) -> Change {
        let mut ch = Change::default();
        // First, deselect any currently selected item
        self.deselect_selected_display_item(&mut ch);
        for id in ids {
            if self.item(*id).is_some_and(|i| self.visible(&i.shape)) {
                self.selected_items.push(*id);
            }
        }
        ch.selection_changed = true;
        self.runs_dirty = true;
        self.end_update(&mut ch);
        ch
    }

    /// Selects every unlocked visible shape, as a rubber band around all of them would: a single shape becomes the
    /// selected item. The C# has no such command.
    pub fn select_all(&mut self) -> Change {
        let mut ch = Change::default();
        self.deselect_selected_display_item(&mut ch);
        self.selected_items.clear();
        // An unlocked shape is always visible.
        self.selected_items.extend(self.data.display_items.iter().filter(|i| !i.shape.locked).map(|i| i.id));
        if self.selected_items.len() == 1 {
            let id = self.selected_items[0];
            self.selected_item = Some(id);
            self.select_shape(id, &mut ch);
            self.on_select_display_item(id, &mut ch);
            self.selected_items.clear();
        }
        ch.selection_changed = true;
        self.runs_dirty = true;
        self.end_update(&mut ch);
        ch
    }

    /// `SelectedDisplayItems.Clear()` and the single item, as a click on empty space does.
    pub fn clear_selection(&mut self) -> Change {
        let mut ch = Change::default();
        self.deselect_selected_display_item(&mut ch);
        self.selected_items.clear();
        ch.selection_changed = true;
        self.runs_dirty = true;
        self.end_update(&mut ch);
        ch
    }

    /// `HideLockedDisplayItems`.
    pub fn set_hide_locked(&mut self, hide: bool) -> Change {
        let mut ch = Change::default();
        if self.hide_locked == hide {
            return ch;
        }
        self.hide_locked = hide;
        self.remove_hidden_from_selection(&mut ch);
        ch.selection_changed = true;
        self.runs_dirty = true;
        self.end_update(&mut ch);
        ch
    }

    pub fn hide_locked(&self) -> bool {
        self.hide_locked
    }

    /// `PreviewCanvasVisibility.IsHideLockedCommandEnabled`.
    pub fn hide_locked_command_enabled(&self) -> bool {
        self.hide_locked || self.data.display_items.iter().any(|i| i.shape.locked)
    }

    /// The Prop Information menu item: `ShowInfo`.
    pub fn set_show_info(&mut self, show: bool) -> Change {
        self.show_info = show;
        Change { overlay_changed: true, ..Change::default() }
    }

    // ----- tools

    /// `CurrentTool`, set by the toolbar buttons: the next mouse drag draws this shape.
    pub fn set_tool(&mut self, tool: Tool) -> Change {
        let mut ch = Change::default();
        self.item_index = 1;
        self.item_name.clear();
        self.draw_shape = tool.draw_shape_name().to_string();
        self.set_current_tool(tool, &mut ch);
        self.end_update(&mut ch);
        ch
    }

    /// `CurrentTool` setter.
    fn set_current_tool(&mut self, tool: Tool, ch: &mut Change) {
        self.tool = tool;
        ch.tool_changed = true;
        if self.selected_item.is_some() {
            self.deselect_selected_display_item(ch);
        }
    }

    /// Ctrl-clicking the pixel tool: the name prefix, first index and light size for Singles drawn from now on.
    pub fn set_single_item_defaults(&mut self, name: &str, start_index: i32, bulb_size: i32) {
        self.item_name = name.to_string();
        self.item_index = start_index;
        self.item_bulb_size = bulb_size;
    }

    /// Adds a shape of this kind at a default place, by drawing it through the same path as the mouse: from
    /// (10, 10) to (110, 110). The C# has only the drawing. The tool stays as it was before.
    pub fn add_shape(&mut self, kind: Tool) -> Change {
        let mut ch = Change::default();
        if kind == Tool::Select {
            return ch;
        }
        let previous = self.tool;
        let keep = self.data.keep_insert_mode;
        self.data.keep_insert_mode = true;
        let scroll = (self.h_scroll, self.v_scroll);
        self.h_scroll = 0;
        self.v_scroll = 0;
        self.absorb(&mut ch, |s| s.set_tool(kind));
        self.absorb(&mut ch, |s| s.mouse_down(10, 10, Modifiers::NONE));
        self.absorb(&mut ch, |s| s.mouse_move(110, 110, Modifiers::NONE));
        self.absorb(&mut ch, |s| s.mouse_up(110, 110, Modifiers::NONE));
        if matches!(kind, Tool::PolyLine | Tool::MultiString) {
            // Those keep drawing until Escape: add the point that Escape will discard.
            self.absorb(&mut ch, |s| s.mouse_move(120, 110, Modifiers::NONE));
            self.absorb(&mut ch, |s| s.key_down(Key::Escape, Modifiers::NONE));
        }
        self.absorb(&mut ch, |s| s.set_tool(previous));
        self.data.keep_insert_mode = keep;
        self.h_scroll = scroll.0;
        self.v_scroll = scroll.1;
        ch
    }

    fn absorb(&mut self, into: &mut Change, f: impl FnOnce(&mut DesignSession) -> Change) {
        let c = f(self);
        into.scene.extend(c.scene);
        into.overlay_changed |= c.overlay_changed;
        into.light_colors_changed |= c.light_colors_changed;
        into.selection_changed |= c.selection_changed;
        into.shapes_added.extend(c.shapes_added);
        into.shapes_removed.extend(c.shapes_removed);
        into.cursor = c.cursor.or(into.cursor);
        into.show_properties = c.show_properties.or(into.show_properties);
        into.clear_properties |= c.clear_properties;
        into.elements_deselected |= c.elements_deselected;
        into.element_template = c.element_template.or(into.element_template);
        into.context_menu = c.context_menu.or(into.context_menu.take());
        into.zoom_changed = c.zoom_changed.or(into.zoom_changed);
        into.scroll_changed |= c.scroll_changed;
        into.tool_changed |= c.tool_changed;
        into.clipboard = c.clipboard.or(into.clipboard.take());
        into.message = c.message.or(into.message.take());
        into.lock_hint |= c.lock_hint;
        into.location_updates.extend(c.location_updates);
        into.undo_changed |= c.undo_changed;
    }

    /// Builds the shape the current tool draws.
    fn new_shape_for_tool(&self, p: PreviewPoint) -> Option<Shape> {
        let node = self.selected_sel();
        let zoom = self.zoom;
        Some(match self.tool {
            Tool::Select => return None,
            Tool::String => Shape::new_line(p, p, 50, node, zoom),
            Tool::Arch => Shape::new_arch(p, node, zoom),
            Tool::Rectangle => Shape::new_rectangle(p, node, zoom),
            Tool::Single => {
                let mut s = Shape::new_single(p, node, zoom);
                if !self.item_name.is_empty() {
                    s.name = Some(format!("{}{}", self.item_name, self.item_index));
                    s.set_pixel_size(self.item_bulb_size);
                }
                s
            }
            Tool::Ellipse => Shape::new_ellipse(p, node, zoom),
            Tool::Triangle => Shape::new_triangle(p, node, zoom),
            Tool::Net => Shape::new_net(p, node, zoom),
            Tool::Cane => Shape::new_cane(p, node, zoom),
            Tool::Star => Shape::new_star(p, node, zoom),
            Tool::StarBurst => Shape::new_star_burst(p, node, zoom),
            Tool::Flood => Shape::new_flood(p, node),
            Tool::MegaTree => Shape::new_mega_tree(p, node, zoom),
            Tool::PixelGrid => Shape::new_pixel_grid(p, node, zoom),
            Tool::Icicle => Shape::new_icicle(p, p, node, zoom),
            Tool::PolyLine => Shape::new_poly_line(p, node, zoom),
            Tool::MultiString => Shape::new_multi_string(p, node, zoom),
            Tool::MovingHead => Shape::new_moving_head(p, node, zoom),
        })
    }

    // ----- mouse

    /// `MouseDown` with the left button.
    pub fn mouse_down(&mut self, x: i32, y: i32, modifiers: Modifiers) -> Change {
        self.mouse_down_with(MouseButton::Left, x, y, modifiers)
    }

    /// `MouseDown`. Coordinates are in the view, and the session adds its scroll offset.
    pub fn mouse_down_with(&mut self, button: MouseButton, x: i32, y: i32, m: Modifiers) -> Change {
        let mut ch = Change::default();
        self.mouse_client = (x, y);
        self.element_selected = self.selected_node;
        let translated = PreviewPoint::new(x.wrapping_add(self.h_scroll), y.wrapping_add(self.v_scroll));
        match button {
            MouseButton::Left => {
                if self.mouse_down_left(translated, m, &mut ch) {
                    self.end_update(&mut ch);
                    return ch;
                }
            }
            MouseButton::Right => self.mouse_down_right(translated, m, &mut ch),
            MouseButton::Middle => {
                // Pan
                self.zoom_to = self.mouse_point_to_zoom_point(x, y);
                self.panning = true;
            }
        }
        self.end_update(&mut ch);
        ch
    }

    /// True for the paths that return from the C# handler early.
    fn mouse_down_left(&mut self, translated: PreviewPoint, m: Modifiers, ch: &mut Change) -> bool {
        let (control_pressed, shift_pressed) = (m.ctrl, m.shift);
        if self.tool == Tool::Select {
            if control_pressed {
                let item = self.display_item_at_point(translated, shift_pressed);
                match item {
                    Some(id) if self.selected_contains(id) => {
                        self.selected_remove(id);
                        ch.selection_changed = true;
                        self.runs_dirty = true;
                    }
                    _ => self.select_item_under_point(translated, true, shift_pressed, ch),
                }
                return true;
            }

            // Is there a single display item selected? (controlPressed is not true here)
            if let Some(i) = self.selected_item.and_then(|id| self.index_of(id)) {
                // Lets see if we've got a drag or rotation point.
                let selected_point = self.data.display_items[i].shape.point_in_select_point(translated);
                if let Some(point) = selected_point {
                    self.modify_type = ModifyType::Resize;
                    self.begin_resize_move(false); //Starts the Undo Process
                    self.drag_start = (translated.x, translated.y);
                    self.data.display_items[i].shape.set_select_point(SelectArg::Point(point));
                    self.mouse_captured = true;
                } else if self.data.display_items[i].point_in_shape(translated) {
                    // If we're not resizing, see if we're moving a single shape
                    self.modify_type = ModifyType::Move;
                    self.begin_resize_move(false); //Starts the Undo Process
                    self.start_move(translated.x, translated.y, false);
                } else {
                    // If we get here, we're outside the shape, deselect
                    self.deselect_selected_display_item(ch);
                }
            } else if self.selected_items.len() > 1 {
                // Are there multiple items selected? If so, we're moving, can't resize them...
                if self.mouse_over_selected_display_items(translated.x, translated.y) {
                    self.modify_type = ModifyType::Move;
                    self.begin_resize_move(true); //Starts the Undo Process
                    self.start_move(translated.x, translated.y, false);
                } else {
                    self.selected_items.clear();
                    ch.selection_changed = true;
                    self.runs_dirty = true;
                }
            }

            self.select_item_under_point(translated, false, shift_pressed, ch);

            // If we get this far, and we've got nothing selected, we're drawing a rubber band!
            if self.selected_item.is_none() && self.selected_items.is_empty() {
                // Capture the mouse in case we want to draw a rubber band
                self.drag_start = (translated.x, translated.y);
                self.mouse_captured = true;
                ch.selection_changed = true;
                self.band_rect.width = 0;
                self.band_rect.height = 0;
                self.banding = true;
            }
            return false;
        }

        let over_selected = self.selected_item.and_then(|id| self.index_of(id)).is_some_and(|i| {
            let visible = self.visible(&self.data.display_items[i].shape);
            let creating = self.data.display_items[i].shape.creating;
            visible && !creating && self.data.display_items[i].point_in_shape(translated)
        });
        if over_selected {
            self.start_move(translated.x, translated.y, false);
        } else if self.mouse_captured && matches!(self.tool, Tool::PolyLine | Tool::MultiString) {
            // If we're not Selecting items, we're drawing them
            return true;
        } else {
            self.deselect_selected_display_item(ch);
            if let Some(shape) = self.new_shape_for_tool(translated) {
                if self.tool == Tool::Single && !self.item_name.is_empty() {
                    self.item_index += 1;
                }
                // Now add the newly created display item to the screen.
                let zoom = self.zoom;
                let id = self.add_display_item(shape, ch);
                let i = self.data.display_items.len() - 1;
                self.data.display_items[i].shape.set_zoom_level(zoom);
                self.selected_item = Some(id);
                self.select_shape(id, ch);
                self.data.display_items[i].shape.select_default_select_point();
                self.drag_start = (translated.x, translated.y);
                self.mouse_captured = true;
                self.modify_type = ModifyType::AddNew;
                self.on_select_display_item(id, ch);
                self.touch(i, ch);
                ch.selection_changed = true;
            }
        }
        false
    }

    fn mouse_down_right(&mut self, translated: PreviewPoint, m: Modifiers, ch: &mut Change) {
        self.select_item_under_point(translated, false, m.shift, ch);
        let mut menu = Vec::new();
        let single = self.selected_item.and_then(|id| self.index_of(id));
        if let Some(i) = single {
            if self.data.display_items[i].point_in_shape(translated) && matches!(self.data.display_items[i].shape.kind(), ShapeKind::Custom { .. }) {
                menu.push(MenuItem::Ungroup);
            }
        } else if self.selected_items.len() > 1 && !self.selected_items.iter().any(|id| self.item(*id).is_some_and(|i| matches!(i.shape.kind(), ShapeKind::CustomProp(_)))) {
            menu.push(MenuItem::Group);
            menu.push(MenuItem::CreateTemplate);
        }
        if single.is_some() {
            menu.push(MenuItem::Cut);
            menu.push(MenuItem::Copy);
        }
        if self.clipboard_populated {
            menu.push(MenuItem::Paste);
        }
        if single.is_some() {
            menu.push(MenuItem::Delete);
        }
        if !self.undoable.is_empty() {
            menu.push(MenuItem::Undo);
        }
        if !self.redoable.is_empty() {
            menu.push(MenuItem::Redo);
        }
        if let Some(i) = single {
            menu.push(if self.data.display_items[i].shape.locked { MenuItem::Unlock } else { MenuItem::Lock });
            if self.data.save_locations {
                menu.push(MenuItem::SetZ);
            }
        }
        if !menu.is_empty() {
            ch.context_menu = Some(menu);
        }
    }

    /// `StartMove`.
    fn start_move(&mut self, x: i32, y: i32, paste: bool) {
        self.drag_start = (x, y);
        // If paste is true, then we'll send a point of 0,0 which indicates a full position reset.
        let arg = if paste { SelectArg::Reset } else { SelectArg::Null };
        if !self.selected_items.is_empty() {
            for n in 0..self.selected_items.len() {
                if let Some(i) = self.index_of(self.selected_items[n]) {
                    self.data.display_items[i].shape.set_select_point(arg);
                }
            }
        } else if let Some(i) = self.selected_item.and_then(|id| self.index_of(id)) {
            self.data.display_items[i].shape.set_select_point(arg);
        }
        self.mouse_captured = true;
    }

    /// `beginResize_Move`: the shapes as they are now, for undo.
    fn begin_resize_move(&mut self, multi: bool) {
        let ids: Vec<ShapeId> = if multi {
            self.selected_items.clone()
        } else {
            self.selected_items.clear();
            self.selected_item.into_iter().collect()
        };
        self.move_info = Some(ids.iter().filter_map(|id| self.item(*id).map(|i| (*id, i.shape.clone()))).collect());
    }

    /// `MouseMove`. Hit testing is skipped during a drag, where the C#'s cursor update finds the same answer it
    /// found on the way in.
    pub fn mouse_move(&mut self, x: i32, y: i32, m: Modifiers) -> Change {
        let mut ch = Change::default();
        self.mouse_client = (x, y);
        let translated = (x.wrapping_add(self.h_scroll), y.wrapping_add(self.v_scroll));
        if self.panning {
            // Woo hoo... we're panning with the middle mouse button
            let to = self.zoom_to;
            self.set_background_position(to, x, y, &mut ch);
            self.end_update(&mut ch);
            return ch;
        }
        let zoom_point = self.point_to_zoom_point(translated);
        let change_x = translated.0.wrapping_sub(self.drag_start.0);
        let change_y = translated.1.wrapping_sub(self.drag_start.1);

        // Are we moving a single display item?
        if let (true, Some(i)) = (self.mouse_captured, self.selected_item.and_then(|id| self.index_of(id))) {
            if self.visible(&self.data.display_items[i].shape) {
                self.data.display_items[i].shape.mouse_move(translated.0, translated.1, change_x, change_y, m);
                self.touch(i, &mut ch);
            }
        } else if self.banding {
            // If we get here, we're drawing a rubber band
            self.update_rubber_band(change_x, change_y, m, &mut ch);
        } else if self.mouse_captured && self.selected_item.is_none() && !self.selected_items.is_empty() {
            // Are we moving a group of display items?
            for n in 0..self.selected_items.len() {
                let Some(i) = self.index_of(self.selected_items[n]) else { continue };
                if self.visible(&self.data.display_items[i].shape) {
                    self.data.display_items[i].shape.mouse_move(zoom_point.0, zoom_point.1, change_x, change_y, m);
                    self.touch(i, &mut ch);
                }
            }
        }

        if !self.mouse_captured {
            self.update_cursor(PreviewPoint::new(translated.0, translated.1), &mut ch);
        }
        self.end_update(&mut ch);
        ch
    }

    /// The rubber band part of `MouseMove`.
    fn update_rubber_band(&mut self, change_x: i32, change_y: i32, m: Modifiers, ch: &mut Change) {
        let x1 = self.drag_start.0.min(self.drag_start.0.wrapping_add(change_x));
        let y1 = self.drag_start.1.min(self.drag_start.1.wrapping_add(change_y));
        self.band_rect = Rect::new(x1, y1, change_x.wrapping_abs(), change_y.wrapping_abs());
        ch.overlay_changed = true;

        let band = self.band_rect;
        let all_in = change_x > 0;
        let override_lock = m.shift;
        // `SelectedDisplayItems.Contains` goes by bounds, once per item, which this keeps constant time.
        let mut keys: HashMap<(i32, i32, i32, i32), u32> = HashMap::with_capacity(self.selected_items.len());
        for id in &self.selected_items {
            if let Some(item) = self.item(*id) {
                *keys.entry(item.shape.bounds_key()).or_insert(0) += 1;
            }
        }
        let mut changed = false;
        for i in 0..self.data.display_items.len() {
            let id = self.data.display_items[i].id;
            let locked = self.data.display_items[i].shape.locked;
            let key = self.data.display_items[i].shape.bounds_key();
            let contained = keys.get(&key).is_some_and(|c| *c > 0);
            let mut remove = false;
            if !self.visible(&self.data.display_items[i].shape) {
                remove = contained;
            } else if self.data.display_items[i].shape_in_rect(band, all_in) {
                if !contained && (override_lock || !locked) {
                    self.selected_items.push(id);
                    *keys.entry(key).or_insert(0) += 1;
                    changed = true;
                }
            } else if contained && (override_lock || !locked) {
                remove = true;
            }
            if remove {
                self.selected_remove(id);
                if let Some(c) = keys.get_mut(&key) {
                    *c -= 1;
                }
                changed = true;
            }
        }
        if changed {
            ch.selection_changed = true;
            self.runs_dirty = true;
        }
    }

    /// The cursor part of `MouseMove`.
    fn update_cursor(&mut self, point: PreviewPoint, ch: &mut Change) {
        let cursor = if let Some(i) = self.selected_item.and_then(|id| self.index_of(id)) {
            let select_point = self.data.display_items[i].shape.point_in_select_point(point);
            if let Some(sp) = select_point {
                if self.data.display_items[i].shape.is_rotate_handle(sp) {
                    Some(Cursor::Rotate)
                } else {
                    Some(Cursor::Cross)
                }
            } else if self.data.display_items[i].point_in_shape(point) {
                Some(Cursor::SizeAll)
            } else {
                Some(Cursor::Default)
            }
        } else if !self.selected_items.is_empty() {
            self.mouse_over_selected_display_items(point.x, point.y).then_some(Cursor::SizeAll)
        } else if self.tool != Tool::Select {
            Some(Cursor::InsertMode)
        } else {
            None
        };
        if let Some(c) = cursor {
            self.cursor = c;
            ch.cursor = Some(c);
        }
    }

    /// `MouseUp`. For a drawing tool that draws point by point, the shape keeps the mouse until Escape.
    pub fn mouse_up(&mut self, x: i32, y: i32, m: Modifiers) -> Change {
        self.mouse_up_with(MouseButton::Left, x, y, m)
    }

    /// `MouseUp`.
    pub fn mouse_up_with(&mut self, button: MouseButton, x: i32, y: i32, _m: Modifiers) -> Change {
        let mut ch = Change::default();
        self.mouse_client = (x, y);
        if button == MouseButton::Middle {
            self.panning = false;
        }
        let left = button == MouseButton::Left;
        if self.mouse_captured {
            let single = self.selected_item.and_then(|id| self.index_of(id));
            if matches!(self.tool, Tool::PolyLine | Tool::MultiString) {
                if let Some(i) = single {
                    if left && self.data.display_items[i].shape.creating {
                        // If we are drawing a PolyLine/Multi String, we want all the mouse events to be passed to the shape
                        let tree = self.tree.clone();
                        self.data.display_items[i].shape.mouse_up(x, y, left, tree.as_deref());
                        self.touch(i, &mut ch);
                        self.end_update(&mut ch);
                        return ch;
                    }
                }
            }
            // If an intelligent fixture (moving head) is being created then tell it to ignore any mouse
            // movements since the mouse up was released
            if self.tool == Tool::MovingHead {
                if let Some(i) = single {
                    self.data.display_items[i].shape.set_moving_head_ignore_mouse(true);
                }
            }
            if let Some(i) = single {
                self.finish_single_item_drag(i, x, y, left, &mut ch);
            } else if !self.selected_items.is_empty() {
                // Ok, if this is true, we've got a rubber band and something is selected, now what?
                if self.selected_items.len() == 1 {
                    // If we only have one item, just select it and go on.
                    let id = self.selected_items[0];
                    self.selected_item = Some(id);
                    self.select_shape(id, &mut ch);
                    self.on_select_display_item(id, &mut ch);
                    self.selected_items.clear();
                    ch.selection_changed = true;
                    self.runs_dirty = true;
                } else if let Some(info) = self.move_info.clone() {
                    // The C# leaves this info in place, so a later rubber band around several shapes pushes this
                    // same move again. Deliberate.
                    self.push_undo(UndoAction::MoveResize { saved: info, resize: false }, &mut ch);
                }
            }
        }
        self.reset_mouse();
        ch.overlay_changed = true;
        self.end_update(&mut ch);
        ch
    }

    fn finish_single_item_drag(&mut self, i: usize, x: i32, y: i32, left: bool, ch: &mut Change) {
        let id = self.data.display_items[i].id;
        if self.element_selected.is_none() && self.modify_type == ModifyType::AddNew {
            // Intercept and populate the element tree. The template dialog is the host's, and it comes back
            // through apply_element_template, which reconfigures the shape then.
            if let Some(template) = self.tool.element_template() {
                ch.element_template = Some((id, template));
            }
            // Give the shape the opportunity to adjust the shape coordinates
            self.data.display_items[i].shape.end_add_new();
            // If Keep Insert mode is off, then reset the current prop state to Select which effectively
            // stops adding new props and the cursor will ultimately reset to the default shape.
            if !self.data.keep_insert_mode {
                self.tool = Tool::Select;
                ch.tool_changed = true;
            }
        } else if self.modify_type == ModifyType::AddNew {
            // Give the shape the opportunity to adjust the shape coordinates
            self.data.display_items[i].shape.end_add_new();
        }

        let tree = self.tree.clone();
        self.data.display_items[i].shape.mouse_up(x, y, left, tree.as_deref());
        self.on_select_display_item(id, ch);
        if self.move_info.is_some() || self.modify_type == ModifyType::AddNew {
            match self.modify_type {
                ModifyType::Resize => {
                    if let Some(saved) = self.move_info.take() {
                        self.push_undo(UndoAction::MoveResize { saved, resize: true }, ch);
                    }
                    // Give the shape the opportunity to adjust the shape coordinates
                    self.data.display_items[i].shape.on_move_point();
                }
                ModifyType::Move => {
                    if let Some(saved) = self.move_info.take() {
                        self.push_undo(UndoAction::MoveResize { saved, resize: false }, ch);
                    }
                }
                ModifyType::AddNew => {
                    self.preview_item_add_action(ch); //starts Undo_Redo Action
                    if let Some(node) = self.selected_node {
                        self.highlight_node(node, ch);
                    } else {
                        self.deselect_selected_display_item(ch);
                    }
                }
                ModifyType::None => {}
            }
            self.modify_type = ModifyType::None;
            self.move_info = None;
        }
        self.touch(i, ch);
    }

    /// `ResetMouse`.
    fn reset_mouse(&mut self) {
        self.banding = false;
        self.mouse_captured = false;
        self.band_rect.width = 0;
        self.band_rect.height = 0;
    }

    /// `PreviewItemAddAction`: an undo action for the selected item, just added.
    fn preview_item_add_action(&mut self, ch: &mut Change) {
        if let Some(id) = self.selected_item {
            self.push_undo(UndoAction::Added { ids: vec![id], parked: Vec::new(), paste: false }, ch);
        }
    }

    // ----- keyboard

    /// `OnKeyDown`. Plus and Minus need Control, as do the letter keys.
    pub fn key_down(&mut self, key: Key, m: Modifiers) -> Change {
        let mut ch = Change::default();
        let only_ctrl = m.only_ctrl();
        match key {
            Key::Delete => return self.delete_selected(),
            Key::C if only_ctrl => {
                ch.clipboard = self.copy();
                ch.selection_changed = true;
            }
            Key::X if only_ctrl => return self.cut(),
            Key::V if only_ctrl => return self.paste(),
            Key::Z if only_ctrl => return self.undo(),
            Key::Y if only_ctrl => return self.redo(),
            Key::L if only_ctrl => return self.lock_selected(),
            Key::U if only_ctrl => return self.unlock_selected(false),
            Key::N if only_ctrl => return self.unlock_selected(true),
            Key::Up => self.nudge_selected(0, -1, &mut ch),
            Key::Down => self.nudge_selected(0, 1, &mut ch),
            Key::Right => self.nudge_selected(1, 0, &mut ch),
            Key::Left => self.nudge_selected(-1, 0, &mut ch),
            Key::Escape => self.key_escape(&mut ch),
            Key::Plus if only_ctrl && self.zoom < 4.0 => return self.set_zoom(self.zoom + 0.25),
            Key::Minus if only_ctrl && self.zoom > 0.25 => return self.set_zoom(self.zoom - 0.25),
            _ => {}
        }
        self.end_update(&mut ch);
        ch
    }

    fn nudge_selected(&mut self, dx: i32, dy: i32, ch: &mut Change) {
        if let Some(i) = self.selected_item.and_then(|id| self.index_of(id)) {
            self.data.display_items[i].shape.nudge(dx, dy);
            self.touch(i, ch);
        } else if !self.selected_items.is_empty() {
            // Over all items, not the selection, so bounds-equal twins of a selected shape nudge too. Deliberate.
            let keys = self.selected_keys();
            for i in 0..self.data.display_items.len() {
                let s = &self.data.display_items[i].shape;
                if keys.contains(&s.bounds_key()) && self.visible(s) {
                    self.data.display_items[i].shape.nudge(dx, dy);
                    self.touch(i, ch);
                }
            }
        }
    }

    fn key_escape(&mut self, ch: &mut Change) {
        let creating = self.selected_item.and_then(|id| self.index_of(id)).filter(|i| {
            let s = &self.data.display_items[*i].shape;
            s.creating && matches!(s.kind(), ShapeKind::PolyLine { .. } | ShapeKind::MultiString { .. })
        });
        if let Some(i) = creating {
            self.preview_item_add_action(ch);
            let id = self.data.display_items[i].id;
            if matches!(self.data.display_items[i].shape.kind(), ShapeKind::PolyLine { .. }) {
                // The C# offers an element template here too, but has none for this tool.
                self.data.display_items[i].shape.end_creation_poly_line();
            } else {
                self.data.display_items[i].shape.end_creation_multi_string();
            }
            self.on_select_display_item(id, ch);
            self.deselect_selected_display_item(ch);
            self.reset_mouse();
            self.touch(i, ch);
        } else {
            self.set_current_tool(Tool::Select, ch);
        }
    }

    // ----- clipboard

    /// `Cut`.
    pub fn cut(&mut self) -> Change {
        let mut ch = Change::default();
        self.remove_hidden_from_selection(&mut ch);
        ch.clipboard = self.copy();
        let del = self.delete_selected();
        self.absorb_into(&mut ch, del);
        ch
    }

    fn absorb_into(&mut self, into: &mut Change, c: Change) {
        self.absorb(into, |_| c);
    }

    /// `Copy`. The returned text is what the C# puts on the clipboard, an `ArrayOfDisplayItem`, so it pastes
    /// into Vixen too. It is also kept here for [`DesignSession::paste`].
    pub fn copy(&mut self) -> Option<String> {
        let mut scratch = Change::default();
        self.remove_hidden_from_selection(&mut scratch);
        if self.selected_items.is_empty() {
            if let Some(id) = self.selected_item {
                // The single item goes into the multi-selection and stays there, as in the C#.
                self.selected_items.push(id);
            }
        }
        let xml = if self.selected_items.is_empty() {
            None
        } else {
            let mut root = XElement::new(SHAPES_NS, "ArrayOfDisplayItem");
            for id in &self.selected_items {
                if let Some(item) = self.item(*id) {
                    root.push(item.to_xml());
                }
            }
            Some(root.to_fragment())
        };
        if let Some(x) = &xml {
            self.clipboard = Some(x.clone());
        }
        self.clipboard_populated = true;
        self.runs_dirty = true;
        self.end_update(&mut scratch);
        xml
    }

    /// `Paste`, from what [`DesignSession::copy`] last produced.
    pub fn paste(&mut self) -> Change {
        match self.clipboard.clone() {
            Some(xml) => self.paste_text(&xml),
            None => {
                let mut ch = Change::default();
                self.selected_items.clear();
                self.runs_dirty = true;
                self.end_update(&mut ch);
                ch
            }
        }
    }

    /// `Paste`, from text off the system clipboard. Text that is not display items pastes nothing, and
    /// empties the multi-selection, as in the C#.
    pub fn paste_text(&mut self, xml: &str) -> Change {
        let mut ch = Change::default();
        let shapes = read_display_item_list(xml);
        self.selected_items.clear();
        self.runs_dirty = true;
        if shapes.is_empty() {
            self.end_update(&mut ch);
            return ch;
        }
        self.deselect_selected_display_item(&mut ch);
        let zoom = self.zoom;
        let mut ids = Vec::with_capacity(shapes.len());
        for s in shapes {
            let id = self.add_display_item(s, &mut ch);
            let i = self.data.display_items.len() - 1;
            self.data.display_items[i].shape.set_zoom_level(zoom);
            ids.push(id);
        }
        self.push_undo(UndoAction::Added { ids: ids.clone(), parked: Vec::new(), paste: true }, &mut ch); //Start Undo Action.
        self.selected_items = ids.clone();

        // move the prop to the mouse position
        let mouse_point = (self.mouse_client.0.wrapping_add(self.h_scroll), self.mouse_client.1.wrapping_add(self.v_scroll));
        let move_to = self.point_to_zoom_point(mouse_point);
        let (mut top, mut left) = (i32::MAX, i32::MAX);
        for id in &ids {
            if let Some(item) = self.item(*id) {
                top = top.min(item.shape.top());
                left = left.min(item.shape.left());
            }
        }
        let delta_y = top.wrapping_sub(move_to.1);
        let delta_x = left.wrapping_sub(move_to.0);
        for id in &ids {
            if let Some(i) = self.index_of(*id) {
                let s = &mut self.data.display_items[i].shape;
                let l = s.left().wrapping_sub(delta_x);
                s.set_left(l);
                let t = s.top().wrapping_sub(delta_y);
                s.set_top(t);
                self.rebuild_item(i, &mut ch);
            }
        }
        self.start_move(mouse_point.0, mouse_point.1, true);
        ch.selection_changed = true;
        self.end_update(&mut ch);
        ch
    }

    /// `Delete`.
    pub fn delete_selected(&mut self) -> Change {
        let mut ch = Change::default();
        self.remove_hidden_from_selection(&mut ch);
        if !self.selected_items.is_empty() {
            let selected = std::mem::take(&mut self.selected_items);
            let mut parked = Vec::with_capacity(selected.len());
            for id in &selected {
                // DeSelectSelectedDisplayItem runs per item in the C#, and only the first does anything.
                self.deselect_selected_display_item(&mut ch);
                parked.extend(self.remove_display_item(*id, &mut ch));
            }
            self.push_undo(UndoAction::Removed { ids: selected, parked }, &mut ch); //Start Undo Action.
            ch.selection_changed = true;
        } else if let Some(id) = self.selected_item {
            self.deselect_selected_display_item(&mut ch);
            let parked: Vec<DisplayItem> = self.remove_display_item(id, &mut ch).into_iter().collect();
            self.push_undo(UndoAction::Removed { ids: vec![id], parked }, &mut ch); //Start Undo Action.
        }
        self.end_update(&mut ch);
        ch
    }

    // ----- locking

    fn lock_entries(&self, ids: &[ShapeId]) -> Vec<((i32, i32, i32, i32), bool)> {
        ids.iter().filter_map(|id| self.item(*id)).map(|i| (i.shape.bounds_key(), i.shape.locked)).collect()
    }

    /// `Lock`.
    pub fn lock_selected(&mut self) -> Change {
        let mut ch = Change::default();
        self.remove_hidden_from_selection(&mut ch);
        if !self.selected_items.is_empty() {
            let items = self.lock_entries(&self.selected_items);
            self.push_undo(UndoAction::Lock { items }, &mut ch); //Start Undo Action.
            for n in 0..self.selected_items.len() {
                if let Some(i) = self.index_of(self.selected_items[n]) {
                    self.data.display_items[i].shape.locked = true;
                }
                self.deselect_selected_display_item(&mut ch);
            }
            self.selected_items.clear();
            ch.selection_changed = true;
        } else if let Some(id) = self.selected_item {
            let items = self.lock_entries(&[id]);
            self.push_undo(UndoAction::Lock { items }, &mut ch); //Start Undo Action.
            if let Some(i) = self.index_of(id) {
                self.data.display_items[i].shape.locked = true;
            }
            self.deselect_selected_display_item(&mut ch);
        } else {
            self.end_update(&mut ch);
            return ch;
        }
        // Display "How to Unlock" just once
        if !self.lock_warning_shown {
            self.lock_warning_shown = true;
            ch.lock_hint = true;
        }
        self.runs_dirty = true;
        self.end_update(&mut ch);
        ch
    }

    /// `Unlock`.
    pub fn unlock_selected(&mut self, all: bool) -> Change {
        let mut ch = Change::default();
        self.remove_hidden_from_selection(&mut ch);
        if all {
            let items = self.lock_entries(&self.selected_items);
            self.push_undo(UndoAction::Lock { items }, &mut ch); //Start Undo Action.
            for item in &mut self.data.display_items {
                item.shape.locked = false;
            }
            ch.selection_changed = true;
        } else if !self.selected_items.is_empty() {
            let items = self.lock_entries(&self.selected_items);
            self.push_undo(UndoAction::Lock { items }, &mut ch); //Start Undo Action.
            for n in 0..self.selected_items.len() {
                if let Some(i) = self.index_of(self.selected_items[n]) {
                    self.data.display_items[i].shape.locked = false;
                }
                self.deselect_selected_display_item(&mut ch);
            }
            self.selected_items.clear();
            ch.selection_changed = true;
        } else if let Some(id) = self.selected_item {
            let items = self.lock_entries(&[id]);
            self.push_undo(UndoAction::Lock { items }, &mut ch); //Start Undo Action.
            if let Some(i) = self.index_of(id) {
                self.data.display_items[i].shape.locked = false;
            }
            self.deselect_selected_display_item(&mut ch);
        }
        self.runs_dirty = true;
        self.end_update(&mut ch);
        ch
    }

    // ----- undo

    fn push_undo(&mut self, action: UndoAction, ch: &mut Change) {
        self.undoable.push_front(action);
        if self.undoable.len() > UNDO_MAX_ITEMS {
            self.undoable.pop_back();
        }
        ch.undo_changed = true;
    }

    /// `UndoManager.Undo`. Nothing to undo does nothing. The redo stack survives new actions, as in the C#.
    pub fn undo(&mut self) -> Change {
        let mut ch = Change::default();
        if let Some(mut action) = self.undoable.pop_front() {
            self.apply_undo(&mut action, true, &mut ch);
            self.redoable.push(action);
            ch.undo_changed = true;
        }
        self.end_update(&mut ch);
        ch
    }

    /// `UndoManager.Redo`.
    pub fn redo(&mut self) -> Change {
        let mut ch = Change::default();
        if let Some(mut action) = self.redoable.pop() {
            self.apply_undo(&mut action, false, &mut ch);
            self.push_undo(action, &mut ch);
        }
        self.end_update(&mut ch);
        ch
    }

    fn apply_undo(&mut self, action: &mut UndoAction, undo: bool, ch: &mut Change) {
        match action {
            UndoAction::Added { ids, parked, .. } => {
                if undo {
                    self.park_items(ids, parked, ch);
                } else {
                    self.unpark_items(ids, parked, ch);
                }
            }
            UndoAction::Removed { ids, parked } => {
                if undo {
                    self.unpark_items(ids, parked, ch);
                } else {
                    self.park_items(ids, parked, ch);
                }
            }
            UndoAction::MoveResize { saved, .. } => self.swap_places(saved, ch),
            UndoAction::Studio(action) => self.apply_studio_undo(action, ch), // STUDIO: designer-live-undo
            UndoAction::Lock { items } => {
                // Undo_Lock. Find goes by the bounds in the clone, and finding nothing throws in C#.
                for (key, locked) in items.iter() {
                    if let Some(item) = self.data.display_items.iter_mut().find(|i| i.shape.bounds_key() == *key) {
                        item.shape.locked = if undo { *locked } else { !*locked };
                    }
                }
                self.runs_dirty = true;
                ch.overlay_changed = true;
            }
            UndoAction::PixelSize { ids, change } => {
                // PixelResizeSwapPlaces runs for undo and redo alike and flips the change each time.
                let reverse = -*change;
                let list = ids.clone().unwrap_or_else(|| self.selected_items.clone());
                for id in list {
                    if let Some(i) = self.index_of(id) {
                        if self.data.display_items[i].shape.is_light_shape() {
                            self.data.display_items[i].shape.resize_pixels_by(reverse);
                            *change = reverse;
                            self.touch(i, ch);
                        }
                    }
                }
            }
            UndoAction::GroupAdded { group, children } => {
                if undo {
                    // removeEffects
                    self.selected_items.clear();
                    let new_children = self.separate_template_items(*group, ch);
                    if !new_children.is_empty() {
                        *children = new_children;
                    }
                } else {
                    // addGroupEffects
                    self.deselect_selected_display_item(ch);
                    self.selected_items = children.clone();
                    if let Some(new_group) = self.add_new_group(ch) {
                        *group = new_group;
                    }
                    self.selected_items.clear();
                }
                ch.selection_changed = true;
            }
        }
    }

    /// `removeEffects`.
    fn park_items(&mut self, ids: &[ShapeId], parked: &mut Vec<DisplayItem>, ch: &mut Change) {
        for id in ids.iter() {
            parked.extend(self.remove_display_item(*id, ch));
        }
    }

    /// `addEffects`.
    fn unpark_items(&mut self, ids: &mut Vec<ShapeId>, parked: &mut Vec<DisplayItem>, ch: &mut Change) {
        ids.clear();
        for item in parked.drain(..) {
            ids.push(item.id);
            self.restore_display_item(item, ch);
        }
    }

    /// `Resize_MoveSwapPlaces`: the shape's data goes back and forth with the saved copy. A restored shape is
    /// as one deserialized, so a selected item loses its handles until it is selected again, as in the C#.
    fn swap_places(&mut self, saved: &mut [(ShapeId, Shape)], ch: &mut Change) {
        let zoom = self.zoom;
        for (id, shape) in saved.iter_mut() {
            let Some(i) = self.index_of(*id) else { continue };
            std::mem::swap(&mut self.data.display_items[i].shape, shape);
            let s = &mut self.data.display_items[i].shape;
            s.roundtrip_state();
            s.set_zoom_level(zoom);
            self.rebuild_item(i, ch);
        }
    }

    // ----- zoom and scroll

    /// `ZoomLevel` setter: clamped to 25% to 400%, and every shape lays out again.
    pub fn set_zoom(&mut self, zoom: f64) -> Change {
        let mut ch = Change::default();
        self.zoom = zoom.clamp(0.25, 4.0);
        for item in &mut self.data.display_items {
            item.shape.set_zoom_level(self.zoom);
            item.invalidate_hit();
        }
        self.scene.set_zoom(self.zoom);
        if self.scene.refresh_geometry(&self.data) {
            ch.scene.push(self.scene.delta_all());
        } else {
            // A net's light count can change with its layout.
            self.rebuild_scene(&mut ch);
        }
        ch.zoom_changed = Some(self.zoom);
        ch.overlay_changed = true;
        self.update_scroll_range();
        let (to, mouse) = (self.zoom_to, self.mouse_client);
        self.set_background_position(to, mouse.0, mouse.1, &mut ch);
        self.end_update(&mut ch);
        ch
    }

    /// `VixenPreviewControl_MouseWheel`.
    pub fn mouse_wheel(&mut self, delta: i32, x: i32, y: i32) -> Change {
        self.mouse_client = (x, y);
        // Zoom to the pointer location
        self.zoom_to = self.mouse_point_to_zoom_point(x, y);
        self.set_zoom(self.zoom + delta as f64 / 1000.0)
    }

    /// `PointToZoomPoint`.
    fn point_to_zoom_point(&self, p: (i32, i32)) -> (i32, i32) {
        (to_int32(p.0 as f64 / self.zoom), to_int32(p.1 as f64 / self.zoom))
    }

    /// `MousePointToZoomPoint`.
    fn mouse_point_to_zoom_point(&self, x: i32, y: i32) -> (i32, i32) {
        self.point_to_zoom_point((x.wrapping_add(self.h_scroll), y.wrapping_add(self.v_scroll)))
    }

    /// `SetBackgroundPosition`: scrolls so the zoom point stays under the mouse.
    fn set_background_position(&mut self, zoom_to: (i32, i32), mouse_x: i32, mouse_y: i32, ch: &mut Change) {
        let bx = to_int32(zoom_to.0 as f64 * self.zoom);
        let by = to_int32(zoom_to.1 as f64 * self.zoom);
        let new_h = bx.wrapping_sub(mouse_x);
        if new_h > 0 && new_h <= self.h_scroll_max && new_h != self.h_scroll {
            self.h_scroll = new_h;
            ch.scroll_changed = true;
        }
        let new_v = by.wrapping_sub(mouse_y);
        if new_v > 0 && new_v <= self.v_scroll_max && new_v != self.v_scroll {
            self.v_scroll = new_v;
            ch.scroll_changed = true;
        }
    }

    /// The view's size in pixels, which with the background decides how far it can scroll.
    pub fn set_view_size(&mut self, width: i32, height: i32) {
        self.view_size = (width, height);
        self.update_scroll_range();
    }

    /// The size of the background image, or of the blank background.
    pub fn set_background_size(&mut self, width: i32, height: i32) {
        self.background_size = Some((width, height));
        self.update_scroll_range();
    }

    pub fn background_size(&self) -> Option<(i32, i32)> {
        self.background_size
    }

    /// `SetupScrollBars`.
    fn update_scroll_range(&mut self) {
        let Some((bw, bh)) = self.background_size else { return };
        let scaled_w = to_int32(bw as f64 * self.zoom);
        let scaled_h = to_int32(bh as f64 * self.zoom);
        self.h_scroll_max = (scaled_w - self.view_size.0).max(0);
        self.v_scroll_max = (scaled_h - self.view_size.1).max(0);
        self.h_scroll = self.h_scroll.clamp(0, self.h_scroll_max);
        self.v_scroll = self.v_scroll.clamp(0, self.v_scroll_max);
    }

    /// The scroll bars moved.
    pub fn set_scroll(&mut self, h: i32, v: i32) {
        self.h_scroll = h;
        self.v_scroll = v;
    }

    /// `keepInsertModeToolStripMenuItem_Click`.
    pub fn set_keep_insert_mode(&mut self, on: bool) {
        self.data.keep_insert_mode = on;
    }

    /// `saveLocationsToolStripMenuItem_Click`.
    pub fn set_save_locations(&mut self, on: bool) {
        self.data.save_locations = on;
    }

    /// `useOpenGLPreviewToolStripMenuItem_Click`. Every Mac this runs on has the GPU preview, so there is nothing
    /// to check first.
    pub fn set_use_open_gl(&mut self, on: bool) {
        self.data.use_open_gl = on;
    }

    /// `LocationOffsetForm`, on OK.
    pub fn set_location_offset(&mut self, x: f64, y: f64, z: f64) {
        self.data.location_offset = Vector3D { x: DoubleText::new(x), y: DoubleText::new(y), z: DoubleText::new(z) };
    }

    /// `buttonSetBackground_Click` once the image is in the module's data folder, or None if it couldn't be loaded,
    /// as `LoadBackground` does. Tell the session its size with [`set_background_size`](Self::set_background_size).
    pub fn set_background_file(&mut self, file_name: Option<String>) {
        self.data.background_file_name = file_name;
    }

    /// `VixenPreviewSetup3_Move` and `_Resize`: where the setup window is, to restore it next time.
    pub fn set_setup_bounds(&mut self, left: i32, top: i32, width: i32, height: i32) {
        self.data.setup_left = left;
        self.data.setup_top = top;
        self.data.setup_width = width;
        self.data.setup_height = height;
    }

    /// Where the preview window is, to restore it next time.
    pub fn set_display_bounds(&mut self, left: i32, top: i32, width: i32, height: i32) {
        self.data.left = left;
        self.data.top = top;
        self.data.width = width;
        self.data.height = height;
    }

    /// `trackBarBackgroundAlpha_ValueChanged`.
    pub fn set_background_alpha(&mut self, alpha: i32) {
        for item in &mut self.data.display_items {
            item.shape.background_alpha = alpha;
        }
        self.data.background_alpha = alpha;
    }

    /// `ResizeBlankBackground` or `ResizeImageBackground`, without the image work, which is the host's: the
    /// aspect of the new width and, optionally, every shape scaled by it.
    pub fn resize_background(&mut self, width: i32, height: i32, scale_shapes: bool) -> Change {
        let mut ch = Change::default();
        let old_width = self.background_size.map_or(width, |b| b.0);
        let aspect = width as f64 / old_width as f64;
        self.background_size = Some((width, height));
        if scale_shapes {
            for i in 0..self.data.display_items.len() {
                self.data.display_items[i].shape.resize(aspect);
                self.touch(i, &mut ch);
            }
        }
        self.update_scroll_range();
        self.end_update(&mut ch);
        ch
    }

    // ----- alignment

    /// `SelectedShapes()[0]`, as a copy so the others can be changed while it is read.
    fn first_selected_shape(&self) -> Option<Shape> {
        self.selected_items.first().and_then(|id| self.item(*id)).map(|i| i.shape.clone())
    }

    /// `toolbarAlignButton_Click`: the operation between `beginResize_Move(true)` and the undo action.
    fn with_alignment(&mut self, f: impl FnOnce(&mut DesignSession, &mut Change)) -> Change {
        let mut ch = Change::default();
        self.modify_type = ModifyType::Move;
        self.begin_resize_move(true); //Starts the Undo Process
        f(self, &mut ch);
        if let Some(saved) = self.move_info.take() {
            self.push_undo(UndoAction::MoveResize { saved, resize: false }, &mut ch);
        }
        self.end_update(&mut ch);
        ch
    }

    /// Runs `f` on each selected shape but the first, with the first shape's bounds.
    fn align_others(&mut self, ch: &mut Change, f: impl Fn(&mut Shape, &Shape)) {
        let Some(first) = self.first_selected_shape() else { return };
        for n in 1..self.selected_items.len() {
            if let Some(i) = self.index_of(self.selected_items[n]) {
                f(&mut self.data.display_items[i].shape, &first);
                self.touch(i, ch);
            }
        }
    }

    /// `AlignLeft`.
    pub fn align_left(&mut self) -> Change {
        self.with_alignment(|s, ch| s.align_others(ch, |shape, first| shape.set_left(first.left())))
    }

    /// `AlignRight`.
    pub fn align_right(&mut self) -> Change {
        self.with_alignment(|s, ch| {
            s.align_others(ch, |shape, first| {
                let v = first.right().wrapping_sub(shape.right().wrapping_sub(shape.left()));
                shape.set_left(v);
            })
        })
    }

    /// `AlignTop`.
    pub fn align_top(&mut self) -> Change {
        self.with_alignment(|s, ch| s.align_others(ch, |shape, first| shape.set_top(first.top())))
    }

    /// `AlignBottom`.
    pub fn align_bottom(&mut self) -> Change {
        self.with_alignment(|s, ch| {
            s.align_others(ch, |shape, first| {
                let v = first.bottom().wrapping_sub(shape.bottom().wrapping_sub(shape.top()));
                shape.set_top(v);
            })
        })
    }

    /// `AlignHorizontal`: the middles line up, moving vertically.
    pub fn align_horizontal(&mut self) -> Change {
        self.with_alignment(|s, ch| {
            s.align_others(ch, |shape, first| {
                let mid = first.top().wrapping_add(first.bottom().wrapping_sub(first.top()) / 2);
                let v = mid.wrapping_sub(shape.bottom().wrapping_sub(shape.top()) / 2);
                shape.set_top(v);
            })
        })
    }

    /// `AlignVertical`: the middles line up, moving horizontally.
    pub fn align_vertical(&mut self) -> Change {
        self.with_alignment(|s, ch| {
            s.align_others(ch, |shape, first| {
                let mid = first.left().wrapping_add(first.right().wrapping_sub(first.left()) / 2);
                let v = mid.wrapping_sub(shape.right().wrapping_sub(shape.left()) / 2);
                shape.set_left(v);
            })
        })
    }

    /// `DistributeHorizontal`.
    pub fn distribute_horizontal(&mut self) -> Change {
        self.with_alignment(|s, ch| s.distribute(ch, true))
    }

    /// `DistributeVertical`.
    pub fn distribute_vertical(&mut self) -> Change {
        self.with_alignment(|s, ch| s.distribute(ch, false))
    }

    fn distribute(&mut self, ch: &mut Change, horizontal: bool) {
        // SelectedShapes().OrderBy(o => o.Left), a stable sort.
        let mut order: Vec<(i32, usize)> = self
            .selected_items
            .iter()
            .filter_map(|id| self.index_of(*id))
            .map(|i| {
                let s = &self.data.display_items[i].shape;
                (if horizontal { s.left() } else { s.top() }, i)
            })
            .collect();
        order.sort_by_key(|o| o.0);
        let shape_count = order.len();
        if shape_count < 3 {
            return;
        }
        let near = |s: &Shape| if horizontal { s.left() } else { s.top() };
        let far = |s: &Shape| if horizontal { s.right() } else { s.bottom() };
        let items = &self.data.display_items;
        let first_far = far(&items[order[0].1].shape);
        let total_space = near(&items[order[shape_count - 1].1].shape).wrapping_sub(first_far);
        let mut space_to_fill = total_space;
        for o in &order[1..shape_count - 1] {
            let s = &items[o.1].shape;
            space_to_fill = space_to_fill.wrapping_sub(far(s).wrapping_sub(near(s)));
        }
        if space_to_fill <= 0 {
            return;
        }
        let shape_spacing = space_to_fill as f32 / (shape_count - 1) as f32;
        let limit = self.background_size.map_or(i32::MAX, |b| if horizontal { b.0 } else { b.1 });
        let mut prop_space_so_far = 0i32;
        for (n, o) in order[1..shape_count - 1].iter().enumerate() {
            let shape_num = n + 1;
            let offset = to_int32((shape_spacing * shape_num as f32) as f64);
            let new_pos = first_far.wrapping_add(prop_space_so_far).wrapping_add(offset);
            //only move to valid coordinates.
            if new_pos >= 0 && new_pos < limit {
                let s = &mut self.data.display_items[o.1].shape;
                if horizontal {
                    s.set_left(new_pos);
                } else {
                    s.set_top(new_pos);
                }
                self.touch(o.1, ch);
            }
            let s = &self.data.display_items[o.1].shape;
            prop_space_so_far = prop_space_so_far.wrapping_add(far(s).wrapping_sub(near(s)));
        }
    }

    /// `MatchProperties`. Shapes of different types get the C#'s message and no change.
    pub fn match_properties(&mut self) -> Change {
        self.with_alignment(|s, ch| {
            if s.selected_items.len() < 2 {
                return;
            }
            let Some(first) = s.first_selected_shape() else { return };
            let same = s.selected_items.iter().filter_map(|id| s.item(*id)).all(|i| std::mem::discriminant(i.shape.kind()) == std::mem::discriminant(first.kind()));
            if !same {
                ch.message = Some("You can only match the properties of like shapes.".to_string());
                return;
            }
            for n in 1..s.selected_items.len() {
                if let Some(i) = s.index_of(s.selected_items[n]) {
                    s.data.display_items[i].shape.match_shape(&first);
                    s.rebuild_item(i, ch);
                }
            }
        })
    }

    // ----- bulb size

    /// `IncreaseBulbSize`.
    pub fn increase_bulb_size(&mut self) -> Change {
        let mut ch = Change::default();
        let single = self.selected_item.and_then(|id| self.index_of(id)).filter(|i| self.data.display_items[*i].shape.is_light_shape());
        let any_multi_light = self.selected_items.iter().filter_map(|id| self.item(*id)).any(|i| i.shape.is_light_shape());
        if let (Some(i), false) = (single, any_multi_light) {
            self.data.display_items[i].shape.resize_pixels_by(1);
            self.touch(i, &mut ch);
            let id = self.data.display_items[i].id;
            self.push_undo(UndoAction::PixelSize { ids: Some(vec![id]), change: 1 }, &mut ch);
        } else {
            self.resize_selected_bulbs(1, &mut ch);
        }
        self.end_update(&mut ch);
        ch
    }

    /// `DecreaseBulbSize`. The single shape's undo records +1 while the size went down, so undoing makes it
    /// smaller still. Deliberate, as in the C#.
    pub fn decrease_bulb_size(&mut self) -> Change {
        let mut ch = Change::default();
        let single = self.selected_item.and_then(|id| self.index_of(id)).filter(|i| self.data.display_items[*i].shape.is_light_shape());
        if let (Some(i), true) = (single, self.selected_items.is_empty()) {
            self.data.display_items[i].shape.resize_pixels_by(-1);
            self.touch(i, &mut ch);
            let id = self.data.display_items[i].id;
            self.push_undo(UndoAction::PixelSize { ids: Some(vec![id]), change: 1 }, &mut ch);
        } else {
            self.resize_selected_bulbs(-1, &mut ch);
        }
        self.end_update(&mut ch);
        ch
    }

    fn resize_selected_bulbs(&mut self, by: i32, ch: &mut Change) {
        for n in 0..self.selected_items.len() {
            if let Some(i) = self.index_of(self.selected_items[n]) {
                if self.data.display_items[i].shape.is_light_shape() {
                    self.data.display_items[i].shape.resize_pixels_by(by);
                    self.touch(i, ch);
                }
            }
        }
        self.push_undo(UndoAction::PixelSize { ids: None, change: by }, ch);
    }

    /// `MatchBulbSize`.
    pub fn match_bulb_size(&mut self) -> Change {
        let mut ch = Change::default();
        if let Some(first) = self.first_selected_shape().filter(Shape::is_light_shape) {
            for n in 1..self.selected_items.len() {
                if let Some(i) = self.index_of(self.selected_items[n]) {
                    if self.data.display_items[i].shape.is_light_shape() {
                        self.data.display_items[i].shape.match_pixel_size(&first);
                        self.touch(i, &mut ch);
                    }
                }
            }
        }
        self.end_update(&mut ch);
        ch
    }

    // ----- groups and templates

    /// `CreateGroup`, plus what the Group menu item does with the result: the group becomes the selected item.
    pub fn group_selected(&mut self) -> Change {
        let mut ch = Change::default();
        if let Some(id) = self.create_group(&mut ch) {
            self.selected_item = Some(id);
            self.select_shape(id, &mut ch);
        }
        self.end_update(&mut ch);
        ch
    }

    fn create_group(&mut self, ch: &mut Change) -> Option<ShapeId> {
        if self.selected_items.is_empty() {
            return None;
        }
        for id in &self.selected_items {
            match self.item(*id).map(|i| i.shape.kind()) {
                Some(ShapeKind::Custom { .. }) => {
                    ch.message = Some("You cannot create a group or a template with an item that is already grouped or a template item. First, separate the items and then re-group all the items you would like.".to_string());
                    return None;
                }
                Some(ShapeKind::CustomProp(_)) => {
                    ch.message = Some("Grouping or creating a template with a Custom Prop is not suppported at this time.".to_string());
                    return None;
                }
                _ => {}
            }
        }
        let children_before = self.selected_items.clone();
        let group = self.add_new_group(ch)?;
        self.selected_items.clear();
        ch.selection_changed = true;
        self.push_undo(UndoAction::GroupAdded { group, children: children_before }, ch); //Start Undo Action.
        Some(group)
    }

    /// `AddNewGroup`. A selected shape that is not a light shape (a fixture) leaves the preview without joining
    /// the group. Deliberate, as in the C#. The shapes are taken out by id, not by bounds, because they
    /// move into the group here where the C# has them in both places.
    fn add_new_group(&mut self, ch: &mut Change) -> Option<ShapeId> {
        //Sets the start TopLeft point of the new group
        let (mut top, mut left) = (i32::MAX, i32::MAX);
        for id in &self.selected_items {
            if let Some(item) = self.item(*id) {
                top = top.min(item.shape.top());
                left = left.min(item.shape.left());
            }
        }
        let ids = std::mem::take(&mut self.selected_items);
        let mut shapes = Vec::with_capacity(ids.len());
        for id in &ids {
            if let Some(item) = self.remove_exact(*id, ch) {
                if item.shape.is_light_shape() {
                    shapes.push(item.shape);
                }
            }
        }
        if shapes.is_empty() {
            return None;
        }
        Some(self.add_display_item(Shape::new_custom(PreviewPoint::new(left, top), shapes), ch))
    }

    /// `SeparateTemplateItems`, the Ungroup menu item: the selected group's shapes become display items again.
    pub fn ungroup_selected(&mut self) -> Change {
        let mut ch = Change::default();
        if let Some(id) = self.selected_item {
            self.separate_template_items(id, &mut ch);
        }
        self.end_update(&mut ch);
        ch
    }

    /// The new items' ids.
    fn separate_template_items(&mut self, id: ShapeId, ch: &mut Change) -> Vec<ShapeId> {
        let was_selected = self.selected_item == Some(id);
        if was_selected {
            self.deselect_selected_display_item(ch);
        }
        let Some(mut item) = self.remove_exact(id, ch) else { return Vec::new() };
        let Some(strings) = item.shape.strings.take() else {
            self.restore_display_item(item, ch);
            return Vec::new();
        };
        self.selected_items.clear();
        ch.selection_changed = true;
        strings.into_iter().map(|s| self.add_display_item(s, ch)).collect()
    }

    /// `CreateTemplate`, with the name the dialog asks for: the group, and the file's text for the Templates
    /// folder. See [`crate::template::TemplateStore`].
    pub fn create_template(&mut self, name: &str) -> (Change, Option<String>) {
        let mut ch = Change::default();
        let mut xml = None;
        if let Some(id) = self.create_group(&mut ch) {
            if let Some(i) = self.index_of(id) {
                self.data.display_items[i].shape.name = Some(name.to_string());
                self.select_shape(id, &mut ch);
                self.selected_item = Some(id);
                xml = Some(self.data.display_items[i].to_xml_document());
            }
        }
        self.end_update(&mut ch);
        (ch, xml)
    }

    /// `AddTtemplateToPreview`, given the template file's text.
    pub fn add_template(&mut self, xml: &str) -> Change {
        let mut ch = Change::default();
        let Some(mut shape) = read_template(xml) else { return ch };
        // Remove any node associations from the template that may have been saved in old versions
        shape.for_each_pixel_mut(&mut |p| p.node_id = Uuid::nil());
        self.deselect_selected_display_item(&mut ch);
        if let (Some(node), Some(tree)) = (self.selected_node, self.tree.clone()) {
            associate_singles_by_name(&mut shape, node, &tree);
        }
        let id = self.add_display_item(shape, &mut ch);
        let i = self.data.display_items.len() - 1;
        self.selected_item = Some(id);
        self.on_select_display_item(id, &mut ch);
        let s = &mut self.data.display_items[i].shape;
        s.move_to(10, 10);
        s.select(true);
        s.set_select_point(SelectArg::Null);
        self.preview_item_add_action(&mut ch); //starts Undo_Redo Action
        self.rebuild_item(i, &mut ch);
        ch.selection_changed = true;
        self.end_update(&mut ch);
        ch
    }

    /// `AddPropToPreviewAsync`: a Custom Prop built by [`crate::custom_prop_builder`], at a sensible size.
    pub fn add_custom_prop(&mut self, mut prop: Shape, x: i32, y: i32) -> Change {
        let mut ch = Change::default();
        //Ensure a reasonable size.
        if let Some(b) = prop.custom_prop_bounds() {
            if b.height > 100.0 {
                prop.resize(100.0 / b.height);
            } else if b.width > 100.0 {
                prop.resize(100.0 / b.width);
            }
        }
        prop.move_to(x, y);
        self.add_display_item(prop, &mut ch);
        self.selected_items.clear();
        if let Some(node) = self.selected_node {
            self.highlight_node(node, &mut ch);
        }
        ch.selection_changed = true;
        self.end_update(&mut ch);
        ch
    }

    // ----- Z and element locations

    /// `SetZForSelectedItems`.
    pub fn set_z(&mut self, pos: i32) -> Change {
        let mut ch = Change::default();
        let ids: Vec<ShapeId> = if !self.selected_items.is_empty() { self.selected_items.clone() } else { self.selected_item.into_iter().collect() };
        for id in &ids {
            if let Some(i) = self.index_of(*id) {
                if self.data.display_items[i].shape.is_light_shape() {
                    self.data.display_items[i].shape.for_each_pixel_mut(&mut |p| p.z = pos);
                }
            }
        }
        ch.location_updates = self.save_locations_for(&ids, true);
        ch.overlay_changed = true;
        ch
    }

    /// `SaveLocations` when `quirk_y_is_z`, and `SaveLocationDataForElements` when not.
    fn save_locations_for(&self, ids: &[ShapeId], quirk_y_is_z: bool) -> Vec<LocationUpdate> {
        let mut out = Vec::new();
        let off_x = self.data.location_offset.x.value;
        let off_y = self.data.location_offset.y.value;
        for id in ids {
            let Some(item) = self.item(*id) else { continue };
            if !item.shape.is_light_shape() {
                continue;
            }
            let common = item.shape.use_common_location().then(|| item.shape.center());
            item.shape.for_each_pixel(&mut |p| {
                if p.node_id.is_nil() {
                    return;
                }
                let (x, mut y) = if let Some(c) = common {
                    (c.x.wrapping_add(to_int32(off_x)), c.y.wrapping_add(to_int32(off_y)))
                } else if p.high_precision {
                    if quirk_y_is_z {
                        ((p.location.0 as i32).wrapping_add(to_int32(off_x)), (p.location.1 as i32).wrapping_add(to_int32(off_y)))
                    } else {
                        ((p.location.0 + off_x) as i32, (p.location.1 + off_y) as i32)
                    }
                } else {
                    (p.x.wrapping_add(to_int32(off_x)), p.y.wrapping_add(to_int32(off_y)))
                };
                if quirk_y_is_z {
                    // `SaveLocations` ends by writing the pixel's Z into Y. Deliberate, as in the C#.
                    y = p.z;
                }
                out.push(LocationUpdate { node: p.node_id, x, y });
            });
        }
        out
    }

    /// `SaveLocationDataForElements`, on save or when the setup closes: nothing unless Save Locations is on.
    pub fn location_updates(&self) -> Vec<LocationUpdate> {
        if !self.data.save_locations {
            return Vec::new();
        }
        let ids: Vec<ShapeId> = self.data.display_items.iter().map(|i| i.id).collect();
        self.save_locations_for(&ids, false)
    }

    // ----- opening

    /// `VixenPreviewSetup3_Load`: drop dead links, remove shapes added by mistake, bring strays back into view.
    pub fn setup_open(&mut self) -> (Change, VerifyReport) {
        let mut ch = self.reload();
        let mut report = VerifyReport::default();
        self.verify_preview_shape_locations(&mut report, &mut ch);
        // If the background is blank, ensure it is large enough to contain all the shapes
        if self.data.background_file_name.as_deref().unwrap_or("").is_empty() {
            self.verify_preview_background_size(&mut ch);
        }
        self.end_update(&mut ch);
        (ch, report)
    }

    /// `VerifyPreviewShapeLocations`.
    fn verify_preview_shape_locations(&mut self, report: &mut VerifyReport, ch: &mut Change) {
        let mut new_origin_x = 10;
        let mut to_remove = Vec::new();
        for i in 0..self.data.display_items.len() {
            let id = self.data.display_items[i].id;
            let s = &self.data.display_items[i].shape;
            if matches!(s.kind(), ShapeKind::Unknown(_)) {
                continue;
            }
            let (l, t, r, b) = (s.left(), s.top(), s.right(), s.bottom());
            //if items don't have any size then they were probably added by mistake.
            if t == b && l == r && t == l {
                to_remove.push(id);
                continue;
            }
            if matches!(s.kind(), ShapeKind::MultiString { .. }) && s.strings().is_none_or(<[Shape]>::is_empty) {
                to_remove.push(id);
                continue;
            }
            if b < 1 || (r < 1 && t < b && l < r) {
                let width = r.wrapping_sub(l);
                self.data.display_items[i].shape.move_to(new_origin_x, 10);
                self.touch(i, ch);
                new_origin_x = new_origin_x.wrapping_add(width).wrapping_add(5);
                let limit = self.background_size.map_or(i32::MAX, |b| b.0);
                if new_origin_x > limit || new_origin_x <= 0 {
                    new_origin_x = 10;
                }
                report.moved.push(id);
            }
        }
        if !report.moved.is_empty() {
            report.message = Some(format!("{} Preview Props were found outside of the preview bounds and have been moved back to the top left.", report.moved.len()));
        }
        for id in to_remove {
            if self.remove_display_item(id, ch).is_some() {
                report.removed.push(id);
            }
        }
    }

    /// `VerifyPreviewBackgroundSize`: a blank background grows to fit the shapes, and a shape past 8K is moved
    /// home as a rogue value.
    fn verify_preview_background_size(&mut self, ch: &mut Change) {
        let (mut max_x, mut max_y) = (0, 0);
        for i in 0..self.data.display_items.len() {
            let s = &self.data.display_items[i].shape;
            if s.right() > max_x {
                if s.right() > 7680 {
                    let s = &mut self.data.display_items[i].shape;
                    s.set_left(5);
                    s.set_top(5);
                    self.touch(i, ch);
                    continue;
                }
                max_x = s.right();
            }
            let s = &self.data.display_items[i].shape;
            if s.bottom() > max_y {
                if s.bottom() > 4320 {
                    let s = &mut self.data.display_items[i].shape;
                    s.set_left(5);
                    s.set_top(5);
                    self.touch(i, ch);
                    continue;
                }
                max_y = s.bottom();
            }
        }
        match self.background_size {
            Some((w, h)) if w >= max_x && h >= max_y => {}
            Some(_) | None => {
                self.background_size = Some((max_x + 1, max_y + 1));
                self.update_scroll_range();
            }
        }
    }

    // ----- properties and links

    /// The property grid's rows for a shape.
    pub fn shape_properties(&self, id: ShapeId) -> Vec<ShapeProperty> {
        self.item(id).map_or_else(Vec::new, |i| i.shape.properties())
    }

    /// A property grid edit, by display name or C# property name. Changes the geometry as the C# setter does.
    /// A warning from the fixture's validation comes back in `message`.
    pub fn set_shape_property(&mut self, id: ShapeId, name: &str, value: PropertyValue) -> Result<Change, PropertyError> {
        let i = self.index_of(id).ok_or(PropertyError::UnknownShape)?;
        let draw_shape = self.draw_shape.clone();
        let mut ch = Change::default();
        let warning = self.data.display_items[i].shape.set_property(name, value, &draw_shape)?;
        ch.message = warning.map(str::to_string);
        // A property can change links and light counts, so this is the full path.
        self.rebuild_item(i, &mut ch);
        self.end_update(&mut ch);
        Ok(ch)
    }

    /// Runs `f` on a shape and brings everything up to date afterwards. For the element linker.
    pub fn edit_shape(&mut self, id: ShapeId, f: impl FnOnce(&mut Shape)) -> Change {
        let mut ch = Change::default();
        if let Some(i) = self.index_of(id) {
            f(&mut self.data.display_items[i].shape);
            self.rebuild_item(i, &mut ch);
        }
        self.end_update(&mut ch);
        ch
    }

    /// Links a shape's strings to element nodes: [`link_elements`](crate::link::link_elements), with the links
    /// and the colour slots brought up to date. Does nothing without an element tree. The Linked Elements dialog
    /// is the longer way round, with an [`ElementLinker`](crate::link::ElementLinker) and [`edit_shape`](Self::edit_shape).
    pub fn link_elements(&mut self, id: ShapeId, nodes: &[NodeId]) -> Change {
        let Some(tree) = self.tree.clone() else { return Change::default() };
        self.edit_shape(id, |shape| crate::link::link_elements(shape, nodes, &tree))
    }

    /// Whether the mouse is captured, for hosts and tests.
    pub fn mouse_captured(&self) -> bool {
        self.mouse_captured
    }
}

// ----- what a window host needs

impl DesignSession {
    /// The toolbar and menu's Unlock All, `unlockAllButton_ButtonClick`: every item unlocked, with no undo entry. Control+N's
    /// path is [`unlock_selected(true)`](Self::unlock_selected), which pushes a useless `Lock 0 props`. The Mac has one
    /// command for both, and takes this one.
    pub fn unlock_all_without_undo(&mut self) -> Change {
        let mut ch = Change::default();
        for item in &mut self.data.display_items {
            item.shape.locked = false;
        }
        ch.selection_changed = true;
        self.runs_dirty = true;
        self.end_update(&mut ch);
        ch
    }

    /// `UndoManager.Undo(n)`, from the split button's list, with one merged change so a long list draws once. Out of range
    /// throws in the C#, and does nothing here.
    pub fn undo_many(&mut self, n: usize) -> Change {
        let mut ch = Change::default();
        if n <= self.undoable.len() {
            for _ in 0..n {
                if let Some(mut action) = self.undoable.pop_front() {
                    self.apply_undo(&mut action, true, &mut ch);
                    self.redoable.push(action);
                    ch.undo_changed = true;
                }
            }
        }
        self.end_update(&mut ch);
        ch
    }

    /// `UndoManager.Redo(n)`. See [`undo_many`](Self::undo_many).
    pub fn redo_many(&mut self, n: usize) -> Change {
        let mut ch = Change::default();
        if n <= self.redoable.len() {
            for _ in 0..n {
                if let Some(mut action) = self.redoable.pop() {
                    self.apply_undo(&mut action, false, &mut ch);
                    self.push_undo(action, &mut ch);
                }
            }
        }
        self.end_update(&mut ch);
        ch
    }

    /// `VixenPreviewControl_Resize`: a blank background goes back to the view's size, then
    /// `VerifyPreviewBackgroundSize` replaces both of its sides if a shape is past either, which can shrink one. An image
    /// keeps its size. Asks every shape for its bounds, so call it when a live resize ends and only
    /// [`set_view_size`](Self::set_view_size) during one.
    pub fn view_resized(&mut self, width: i32, height: i32) -> Change {
        let mut ch = Change::default();
        self.view_size = (width, height);
        if self.background_is_blank() {
            self.background_size = Some((width, height));
            self.verify_preview_background_size(&mut ch);
        }
        self.update_scroll_range();
        self.end_update(&mut ch);
        ch
    }

    /// Whether this preview has no image: `DefaultBackground`, and the test on `BackgroundFileName` at load.
    pub fn background_is_blank(&self) -> bool {
        self.data.background_file_name.as_deref().unwrap_or("").is_empty()
    }

    /// `CreateTemplate`'s part after its name dialog, given the group [`group_selected`](Self::group_selected) just made:
    /// names it, selects it and returns the file's text. The C# groups first and then asks, so a refused selection never
    /// sees the dialog and Cancel leaves the group.
    pub fn name_group_as_template(&mut self, id: ShapeId, name: &str) -> (Change, Option<String>) {
        let mut ch = Change::default();
        let mut xml = None;
        if let Some(i) = self.index_of(id) {
            self.data.display_items[i].shape.name = Some(name.to_string());
            self.select_shape(id, &mut ch);
            self.selected_item = Some(id);
            xml = Some(self.data.display_items[i].to_xml_document());
        }
        self.end_update(&mut ch);
        (ch, xml)
    }

    /// `ClipboardPopulated`. Static in the C#, so a new window's context menu still offers Paste: the host keeps it per
    /// process and hands it to each session.
    pub fn set_clipboard_populated(&mut self, on: bool) {
        self.clipboard_populated = on;
    }

    pub fn clipboard_populated(&self) -> bool {
        self.clipboard_populated
    }

    /// See [`ToolbarState`].
    pub fn toolbar_state(&self) -> ToolbarState {
        let single = self.selected_item.and_then(|id| self.item(id));
        let any_multi = !self.selected_items.is_empty();
        let (lock, unlock) = match single {
            Some(item) => (!item.shape.locked, item.shape.locked),
            None => {
                let mut multi = self.selected_items.iter().filter_map(|id| self.item(*id));
                let any_locked = multi.clone().any(|i| i.shape.locked);
                let any_unlocked = multi.any(|i| !i.shape.locked);
                (any_unlocked, any_locked)
            }
        };
        ToolbarState {
            bulb: self.selected_item.is_some() || any_multi,
            lock,
            unlock,
            unlock_all: self.data.display_items.iter().any(|i| i.shape.locked),
            align: self.selected_items.len() > 1,
            hide_locked_enabled: self.hide_locked_command_enabled(),
            hide_locked_active: self.hide_locked,
            can_undo: !self.undoable.is_empty(),
            can_redo: !self.redoable.is_empty(),
            tool: self.tool,
        }
    }

    /// `PopulatePropList`: a group's strings by name, the class name when it has none, and "Unnamed String" when it's empty.
    pub fn string_names(&self, id: ShapeId) -> Vec<String> {
        let Some(strings) = self.item(id).and_then(|i| i.shape.strings()) else { return Vec::new() };
        strings
            .iter()
            .map(|s| match s.name.as_deref() {
                None => s.class_name().to_string(),
                Some("") => "Unnamed String".to_string(),
                Some(name) => name.to_string(),
            })
            .collect()
    }

    /// The property grid embedded in a group's panel, for one of its strings.
    pub fn string_properties(&self, id: ShapeId, string: usize) -> Vec<ShapeProperty> {
        self.item(id).and_then(|i| i.shape.strings()).and_then(|s| s.get(string)).map_or_else(Vec::new, Shape::properties)
    }

    /// An edit in that grid. The C# doesn't redraw until something else does, and this does.
    pub fn set_string_property(&mut self, id: ShapeId, string: usize, name: &str, value: PropertyValue) -> Result<Change, PropertyError> {
        let i = self.index_of(id).ok_or(PropertyError::UnknownShape)?;
        let draw_shape = self.draw_shape.clone();
        let mut ch = Change::default();
        let child = self.data.display_items[i].shape.strings.as_mut().and_then(|s| s.get_mut(string)).ok_or(PropertyError::UnknownShape)?;
        let warning = child.set_property(name, value, &draw_shape)?;
        ch.message = warning.map(str::to_string);
        self.rebuild_item(i, &mut ch);
        self.end_update(&mut ch);
        Ok(ch)
    }

    /// `btnSyncBulbSize_Click`: every string takes this string's light size. No undo.
    pub fn sync_string_light_sizes(&mut self, id: ShapeId, from_string: usize) -> Change {
        let mut ch = Change::default();
        let Some(i) = self.index_of(id) else { return ch };
        if let Some(strings) = self.data.display_items[i].shape.strings.as_mut() {
            if let Some(size) = strings.get(from_string).map(Shape::pixel_size) {
                for s in strings.iter_mut() {
                    s.set_pixel_size(size);
                }
            }
        }
        self.rebuild_item(i, &mut ch);
        self.end_update(&mut ch);
        ch
    }

    /// The shape the Properties panel shows: what `OnSelectDisplayItem` and `OnDeSelectDisplayItem` come to, however many of
    /// them one event ran.
    pub fn properties_shape(&self) -> Option<ShapeId> {
        self.properties_shape
    }

    /// How far the view can scroll, for a host with scroll bars of its own.
    pub fn scroll_max(&self) -> (i32, i32) {
        (self.h_scroll_max, self.v_scroll_max)
    }
}

/// `DeSerializeToDisplayItemList`: nothing at all if the text is not what it should be.
fn read_display_item_list(xml: &str) -> Vec<Shape> {
    let Ok(root) = XElement::parse(xml) else { return Vec::new() };
    if root.local != "ArrayOfDisplayItem" {
        return Vec::new();
    }
    root.elements().filter_map(|e| e.child("Shape")).map(crate::shape::xml::shape_from_xml).collect()
}

/// `DeSerializeToDisplayItem`.
pub(crate) fn read_template(xml: &str) -> Option<Shape> {
    let root = XElement::parse(xml).ok()?;
    if root.local != "DisplayItem" {
        return None;
    }
    Some(crate::shape::xml::shape_from_xml(root.child("Shape")?))
}

/// The part of `AddTtemplateToPreview` that links a template's named Singles to the selected node's leaves.
fn associate_singles_by_name(shape: &mut Shape, node: Uuid, tree: &ElementTree) {
    let Some(n) = tree.find(&node) else { return };
    // GroupBy name, first of each, as a dictionary whose enumeration follows insertion order.
    let mut children: Vec<(String, Uuid)> = Vec::new();
    for leaf in tree.leaves(n) {
        let leaf_node = tree.node(leaf);
        if !children.iter().any(|(name, _)| *name == leaf_node.name) {
            children.push((leaf_node.name.clone(), leaf_node.id));
        }
    }
    let Some(strings) = shape.strings.as_mut() else { return };
    for s in strings {
        if !matches!(s.kind(), ShapeKind::Single) {
            continue;
        }
        let Some(name) = s.name.as_deref() else { continue };
        if let Some((_, id)) = children.iter().find(|(c, _)| c.contains(name)) {
            if let Some(p) = s.pixels.first_mut() {
                p.node_id = *id;
            }
        }
    }
}

/// Writes locations into the tree: `LocationDescriptor`'s property, added to the node if missing.
pub fn apply_location_updates(tree: &mut ElementTree, updates: &[LocationUpdate]) {
    for u in updates {
        let Some(n) = tree.find(&u.node) else { continue };
        let node = tree.node_mut(n);
        match node.properties.iter_mut().find_map(|p| match &mut p.data {
            PropertyData::Location { x, y, .. } => Some((x, y)),
            _ => None,
        }) {
            Some((x, y)) => {
                *x = u.x;
                *y = u.y;
            }
            None => node.properties.push(PropertyInstance {
                type_id: property_ids::LOCATION,
                instance_id: Uuid::new_v4(),
                data: PropertyData::Location { x: u.x, y: u.y, z: 0 },
            }),
        }
    }
}
