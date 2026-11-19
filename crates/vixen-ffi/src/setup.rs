//! The Display Setup window's API. Everything crosses in bulk, per refresh or per gesture, never per row. Long calls are plain functions the Swift side runs on a
//! background task. Every name starts with `Setup` or `DisplaySetup`, because bindings share one namespace across areas.

mod fixture;
mod live;
mod wire;

use crate::core::{FfiError, VixenCore};
use parking_lot::{Mutex, RwLock};
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;
use vixen_session::app::AppCore;
use vixen_session::setup::controllers::{
    controller_types, output_limit, ControllerIcon, ControllerMenuState, ControllerPanelState, ControllerReveal, ControllerRow, ControllerRowKind,
    OUTPUT_PAGE_SIZE,
};
use vixen_session::setup::elements::{EditResult, ElementRow, TreeMenuState};
use vixen_session::setup::flow::Cid;
use vixen_session::setup::graph::{port_fraction, zoom_step, zoom_to_fit, GraphScene, LineKind, Rect, ShapeKind, STUB_LENGTH};

use vixen_session::setup::patching::{
    patch_summary, ControllerStats, ElementStats, OrderCheck, PatchOptions, PatchSummary, UNPATCH_PROMPT_THRESHOLD,
};
use vixen_session::setup::properties::{PropertyRow, PROPERTY_DESCRIPTORS};
use vixen_session::setup::selection::ordered_rows;
use vixen_session::setup::tags::{TagCheck, TagColorRow, TagMenuItem};
use vixen_session::setup::templates::{
    generate_template, template_defaults, template_needs_followups, validate_template, zig_zag_every, StartLocation, TemplateError, TemplateKind,
    TemplateParams, ALL_TEMPLATES, BAD_PATTERN_MESSAGE, BAD_PATTERN_TITLE,
};
use vixen_session::setup::{RowIcon, SetupSession};

fn uuid(text: &str) -> Result<Uuid, FfiError> {
    Uuid::parse_str(text).map_err(|_| FfiError::new("That item no longer exists."))
}

// ---- Records --------------------------------------------------------------------------------------------------------

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum SetupRowIcon {
    Group,
    Patched,
    PatchedMasked,
    Unpatched,
}

impl From<RowIcon> for SetupRowIcon {
    fn from(i: RowIcon) -> SetupRowIcon {
        match i {
            RowIcon::Group => SetupRowIcon::Group,
            RowIcon::Patched => SetupRowIcon::Patched,
            RowIcon::PatchedMasked => SetupRowIcon::PatchedMasked,
            RowIcon::Unpatched => SetupRowIcon::Unpatched,
        }
    }
}

/// A row of the element tree.
#[derive(uniffi::Record)]
pub struct SetupElementRow {
    pub path: String,
    pub name: String,
    pub icon: SetupRowIcon,
    pub has_children: bool,
    pub child_count: u32,
    pub tag_colors: Vec<u32>,
}

impl From<ElementRow> for SetupElementRow {
    fn from(r: ElementRow) -> SetupElementRow {
        SetupElementRow { path: r.path, name: r.name, icon: r.icon.into(), has_children: r.has_children, child_count: r.child_count, tag_colors: r.tag_colors }
    }
}

/// What an edit changed. `reload_nodes` and `refresh_nodes` are the last component of row paths.
#[derive(uniffi::Record)]
pub struct SetupEditResult {
    pub reload_roots: bool,
    pub reload_nodes: Vec<u32>,
    pub refresh_nodes: Vec<u32>,
    pub changes_selection: bool,
    pub select: Vec<String>,
    pub expand: Vec<String>,
    pub scroll_to_top: bool,
    pub tree_revision: u64,
    pub patch_revision: u64,
}

fn edit_result(s: &SetupSession, e: EditResult) -> SetupEditResult {
    let changes_selection = e.select.is_some();
    SetupEditResult {
        reload_roots: e.reload_roots,
        reload_nodes: e.reload_nodes.iter().map(|n| n.0).collect(),
        refresh_nodes: e.refresh_nodes.iter().map(|n| n.0).collect(),
        changes_selection,
        select: e.select.unwrap_or_default(),
        expand: e.expand,
        scroll_to_top: e.scroll_to_top,
        tree_revision: s.tree_revision(),
        patch_revision: s.patch_revision(),
    }
}

#[derive(uniffi::Record)]
pub struct SetupTreeMenuState {
    pub copy: bool,
    pub paste: bool,
    pub paste_as_new: bool,
    pub item_properties: bool,
    pub copy_properties: bool,
    pub paste_properties: bool,
    pub delete: bool,
    pub create_group: bool,
    pub rename: bool,
    pub find_replace: bool,
    pub reverse: bool,
    pub sort: bool,
    pub export_wire_diagram: bool,
    pub export_element_tree: bool,
    pub tags: bool,
}

impl From<TreeMenuState> for SetupTreeMenuState {
    fn from(m: TreeMenuState) -> SetupTreeMenuState {
        SetupTreeMenuState {
            copy: m.copy,
            paste: m.paste,
            paste_as_new: m.paste_as_new,
            item_properties: m.item_properties,
            copy_properties: m.copy_properties,
            paste_properties: m.paste_properties,
            delete: m.delete,
            create_group: m.create_group,
            rename: m.rename,
            find_replace: m.find_replace,
            reverse: m.reverse,
            sort: m.sort,
            export_wire_diagram: m.export_wire_diagram,
            export_element_tree: m.export_element_tree,
            tags: m.tags,
        }
    }
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum SetupTagCheck {
    Off,
    On,
    Mixed,
}

#[derive(uniffi::Record)]
pub struct SetupTagMenuItem {
    pub id: String,
    pub name: String,
    pub color: Option<u32>,
    pub check: SetupTagCheck,
}

impl From<TagMenuItem> for SetupTagMenuItem {
    fn from(t: TagMenuItem) -> SetupTagMenuItem {
        SetupTagMenuItem {
            id: t.id.to_string(),
            name: t.name,
            color: t.color,
            check: match t.check {
                TagCheck::Off => SetupTagCheck::Off,
                TagCheck::On => SetupTagCheck::On,
                TagCheck::Mixed => SetupTagCheck::Mixed,
            },
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct SetupTagColor {
    pub id: String,
    pub name: String,
    pub color: u32,
}

impl From<TagColorRow> for SetupTagColor {
    fn from(t: TagColorRow) -> SetupTagColor {
        SetupTagColor { id: t.id.to_string(), name: t.name, color: t.color }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct SetupPropertyRow {
    pub type_id: String,
    pub type_name: String,
    pub instance_id: String,
    pub has_setup: bool,
    pub has_helper: bool,
    pub is_color: bool,
}

impl From<PropertyRow> for SetupPropertyRow {
    fn from(p: PropertyRow) -> SetupPropertyRow {
        SetupPropertyRow {
            type_id: p.type_id.to_string(),
            type_name: p.type_name,
            instance_id: p.instance_id.to_string(),
            has_setup: p.has_setup,
            has_helper: p.has_helper,
            is_color: p.is_color,
        }
    }
}

/// A module in one of the window's type lists.
#[derive(uniffi::Record, Clone)]
pub struct SetupModuleType {
    pub id: String,
    pub name: String,
}

#[derive(uniffi::Record, Default, Clone)]
pub struct SetupElementStats {
    pub total: u32,
    pub connected: u32,
    pub unconnected: u32,
    pub items: u32,
    pub groups: u32,
    pub leaves: u32,
    pub filters: u32,
}

impl From<ElementStats> for SetupElementStats {
    fn from(s: ElementStats) -> SetupElementStats {
        SetupElementStats { total: s.total, connected: s.connected, unconnected: s.unconnected, items: s.items, groups: s.groups, leaves: s.leaves, filters: s.filters }
    }
}

impl From<&SetupElementStats> for ElementStats {
    fn from(s: &SetupElementStats) -> ElementStats {
        ElementStats { total: s.total, connected: s.connected, unconnected: s.unconnected, items: s.items, groups: s.groups, leaves: s.leaves, filters: s.filters }
    }
}

#[derive(uniffi::Record, Default, Clone)]
pub struct SetupControllerStats {
    pub outputs: u32,
    pub patched: u32,
    pub unpatched: u32,
    pub controllers: u32,
    pub first_label: String,
    pub last_label: String,
}

impl From<ControllerStats> for SetupControllerStats {
    fn from(s: ControllerStats) -> SetupControllerStats {
        SetupControllerStats { outputs: s.outputs, patched: s.patched, unpatched: s.unpatched, controllers: s.controllers, first_label: s.first_label, last_label: s.last_label }
    }
}

impl From<&SetupControllerStats> for ControllerStats {
    fn from(s: &SetupControllerStats) -> ControllerStats {
        ControllerStats {
            outputs: s.outputs,
            patched: s.patched,
            unpatched: s.unpatched,
            controllers: s.controllers,
            first_label: s.first_label.clone(),
            last_label: s.last_label.clone(),
        }
    }
}

#[derive(uniffi::Record, Clone, Copy, Default, PartialEq, Eq)]
pub struct SetupPatchOptions {
    pub all_points: bool,
    pub all_outputs: bool,
    pub reverse_elements: bool,
    pub reverse_outputs: bool,
}

impl From<SetupPatchOptions> for PatchOptions {
    fn from(o: SetupPatchOptions) -> PatchOptions {
        PatchOptions { all_points: o.all_points, all_outputs: o.all_outputs, reverse_elements: o.reverse_elements, reverse_outputs: o.reverse_outputs }
    }
}

#[derive(uniffi::Record, Default, Clone)]
pub struct SetupPatchSummary {
    pub sources: u32,
    pub destinations: u32,
    pub text: String,
    pub warning: String,
    pub can_patch: bool,
}

impl From<PatchSummary> for SetupPatchSummary {
    fn from(s: PatchSummary) -> SetupPatchSummary {
        SetupPatchSummary { sources: s.sources, destinations: s.destinations, text: s.text, warning: s.warning, can_patch: s.can_patch }
    }
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum SetupOrderCheck {
    TreeOrder,
    UseOrder,
    MissingSome,
    Duplicates,
}

#[derive(uniffi::Record)]
pub struct SetupUnpatchPreview {
    pub patched: u32,
    pub any_filters: bool,
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum SetupControllerIcon {
    Group,
    Stopped,
    OutputUnpatched,
    OutputBroken,
    OutputPatched,
}

impl From<ControllerIcon> for SetupControllerIcon {
    fn from(i: ControllerIcon) -> SetupControllerIcon {
        match i {
            ControllerIcon::Group => SetupControllerIcon::Group,
            ControllerIcon::Stopped => SetupControllerIcon::Stopped,
            ControllerIcon::OutputUnpatched => SetupControllerIcon::OutputUnpatched,
            ControllerIcon::OutputBroken => SetupControllerIcon::OutputBroken,
            ControllerIcon::OutputPatched => SetupControllerIcon::OutputPatched,
        }
    }
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum SetupControllerRowKind {
    Controller,
    Page,
    Output,
}

#[derive(uniffi::Record)]
pub struct SetupControllerRow {
    pub id: String,
    pub title: String,
    pub kind: SetupControllerRowKind,
    pub icon: SetupControllerIcon,
    pub has_children: bool,
    pub child_count: u32,
}

impl From<ControllerRow> for SetupControllerRow {
    fn from(r: ControllerRow) -> SetupControllerRow {
        SetupControllerRow {
            id: r.id,
            title: r.title,
            kind: match r.kind {
                ControllerRowKind::Controller => SetupControllerRowKind::Controller,
                ControllerRowKind::Page => SetupControllerRowKind::Page,
                ControllerRowKind::Output => SetupControllerRowKind::Output,
            },
            icon: r.icon.into(),
            has_children: r.has_children,
            child_count: r.child_count,
        }
    }
}

#[derive(uniffi::Record, Default, Clone)]
pub struct SetupControllerPanelState {
    pub type_label: String,
    pub outputs_label: String,
    pub one_controller: bool,
    pub any_controller: bool,
    pub can_start: bool,
    pub can_stop: bool,
    pub can_find_elements: bool,
    pub has_setup: bool,
}

impl From<ControllerPanelState> for SetupControllerPanelState {
    fn from(s: ControllerPanelState) -> SetupControllerPanelState {
        SetupControllerPanelState {
            type_label: s.type_label,
            outputs_label: s.outputs_label,
            one_controller: s.one_controller,
            any_controller: s.any_controller,
            can_start: s.can_start,
            can_stop: s.can_stop,
            can_find_elements: s.can_find_elements,
            has_setup: s.has_setup,
        }
    }
}

#[derive(uniffi::Record, Default, Clone)]
pub struct SetupControllerMenuState {
    pub show: bool,
    pub controller_items: bool,
    pub configure: bool,
    pub channel_count: bool,
    pub rename: bool,
    pub delete: bool,
    pub start: bool,
    pub stop: bool,
    pub unpatch_controllers: bool,
    pub unpatch_controllers_title: String,
    pub insert_channels: bool,
    pub unpatch_channels: bool,
    pub unpatch_channels_title: String,
    pub find_elements: bool,
    pub find_elements_title: String,
}

impl From<ControllerMenuState> for SetupControllerMenuState {
    fn from(m: ControllerMenuState) -> SetupControllerMenuState {
        SetupControllerMenuState {
            show: m.show,
            controller_items: m.controller_items,
            configure: m.configure,
            channel_count: m.channel_count,
            rename: m.rename,
            delete: m.delete,
            start: m.start,
            stop: m.stop,
            unpatch_controllers: m.unpatch_controllers,
            unpatch_controllers_title: m.unpatch_controllers_title,
            insert_channels: m.insert_channels,
            unpatch_channels: m.unpatch_channels,
            unpatch_channels_title: m.unpatch_channels_title,
            find_elements: m.find_elements,
            find_elements_title: m.find_elements_title,
        }
    }
}

#[derive(uniffi::Record)]
pub struct SetupControllerReveal {
    pub expand: Vec<String>,
    pub select: Vec<String>,
}

impl From<ControllerReveal> for SetupControllerReveal {
    fn from(r: ControllerReveal) -> SetupControllerReveal {
        SetupControllerReveal { expand: r.expand, select: r.select }
    }
}

/// What a dialog about one controller needs.
#[derive(uniffi::Record)]
pub struct SetupControllerInfo {
    pub id: String,
    pub name: String,
    pub type_id: String,
    pub type_name: String,
    pub instance_id: String,
    pub output_count: u32,
    pub has_setup: bool,
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum SetupTemplateKind {
    Arch,
    CandyCane,
    Icicles,
    IntelligentFixture,
    LipSync,
    Megatree,
    NumberedGroup,
    PixelGrid,
    SingleItem,
    Star,
    Starburst,
}

impl From<TemplateKind> for SetupTemplateKind {
    fn from(k: TemplateKind) -> SetupTemplateKind {
        match k {
            TemplateKind::Arch => SetupTemplateKind::Arch,
            TemplateKind::CandyCane => SetupTemplateKind::CandyCane,
            TemplateKind::Icicles => SetupTemplateKind::Icicles,
            TemplateKind::IntelligentFixture => SetupTemplateKind::IntelligentFixture,
            TemplateKind::LipSync => SetupTemplateKind::LipSync,
            TemplateKind::Megatree => SetupTemplateKind::Megatree,
            TemplateKind::NumberedGroup => SetupTemplateKind::NumberedGroup,
            TemplateKind::PixelGrid => SetupTemplateKind::PixelGrid,
            TemplateKind::SingleItem => SetupTemplateKind::SingleItem,
            TemplateKind::Star => SetupTemplateKind::Star,
            TemplateKind::Starburst => SetupTemplateKind::Starburst,
        }
    }
}

impl From<SetupTemplateKind> for TemplateKind {
    fn from(k: SetupTemplateKind) -> TemplateKind {
        match k {
            SetupTemplateKind::Arch => TemplateKind::Arch,
            SetupTemplateKind::CandyCane => TemplateKind::CandyCane,
            SetupTemplateKind::Icicles => TemplateKind::Icicles,
            SetupTemplateKind::IntelligentFixture => TemplateKind::IntelligentFixture,
            SetupTemplateKind::LipSync => TemplateKind::LipSync,
            SetupTemplateKind::Megatree => TemplateKind::Megatree,
            SetupTemplateKind::NumberedGroup => TemplateKind::NumberedGroup,
            SetupTemplateKind::PixelGrid => TemplateKind::PixelGrid,
            SetupTemplateKind::SingleItem => TemplateKind::SingleItem,
            SetupTemplateKind::Star => TemplateKind::Star,
            SetupTemplateKind::Starburst => TemplateKind::Starburst,
        }
    }
}

#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum SetupStartLocation {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

impl From<SetupStartLocation> for StartLocation {
    fn from(s: SetupStartLocation) -> StartLocation {
        match s {
            SetupStartLocation::BottomLeft => StartLocation::BottomLeft,
            SetupStartLocation::BottomRight => StartLocation::BottomRight,
            SetupStartLocation::TopLeft => StartLocation::TopLeft,
            SetupStartLocation::TopRight => StartLocation::TopRight,
        }
    }
}

impl From<StartLocation> for SetupStartLocation {
    fn from(s: StartLocation) -> SetupStartLocation {
        match s {
            StartLocation::BottomLeft => SetupStartLocation::BottomLeft,
            StartLocation::BottomRight => SetupStartLocation::BottomRight,
            StartLocation::TopLeft => SetupStartLocation::TopLeft,
            StartLocation::TopRight => SetupStartLocation::TopRight,
        }
    }
}

/// A template's dialog fields, flat so one form model holds any of them. What each means depends on `kind`: `flag` is Pixel Tree or Pixel Starburst on a tree and a
/// starburst, and Horizontal on a grid. `count` and `second_count` are the two sizes, in the dialog's order. On a grid they are rows and columns.
#[derive(uniffi::Record, Clone)]
pub struct SetupTemplateDraft {
    pub kind: SetupTemplateKind,
    pub name: String,
    pub prefix: String,
    pub second_prefix: String,
    pub count: u32,
    pub second_count: u32,
    pub pattern: String,
    pub flag: bool,
    pub start: SetupStartLocation,
    pub zig_zag: bool,
}

fn draft_of(kind: SetupTemplateKind, params: &TemplateParams) -> SetupTemplateDraft {
    let mut d = SetupTemplateDraft {
        kind,
        name: String::new(),
        prefix: String::new(),
        second_prefix: String::new(),
        count: 0,
        second_count: 0,
        pattern: String::new(),
        flag: false,
        start: SetupStartLocation::BottomLeft,
        zig_zag: false,
    };
    match params {
        TemplateParams::Single { name } | TemplateParams::LipSync { name } => d.name.clone_from(name),
        TemplateParams::Numbered { group, prefix, count } => {
            d.name.clone_from(group);
            d.prefix.clone_from(prefix);
            d.count = *count;
        }
        TemplateParams::Starburst { name, spokes, spoke_prefix, pixel, pixels, pixel_prefix } => {
            d.name.clone_from(name);
            d.count = *spokes;
            d.prefix.clone_from(spoke_prefix);
            d.flag = *pixel;
            d.second_count = *pixels;
            d.second_prefix.clone_from(pixel_prefix);
        }
        TemplateParams::Icicles { name, strings, pattern, string_prefix, pixel_prefix } => {
            d.name.clone_from(name);
            d.count = *strings;
            d.pattern.clone_from(pattern);
            d.prefix.clone_from(string_prefix);
            d.second_prefix.clone_from(pixel_prefix);
        }
        TemplateParams::Megatree { name, strings, string_prefix, pixel, pixels, pixel_prefix, start, zig_zag } => {
            d.name.clone_from(name);
            d.count = *strings;
            d.prefix.clone_from(string_prefix);
            d.flag = *pixel;
            d.second_count = *pixels;
            d.second_prefix.clone_from(pixel_prefix);
            d.start = (*start).into();
            d.zig_zag = *zig_zag;
        }
        TemplateParams::PixelGrid { name, horizontal, rows, columns, first_prefix, second_prefix, start, zig_zag } => {
            d.name.clone_from(name);
            d.flag = *horizontal;
            d.count = *rows;
            d.second_count = *columns;
            d.prefix.clone_from(first_prefix);
            d.second_prefix.clone_from(second_prefix);
            d.start = (*start).into();
            d.zig_zag = *zig_zag;
        }
    }
    d
}

fn params_of(d: &SetupTemplateDraft) -> TemplateParams {
    let name = d.name.clone();
    match d.kind {
        SetupTemplateKind::SingleItem => TemplateParams::Single { name },
        SetupTemplateKind::LipSync => TemplateParams::LipSync { name },
        SetupTemplateKind::NumberedGroup | SetupTemplateKind::Arch | SetupTemplateKind::CandyCane | SetupTemplateKind::Star => {
            TemplateParams::Numbered { group: name, prefix: d.prefix.clone(), count: d.count }
        }
        SetupTemplateKind::Starburst => TemplateParams::Starburst {
            name,
            spokes: d.count,
            spoke_prefix: d.prefix.clone(),
            pixel: d.flag,
            pixels: d.second_count,
            pixel_prefix: d.second_prefix.clone(),
        },
        SetupTemplateKind::Icicles => TemplateParams::Icicles {
            name,
            strings: d.count,
            pattern: d.pattern.clone(),
            string_prefix: d.prefix.clone(),
            pixel_prefix: d.second_prefix.clone(),
        },
        SetupTemplateKind::Megatree => TemplateParams::Megatree {
            name,
            strings: d.count,
            string_prefix: d.prefix.clone(),
            pixel: d.flag,
            pixels: d.second_count,
            pixel_prefix: d.second_prefix.clone(),
            start: d.start.into(),
            zig_zag: d.zig_zag,
        },
        SetupTemplateKind::PixelGrid | SetupTemplateKind::IntelligentFixture => TemplateParams::PixelGrid {
            name,
            horizontal: d.flag,
            rows: d.count,
            columns: d.second_count,
            first_prefix: d.prefix.clone(),
            second_prefix: d.second_prefix.clone(),
            start: d.start.into(),
            zig_zag: d.zig_zag,
        },
    }
}

#[derive(uniffi::Record, Clone)]
pub struct SetupTemplateInfo {
    pub kind: SetupTemplateKind,
    pub name: String,
    pub ask_dimming: bool,
    pub run_color: bool,
}

/// An alert a dialog shows and stays open under.
#[derive(uniffi::Record)]
pub struct SetupTemplateProblem {
    pub title: String,
    pub message: String,
}

/// What generating a template made. `created` holds every node, head first, for the follow-up helpers.
#[derive(uniffi::Record)]
pub struct SetupTemplateOutcome {
    pub edit: SetupEditResult,
    pub created: Vec<u32>,
    pub created_paths: Vec<String>,
}

/// A graph scene, as parallel arrays per shape and per line, so a scene of ten thousand shapes doesn't make ten thousand objects to cross here.
#[derive(uniffi::Record)]
pub struct SetupGraphScene {
    pub ids: Vec<u64>,
    /// `SetupGraphShapeKind` as numbers: 0 element group, 1 element leaf, 2 filter, 3 controller, 4 output.
    pub kinds: Vec<u8>,
    pub titles: Vec<String>,
    /// `x, y, w, h` per shape.
    pub rects: Vec<f32>,
    pub z: Vec<u8>,
    /// Bit 0 hidden, bit 1 expandable, bit 2 collapsed.
    pub flags: Vec<u8>,
    pub inputs: Vec<u32>,
    pub outputs: Vec<u32>,
    /// The data flow component, or -1.
    pub components: Vec<i64>,
    pub parents: Vec<i32>,
    pub line_ids: Vec<u64>,
    /// 0 real, 1 source stub, 2 destination stub.
    pub line_kinds: Vec<u8>,
    /// `x1, y1, x2, y2` per line.
    pub line_points: Vec<f32>,
    pub line_from: Vec<i32>,
    pub line_from_ports: Vec<u32>,
    pub line_to: Vec<i32>,
    /// What Delete cuts, or -1.
    pub line_destinations: Vec<i64>,
    pub content_width: f32,
    pub content_height: f32,
}

impl From<GraphScene> for SetupGraphScene {
    fn from(scene: GraphScene) -> SetupGraphScene {
        let n = scene.shapes.len();
        let m = scene.lines.len();
        let mut out = SetupGraphScene {
            ids: Vec::with_capacity(n),
            kinds: Vec::with_capacity(n),
            titles: Vec::with_capacity(n),
            rects: Vec::with_capacity(n * 4),
            z: Vec::with_capacity(n),
            flags: Vec::with_capacity(n),
            inputs: Vec::with_capacity(n),
            outputs: Vec::with_capacity(n),
            components: Vec::with_capacity(n),
            parents: Vec::with_capacity(n),
            line_ids: Vec::with_capacity(m),
            line_kinds: Vec::with_capacity(m),
            line_points: Vec::with_capacity(m * 4),
            line_from: Vec::with_capacity(m),
            line_from_ports: Vec::with_capacity(m),
            line_to: Vec::with_capacity(m),
            line_destinations: Vec::with_capacity(m),
            content_width: scene.content_width,
            content_height: scene.content_height,
        };
        for s in scene.shapes {
            out.ids.push(s.id);
            out.kinds.push(match s.kind {
                ShapeKind::ElementGroup => 0,
                ShapeKind::ElementLeaf => 1,
                ShapeKind::Filter => 2,
                ShapeKind::Controller => 3,
                ShapeKind::Output => 4,
            });
            out.titles.push(s.title);
            out.rects.extend_from_slice(&[s.rect.x, s.rect.y, s.rect.w, s.rect.h]);
            out.z.push(s.z);
            out.flags.push(u8::from(s.hidden) | u8::from(s.expandable) << 1 | u8::from(s.collapsed) << 2);
            out.inputs.push(s.inputs);
            out.outputs.push(s.outputs);
            out.components.push(s.component.map_or(-1, |c| i64::from(c.0)));
            out.parents.push(s.parent.map_or(-1, |p| p as i32));
        }
        for l in scene.lines {
            out.line_ids.push(l.id);
            out.line_kinds.push(match l.kind {
                LineKind::Real => 0,
                LineKind::SourceStub => 1,
                LineKind::DestinationStub => 2,
            });
            out.line_points.extend_from_slice(&[l.x1, l.y1, l.x2, l.y2]);
            out.line_from.push(l.from_shape.map_or(-1, |s| s as i32));
            out.line_from_ports.push(l.from_port);
            out.line_to.push(l.to_shape.map_or(-1, |s| s as i32));
            out.line_destinations.push(l.destination.map_or(-1, |c| i64::from(c.0)));
        }
        out
    }
}

/// A filter's place on the canvas after a gesture.
#[derive(uniffi::Record, Clone, Copy)]
pub struct SetupFilterPlacement {
    pub component: u32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(uniffi::Record, Clone)]
pub struct SetupCommitProgress {
    pub percent: u32,
    pub text: String,
    pub running: bool,
}

// ---- The session --------------------------------------------------------------------------------------------------

/// The window's working copy of the display.
#[derive(uniffi::Object)]
pub struct DisplaySetupSession {
    core: Arc<AppCore>,
    pub(crate) inner: RwLock<SetupSession>, // STUDIO: the live Display pane publishes this working copy (studio_agent.rs)
    progress: Mutex<SetupCommitProgress>,
}

#[uniffi::export]
impl VixenCore {
    pub fn element_tag_colors(&self) -> Vec<SetupTagColor> {
        self.inner.element_tag_colors().into_iter().map(Into::into).collect()
    }

    pub fn save_element_tag_colors(&self, colors: Vec<SetupTagColor>) -> Result<(), FfiError> {
        let colors: Result<Vec<_>, FfiError> = colors.into_iter().map(|c| Ok((uuid(&c.id)?, c.color))).collect();
        self.inner.save_element_tag_colors(&colors?)?;
        Ok(())
    }

    /// A working copy of the display as it stands. Takes a moment on a big show, so call it off the main thread.
    pub fn open_display_setup(&self) -> Arc<DisplaySetupSession> {
        let session = SetupSession::open(&self.inner);
        Arc::new(DisplaySetupSession {
            core: Arc::clone(&self.inner),
            inner: RwLock::new(session),
            progress: Mutex::new(SetupCommitProgress { percent: 0, text: String::new(), running: false }),
        })
    }

    /// Bumps whenever Display Setup's OK changes the display, for anything that keeps something derived from the tree.
    pub fn display_revision(&self) -> u64 {
        self.inner.display.revision()
    }
}

#[uniffi::export]
impl DisplaySetupSession {
    // ---- The whole window -----------------------------------------------------------------------------------------

    pub fn is_dirty(&self) -> bool {
        self.inner.read().is_dirty()
    }

    /// Patches in the file that couldn't be used, as a line each, for the log.
    pub fn load_warnings(&self) -> Vec<String> {
        self.inner.read().warnings.iter().map(|w| format!("{w:?}")).collect()
    }

    /// F6. False if nothing had changed, so nothing was written. Keeps the session open on an error.
    pub fn commit(&self) -> Result<bool, FfiError> {
        *self.progress.lock() = SetupCommitProgress { percent: 0, text: "Preparing Display Setup Changes".to_string(), running: true };
        let result = self.inner.write().commit(&self.core, |percent, text| {
            *self.progress.lock() = SetupCommitProgress { percent, text: text.to_string(), running: true };
        });
        self.progress.lock().running = false;
        Ok(result?)
    }

    /// Where a running `commit` has got to. Never waits for it.
    pub fn commit_progress(&self) -> SetupCommitProgress {
        self.progress.lock().clone()
    }

    /// F7: restores the run state of controllers started or stopped here. Blocks while a controller stops.
    pub fn discard(&self) {
        self.inner.write().discard(&self.core);
    }

    /// F53.
    pub fn export_element_tree(&self, path: String) -> Result<(), FfiError> {
        let session = self.inner.read();
        vixen_session::setup::export::write_element_tree(&session.tree, Path::new(&path))?;
        Ok(())
    }

    // ---- Element rows ----------------------------------------------------------------------------------------------

    pub fn element_rows(&self, parent: Option<String>) -> Vec<SetupElementRow> {
        self.inner.read().element_rows(parent.as_deref()).into_iter().map(Into::into).collect()
    }

    pub fn element_rows_at(&self, paths: Vec<String>) -> Vec<Option<SetupElementRow>> {
        self.inner.read().element_rows_at(&paths).into_iter().map(|r| r.map(Into::into)).collect()
    }

    /// Icons for the rows on screen. Empty if a long edit holds the session right now: the edit's own refresh will come.
    pub fn try_element_icons(&self, paths: Vec<String>) -> Vec<SetupRowIcon> {
        match self.inner.try_read() {
            Some(s) => s.element_icons(&paths).into_iter().map(Into::into).collect(),
            None => Vec::new(),
        }
    }

    pub fn context_menu_state(&self, selection: Vec<String>, primary: Option<String>) -> SetupTreeMenuState {
        self.inner.read().context_menu_state(&selection, primary.as_deref()).into()
    }

    pub fn would_lose_patches(&self, destination: Option<String>) -> bool {
        self.inner.read().would_lose_patches(destination.as_deref())
    }

    pub fn names_of(&self, selection: Vec<String>) -> Vec<String> {
        self.inner.read().names_of(&selection)
    }

    /// The node ids of these rows, as the setup dialogs name them, with repeats dropped.
    pub fn node_ids_of(&self, selection: Vec<String>) -> Vec<String> {
        let s = self.inner.read();
        let mut seen = std::collections::HashSet::new();
        selection
            .iter()
            .filter_map(|p| s.tree.resolve(p)?.last().copied())
            .filter(|n| seen.insert(*n))
            .map(|n| s.tree.node(n).id.to_string())
            .collect()
    }

    /// The instance id of a filter's component, as its setup dialog names it.
    pub fn filter_instance_id(&self, component: u32) -> Option<String> {
        self.inner.read().filter_for(Cid(component)).map(|f| f.instance_id.to_string())
    }

    /// `(paste, paste as new)` for a destination row.
    pub fn can_paste(&self, destination: Option<String>) -> Vec<bool> {
        let s = self.inner.read();
        let node = destination.as_deref().and_then(|d| s.tree.resolve(d)).and_then(|p| p.last().copied());
        let (a, b) = s.clipboard_state(node);
        vec![a, b]
    }

    // ---- Element commands --------------------------------------------------------------------------------------------

    pub fn add_element(&self, parent: Option<String>, name: String) -> SetupEditResult {
        let mut s = self.inner.write();
        let e = s.add_element(parent.as_deref(), &name);
        edit_result(&s, e)
    }

    pub fn add_elements(&self, parent: Option<String>, names: Vec<String>) -> SetupEditResult {
        let mut s = self.inner.write();
        let e = s.add_elements(parent.as_deref(), &names);
        edit_result(&s, e)
    }

    pub fn delete_rows(&self, selection: Vec<String>) -> SetupEditResult {
        let mut s = self.inner.write();
        let e = s.delete_rows(&selection);
        edit_result(&s, e)
    }

    pub fn create_group(&self, selection: Vec<String>, name: String) -> SetupEditResult {
        let mut s = self.inner.write();
        let e = s.create_group(&selection, &name);
        edit_result(&s, e)
    }

    pub fn rename_rows(&self, selection: Vec<String>, names: Vec<String>) -> SetupEditResult {
        let mut s = self.inner.write();
        let e = s.rename_rows(&selection, &names);
        edit_result(&s, e)
    }

    pub fn copy_rows(&self, selection: Vec<String>) {
        self.inner.write().copy_rows(&selection);
    }

    pub fn paste_links(&self, destination: Option<String>) -> SetupEditResult {
        let mut s = self.inner.write();
        let e = s.paste_links(destination.as_deref());
        edit_result(&s, e)
    }

    pub fn paste_as_new(&self, destination: Option<String>) -> SetupEditResult {
        let mut s = self.inner.write();
        let e = s.paste_as_new(destination.as_deref());
        edit_result(&s, e)
    }

    /// `index` is the child position `NSOutlineView` proposes, or `None` for a drop on the row itself.
    pub fn can_drop_rows(&self, sources: Vec<String>, parent: Option<String>, index: Option<u32>, copy: bool) -> bool {
        self.inner.read().can_drop_rows(&sources, parent.as_deref(), index, copy)
    }

    pub fn drop_rows(&self, sources: Vec<String>, parent: Option<String>, index: Option<u32>, copy: bool) -> SetupEditResult {
        let mut s = self.inner.write();
        let e = s.drop_rows(&sources, parent.as_deref(), index, copy);
        edit_result(&s, e)
    }

    pub fn sort_group(&self, path: String) -> SetupEditResult {
        let mut s = self.inner.write();
        let e = s.sort_group(&path);
        edit_result(&s, e)
    }

    pub fn reverse_rows(&self, selection: Vec<String>) -> SetupEditResult {
        let mut s = self.inner.write();
        let e = s.reverse_rows(&selection);
        edit_result(&s, e)
    }

    /// F100 step 2. An `Err` here is upstream's "Could not create elements" case.
    pub fn generate_template(&self, draft: SetupTemplateDraft) -> Result<SetupTemplateOutcome, FfiError> {
        let mut guard = self.inner.write();
        let s = &mut *guard;
        let result = generate_template(&mut s.tree, &params_of(&draft)).map_err(|e| match e {
            TemplateError::EmptyName => FfiError::new("Could not create elements."),
            TemplateError::BadPattern => FfiError::new(BAD_PATTERN_MESSAGE),
        })?;
        let head = result.head;
        let created = result.created.clone();
        s.apply_template_result(result);
        let mut edit = EditResult { reload_roots: true, scroll_to_top: true, ..EditResult::default() };
        edit.select = head.map(|h| vec![s.row_path_of(h)]);
        let created_paths = created.iter().map(|n| s.row_path_of(*n)).collect();
        Ok(SetupTemplateOutcome { edit: edit_result(s, edit), created: created.iter().map(|n| n.0).collect(), created_paths })
    }

    // ---- Tags ----------------------------------------------------------------------------------------------------------

    pub fn tag_menu(&self, selection: Vec<String>) -> Vec<SetupTagMenuItem> {
        self.inner.read().tag_menu(&selection).into_iter().map(Into::into).collect()
    }

    /// F55. Returns the nodes to repaint.
    pub fn toggle_tag(&self, selection: Vec<String>, tag: String, cascade: bool) -> Result<Vec<u32>, FfiError> {
        let tag = uuid(&tag)?;
        Ok(self.inner.write().toggle_tag(&selection, tag, cascade).into_iter().map(|n| n.0).collect())
    }

    pub fn tag_colors(&self) -> Vec<SetupTagColor> {
        self.inner.read().tag_colors().into_iter().map(Into::into).collect()
    }

    pub fn set_tag_colors(&self, colors: Vec<SetupTagColor>) {
        let list: Vec<(Uuid, u32)> = colors.iter().filter_map(|c| Some((Uuid::parse_str(&c.id).ok()?, c.color))).collect();
        self.inner.write().set_tag_colors(&list);
    }

    // ---- Properties ------------------------------------------------------------------------------------------------------

    pub fn properties_of(&self, primary: String) -> Vec<SetupPropertyRow> {
        self.inner.read().properties_of(&primary).into_iter().map(Into::into).collect()
    }

    /// F17. Returns the nodes that got it.
    pub fn add_property(&self, selection: Vec<String>, type_id: String) -> Result<Vec<u32>, FfiError> {
        let id = uuid(&type_id)?;
        Ok(self.inner.write().add_property(&selection, id).into_iter().map(|n| n.0).collect())
    }

    pub fn remove_properties(&self, selection: Vec<String>, type_ids: Vec<String>) {
        let ids: Vec<Uuid> = type_ids.iter().filter_map(|t| Uuid::parse_str(t).ok()).collect();
        self.inner.write().remove_properties(&selection, &ids);
    }

    pub fn clone_property_to_selection(&self, selection: Vec<String>, source: String, type_id: String) -> Result<(), FfiError> {
        let id = uuid(&type_id)?;
        self.inner.write().clone_property_to_selection(&selection, &source, id);
        Ok(())
    }

    pub fn copy_properties(&self, path: String) {
        self.inner.write().copy_properties(&path);
    }

    pub fn paste_properties(&self, selection: Vec<String>) {
        self.inner.write().paste_properties(&selection);
    }

    // ---- Simple patching ---------------------------------------------------------------------------------------------------

    /// F62. Run this off the main thread, with the newest selection only.
    pub fn element_stats(&self, selection: Vec<String>) -> SetupElementStats {
        self.inner.read().element_stats(&selection).into()
    }

    /// F65.
    pub fn controller_stats(&self, rows: Vec<String>) -> SetupControllerStats {
        let s = self.inner.read();
        s.controller_stats(&s.output_selection(&rows)).into()
    }

    pub fn order_check(&self, selection: Vec<String>) -> SetupOrderCheck {
        match self.inner.read().order_check(&selection) {
            OrderCheck::TreeOrder => SetupOrderCheck::TreeOrder,
            OrderCheck::UseOrder => SetupOrderCheck::UseOrder,
            OrderCheck::MissingSome => SetupOrderCheck::MissingSome,
            OrderCheck::Duplicates => SetupOrderCheck::Duplicates,
        }
    }

    /// F69, once its prompts are answered.
    pub fn patch_elements(&self, selection: Vec<String>, rows: Vec<String>, options: SetupPatchOptions, use_order: bool) -> u32 {
        let mut s = self.inner.write();
        let outputs = s.output_selection(&rows);
        s.patch_elements(&selection, &outputs, &options.into(), use_order)
    }

    pub fn unpatch_elements_preview(&self, selection: Vec<String>) -> SetupUnpatchPreview {
        let (patched, any_filters) = self.inner.read().unpatch_elements_preview(&selection);
        SetupUnpatchPreview { patched, any_filters }
    }

    pub fn unpatch_elements(&self, selection: Vec<String>, remove_filters: bool) {
        self.inner.write().unpatch_elements(&selection, remove_filters);
    }

    pub fn unpatch_outputs(&self, rows: Vec<String>) {
        let mut s = self.inner.write();
        let outputs = s.output_selection(&rows);
        s.unpatch_outputs(&outputs);
    }

    /// F14. Empty means "No controller patch points found."
    pub fn find_outputs(&self, selection: Vec<String>) -> SetupControllerReveal {
        let s = self.inner.read();
        s.reveal_outputs(&s.find_outputs(&selection)).into()
    }

    /// F84. No rows selected means "No element patch points found."
    pub fn find_elements(&self, rows: Vec<String>) -> SetupEditResult {
        let s = self.inner.read();
        let nodes = s.find_elements(&s.output_selection(&rows));
        let e = if nodes.is_empty() { EditResult::default() } else { s.reveal_nodes(&nodes) };
        edit_result(&s, e)
    }

    // ---- Controllers ---------------------------------------------------------------------------------------------------------

    pub fn controller_rows(&self) -> Vec<SetupControllerRow> {
        self.inner.read().controller_rows().into_iter().map(Into::into).collect()
    }

    pub fn controller_children(&self, parent: String) -> Vec<SetupControllerRow> {
        self.inner.read().controller_children(&parent).into_iter().map(Into::into).collect()
    }

    pub fn try_controller_icons(&self, rows: Vec<String>) -> Vec<SetupControllerIcon> {
        match self.inner.try_read() {
            Some(s) => s.controller_icons(&rows).into_iter().map(Into::into).collect(),
            None => Vec::new(),
        }
    }

    pub fn controller_panel_state(&self, rows: Vec<String>) -> SetupControllerPanelState {
        self.inner.read().controller_panel_state(&rows).into()
    }

    pub fn controller_menu_state(&self, rows: Vec<String>) -> SetupControllerMenuState {
        self.inner.read().controller_menu_state(&rows).into()
    }

    /// The first selected controller's details, for its prompts and dialogs.
    pub fn first_controller_info(&self, rows: Vec<String>) -> Option<SetupControllerInfo> {
        let s = self.inner.read();
        let slot = *s.tree_selection(&rows).controllers.first()?;
        let c = s.controllers.get(slot as usize)?;
        Some(SetupControllerInfo {
            id: c.id.to_string(),
            name: c.name.clone(),
            type_id: c.type_id.to_string(),
            type_name: c.type_name().to_string(),
            instance_id: c.instance_id.to_string(),
            output_count: c.outputs.len() as u32,
            has_setup: c.has_setup(),
        })
    }

    /// F77. Returns the new root row.
    pub fn add_controller(&self, type_id: String, name: String, outputs: u32) -> Result<String, FfiError> {
        let id = uuid(&type_id)?;
        let slot = self.inner.write().add_controller(id, &name, outputs).map_err(FfiError::new)?;
        Ok(format!("c{slot}"))
    }

    pub fn rename_controller(&self, id: String, name: String) -> Result<(), FfiError> {
        let id = uuid(&id)?;
        self.inner.write().rename_controller(id, &name);
        Ok(())
    }

    pub fn delete_controllers(&self, rows: Vec<String>) {
        let mut s = self.inner.write();
        let slots = s.tree_selection(&rows).controllers;
        s.delete_controllers(Some(&self.core), &slots);
    }

    pub fn set_output_count(&self, id: String, count: u32) -> Result<(), FfiError> {
        let id = uuid(&id)?;
        self.inner.write().set_output_count(id, count);
        Ok(())
    }

    /// F94, at the one selected output row.
    pub fn insert_outputs(&self, output_row: String, count: u32) {
        let Some((slot, index)) = parse_output_row(&output_row) else { return };
        self.inner.write().insert_outputs(slot, index, count);
    }

    /// F95.
    pub fn remove_selected_outputs(&self, rows: Vec<String>) -> bool {
        self.inner.write().remove_selected_outputs(&rows)
    }

    pub fn move_controllers(&self, rows: Vec<String>, up: bool) {
        let mut s = self.inner.write();
        let slots = s.tree_selection(&rows).controllers;
        s.move_controllers(&slots, up);
    }

    /// F79.
    pub fn drop_controllers(&self, rows: Vec<String>, index: u32) {
        let mut s = self.inner.write();
        let slots = s.tree_selection(&rows).controllers;
        s.drop_controllers(&slots, index);
    }

    pub fn start_controllers(&self, rows: Vec<String>) {
        let mut s = self.inner.write();
        let slots = s.tree_selection(&rows).controllers;
        s.start_controllers(Some(&self.core), &slots);
    }

    pub fn stop_controllers(&self, rows: Vec<String>) {
        let mut s = self.inner.write();
        let slots = s.tree_selection(&rows).controllers;
        s.stop_controllers(Some(&self.core), &slots);
    }

    /// Whether these rows hold only root rows, the only ones that drag (F79).
    pub fn rows_are_controllers(&self, rows: Vec<String>) -> bool {
        !rows.is_empty() && rows.iter().all(|r| r.starts_with('c'))
    }

    // ---- Graphical view ------------------------------------------------------------------------------------------------------

    /// F112, F115, F116. Off the main thread.
    pub fn graph_scene(&self, selection: Vec<String>, rows: Vec<String>, width: i32) -> SetupGraphScene {
        let s = self.inner.read();
        let outputs = s.output_selection(&rows);
        s.graph_scene(&selection, &outputs, width).into()
    }

    pub fn graph_revision(&self) -> u64 {
        self.inner.read().graph_revision()
    }

    pub fn graph_toggle_collapsed(&self, shape: u64) {
        self.inner.write().graph_toggle_collapsed(shape);
    }

    pub fn graph_place_filters(&self, placed: Vec<SetupFilterPlacement>) {
        let list: Vec<(Cid, Rect)> = placed.iter().map(|p| (Cid(p.component), Rect { x: p.x, y: p.y, w: p.w, h: p.h })).collect();
        self.inner.write().graph_place_filters(&list);
    }

    pub fn graph_connect_candidate(&self, source: u32, target: u32) -> bool {
        self.inner.read().graph_connect_candidate(Cid(source), Cid(target))
    }

    pub fn graph_connect(&self, target: u32, source: u32, output: u32) -> bool {
        self.inner.write().graph_connect(Cid(target), Cid(source), output)
    }

    pub fn graph_delete(&self, destinations: Vec<u32>, filters: Vec<u32>) {
        let d: Vec<Cid> = destinations.into_iter().map(Cid).collect();
        let f: Vec<Cid> = filters.into_iter().map(Cid).collect();
        self.inner.write().graph_delete(&d, &f);
    }

    /// F121. Returns the new filter's component.
    pub fn graph_add_filter(&self, type_id: String, x: f32, y: f32) -> Result<Option<u32>, FfiError> {
        let id = uuid(&type_id)?;
        let rect = Rect { x, y, w: 160.0, h: 20.0 };
        Ok(self.inner.write().graph_add_filter(id, rect).map(|c| c.0))
    }

    /// F122, F123.
    pub fn graph_paste_filters(&self, sources: Vec<u32>, copies: u32, x: f32, y: f32) -> Vec<u32> {
        let list: Vec<Cid> = sources.into_iter().map(Cid).collect();
        self.inner.write().graph_paste_filters(&list, copies, (x, y)).into_iter().map(|c| c.0).collect()
    }

    /// A filter's module type, for its dialog's placeholder. `None` if the component isn't a filter.
    pub fn filter_type_name(&self, component: u32) -> Option<String> {
        self.inner.read().filter_type_name(Cid(component)).map(str::to_string)
    }
}

fn parse_output_row(row: &str) -> Option<(u32, u32)> {
    let (slot, index) = row.strip_prefix('o')?.split_once(':')?;
    Some((slot.parse().ok()?, index.parse().ok()?))
}

// ---- Free functions -------------------------------------------------------------------------------------------------

/// The "Add:" list, in order. "Single Item" is the default selection.
#[uniffi::export]
pub fn setup_element_templates() -> Vec<SetupTemplateInfo> {
    ALL_TEMPLATES
        .iter()
        .map(|(kind, name)| {
            let (ask_dimming, run_color) = template_needs_followups(*kind);
            SetupTemplateInfo { kind: (*kind).into(), name: (*name).to_string(), ask_dimming, run_color }
        })
        .collect()
}

/// The form's starting values. `None` for Intelligent Fixture, which has a wizard.
#[uniffi::export]
pub fn setup_template_draft(kind: SetupTemplateKind) -> Option<SetupTemplateDraft> {
    template_defaults(kind.into()).map(|p| draft_of(kind, &p))
}

/// What the dialog's OK checks. `None` means it can close.
#[uniffi::export]
pub fn setup_template_problem(draft: SetupTemplateDraft) -> Option<SetupTemplateProblem> {
    match validate_template(&params_of(&draft)) {
        Err(TemplateError::BadPattern) => {
            Some(SetupTemplateProblem { title: BAD_PATTERN_TITLE.to_string(), message: BAD_PATTERN_MESSAGE.to_string() })
        }
        _ => None,
    }
}

/// The read-only number beside "Zig Zag Every".
#[uniffi::export]
pub fn setup_zig_zag_every(draft: SetupTemplateDraft) -> u32 {
    zig_zag_every(&params_of(&draft))
}

/// `(type id, TypeName)` for Add Property, sorted by name.
#[uniffi::export]
pub fn setup_property_types() -> Vec<SetupModuleType> {
    PROPERTY_DESCRIPTORS.iter().map(|d| SetupModuleType { id: d.type_id.to_string(), name: d.type_name.to_string() }).collect()
}

/// `(type id, TypeName)` for the controllers' "Add:" list, sorted by name.
#[uniffi::export]
pub fn setup_controller_types() -> Vec<SetupModuleType> {
    controller_types().into_iter().map(|(id, name)| SetupModuleType { id: id.to_string(), name: name.to_string() }).collect()
}

/// `(type id, TypeName)` for Add Filter, sorted by name.
#[uniffi::export]
pub fn setup_filter_types() -> Vec<SetupModuleType> {
    SetupSession::filter_types().into_iter().map(|(id, name)| SetupModuleType { id: id.to_string(), name: name.to_string() }).collect()
}

/// The Configure combo's helpers, in their fixed order (sibling spec F4).
#[uniffi::export]
pub fn setup_helper_names() -> Vec<String> {
    ["Color Handling", "Dimming Curve", "Face Mapping", "Orientation", "Patching Order", "State Mapping"].iter().map(|s| (*s).to_string()).collect()
}

#[uniffi::export]
pub fn setup_output_limit(type_id: String) -> u32 {
    Uuid::parse_str(&type_id).map_or(i32::MAX as u32, |id| output_limit(&id))
}

/// How many outputs a page row holds.
#[uniffi::export]
pub fn setup_output_page_size() -> u32 {
    OUTPUT_PAGE_SIZE
}

/// Above this many, unpatching asks first.
#[uniffi::export]
pub fn setup_unpatch_prompt_threshold() -> u32 {
    UNPATCH_PROMPT_THRESHOLD
}

/// F68, from the two sets of numbers on screen.
#[uniffi::export]
pub fn setup_patch_summary(elements: SetupElementStats, controllers: SetupControllerStats, options: SetupPatchOptions) -> SetupPatchSummary {
    patch_summary(&(&elements).into(), &(&controllers).into(), &options.into()).into()
}

/// Selection order within a range gesture (F26, F71).
#[uniffi::export]
pub fn setup_ordered_rows(selected: Vec<u32>, anchor: Option<u32>, endpoint: Option<u32>) -> Vec<u32> {
    ordered_rows(&selected, anchor, endpoint)
}

#[uniffi::export]
pub fn setup_port_fraction(index: u32, count: u32) -> f32 {
    port_fraction(index, count)
}

#[uniffi::export]
pub fn setup_stub_length() -> f32 {
    STUB_LENGTH
}

#[uniffi::export]
pub fn setup_zoom_to_fit(view_width: f32, view_height: f32, content_width: f32, content_height: f32) -> f32 {
    zoom_to_fit(view_width, view_height, content_width, content_height)
}

/// In whole percent.
#[uniffi::export]
pub fn setup_zoom_step(level: i32, zoom_in: bool) -> i32 {
    zoom_step(level, zoom_in)
}

// ---- The property, filter and controller dialogs ---------------------------------------------------------------------

#[cfg(feature = "dialogs")]
struct SetupDialogWorld(Arc<DisplaySetupSession>);

#[cfg(feature = "dialogs")]
impl vixen_session::dialogs::world::WorldProvider for SetupDialogWorld {
    fn with_world(&self, f: &mut dyn FnMut(&mut dyn vixen_session::dialogs::world::DialogWorld)) {
        f(&mut vixen_session::dialogs::setup_world::SetupWorld(&mut self.0.inner.write()));
    }
}

#[cfg(feature = "dialogs")]
#[uniffi::export]
impl DisplaySetupSession {
    /// What to pass the dialogs in `VixenDialogs`: what they do goes to this working copy, so the window's Cancel drops it.
    pub fn dialog_context(self: Arc<Self>) -> Arc<crate::dialogs::DialogContext> {
        crate::dialogs::DialogContext::with_provider(Arc::clone(&self.core), Arc::new(SetupDialogWorld(Arc::clone(&self))))
    }
}
