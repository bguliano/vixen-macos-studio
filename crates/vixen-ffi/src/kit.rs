//! The Rust side of VixenKit's shared controls: the curve and gradient strips, the Curve and Color Gradient Editor dialogs, the number field, the discrete colour
//! picker's size and the libraries. VixenKit is compiled in every build, so this module is too, whichever areas a build has. Nothing here depends on a sequence.

use crate::core::VixenCore;
use parking_lot::Mutex;
use std::sync::Arc;
use vixen_core::color::Xyz;
use vixen_core::gradient::{AlphaPoint, ColorPoint};
use vixen_core::{Color, ColorGradient, Curve};
use vixen_session::effect_editor::curve_strip::{self, CurveGesture};
use vixen_session::effect_editor::dialogs::{self, CurveEditSession, GradientEditSession};
use vixen_session::effect_editor::drag::{self, DragPayload};
use vixen_session::effect_editor::gradient_strip::{self, StripBand};
use vixen_session::effect_editor::libraries;
use vixen_session::effect_editor::numeric::{self, DoubleField, IntField, NumberDrag, NumberField};
use vixen_session::effect_editor::{Mods, StripCursor};

/// The modifier keys, read from the event. ⌘ is upstream's Ctrl and ⌥ its Alt.
#[derive(uniffi::Record, Copy, Clone, Default)]
pub struct KitMods {
    pub shift: bool,
    pub command: bool,
    pub option: bool,
}

impl From<KitMods> for Mods {
    fn from(m: KitMods) -> Mods {
        Mods { shift: m.shift, command: m.command, option: m.option }
    }
}

/// The cursor a control asks for.
#[derive(uniffi::Enum, Copy, Clone, PartialEq, Eq)]
pub enum KitCursor {
    Arrow,
    Cross,
    ResizeHorizontal,
    ResizeVertical,
    Hand,
}

impl From<StripCursor> for KitCursor {
    fn from(c: StripCursor) -> KitCursor {
        match c {
            StripCursor::Arrow => KitCursor::Arrow,
            StripCursor::Cross => KitCursor::Cross,
            StripCursor::ResizeHorizontal => KitCursor::ResizeHorizontal,
            StripCursor::ResizeVertical => KitCursor::ResizeVertical,
            StripCursor::Hand => KitCursor::Hand,
        }
    }
}

/// A point of a curve, in 0..100 on each axis. Also used for points to draw.
#[derive(uniffi::Record, Copy, Clone, PartialEq)]
pub struct KitPoint {
    pub x: f64,
    pub y: f64,
}

/// A curve as data: `Curve`, whole.
#[derive(uniffi::Record, Clone, PartialEq)]
pub struct CurveValue {
    pub points: Vec<KitPoint>,
    /// `LibraryReferenceName`. Not empty means linked: the row can't be edited inline, and the badge shows.
    pub library_name: String,
    pub is_current_library: bool,
}

impl From<&Curve> for CurveValue {
    fn from(c: &Curve) -> CurveValue {
        CurveValue {
            points: c.points.iter().map(|p| KitPoint { x: p.0, y: p.1 }).collect(),
            library_name: c.library_reference_name.clone(),
            is_current_library: c.is_current_library_curve,
        }
    }
}

impl From<&CurveValue> for Curve {
    fn from(v: &CurveValue) -> Curve {
        let mut c = Curve::from_points(v.points.iter().map(|p| (p.x, p.y)).collect());
        c.library_reference_name = v.library_name.clone();
        c.is_current_library_curve = v.is_current_library;
        c
    }
}

/// A gradient's colour point. The colour is kept as XYZ, as `ColorPoint` stores it, so a value survives the trip exactly.
#[derive(uniffi::Record, Copy, Clone, PartialEq)]
pub struct GradientStop {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub position: f64,
    pub focus: f64,
    /// The colour as ARGB, for drawing.
    pub argb: u32,
}

#[derive(uniffi::Record, Copy, Clone, PartialEq)]
pub struct GradientAlpha {
    pub alpha: f64,
    pub position: f64,
    pub focus: f64,
}

/// A gradient as data: `ColorGradient`, whole.
#[derive(uniffi::Record, Clone, PartialEq)]
pub struct GradientValue {
    pub stops: Vec<GradientStop>,
    pub alphas: Vec<GradientAlpha>,
    pub gammacorrected: bool,
    pub title: Option<String>,
    pub library_name: String,
    pub is_current_library: bool,
}

impl From<&ColorGradient> for GradientValue {
    fn from(g: &ColorGradient) -> GradientValue {
        GradientValue {
            stops: g
                .colors()
                .iter()
                .map(|p| GradientStop {
                    x: p.color.x,
                    y: p.color.y,
                    z: p.color.z,
                    position: p.position,
                    focus: p.focus,
                    argb: p.rgb_color().to_argb_u32(),
                })
                .collect(),
            alphas: g.alphas().iter().map(|a| GradientAlpha { alpha: a.alpha, position: a.position, focus: a.focus }).collect(),
            gammacorrected: g.gammacorrected,
            title: g.title.clone(),
            library_name: g.library_reference_name.clone(),
            is_current_library: g.is_current_library_gradient,
        }
    }
}

impl From<&GradientValue> for ColorGradient {
    fn from(v: &GradientValue) -> ColorGradient {
        let colors = v.stops.iter().map(|s| ColorPoint::from_xyz(Xyz::new(s.x, s.y, s.z), s.focus, s.position)).collect();
        let alphas = v.alphas.iter().map(|a| AlphaPoint { alpha: a.alpha, position: a.position, focus: a.focus }).collect();
        let mut g = ColorGradient::from_parts(colors, alphas);
        g.gammacorrected = v.gammacorrected;
        g.title = v.title.clone();
        g.library_reference_name = v.library_name.clone();
        g.is_current_library_gradient = v.is_current_library;
        g
    }
}

/// `new Curve()`: a ramp.
#[uniffi::export]
pub fn default_curve_value() -> CurveValue {
    (&Curve::default()).into()
}

/// `new Curve(CurveType.Flat100)`: what a new gradient and level pair starts with.
#[uniffi::export]
pub fn flat100_curve_value() -> CurveValue {
    (&Curve::flat100()).into()
}

/// `new ColorGradient()`: one white stop.
#[uniffi::export]
pub fn default_gradient_value() -> GradientValue {
    (&ColorGradient::default()).into()
}

/// `new ColorGradient(color)`: one stop at 0.
#[uniffi::export]
pub fn solid_gradient_value(argb: u32) -> GradientValue {
    (&ColorGradient::solid(Color::from_argb_u32(argb))).into()
}

// Curve strips: F38 to F43.

/// `TransformPoint`: what to draw in `width` by `height`, y downwards. Call once per value or size, not per draw.
#[uniffi::export]
pub fn curve_strip_points(points: Vec<KitPoint>, width: f64, height: f64) -> Vec<KitPoint> {
    let pairs: Vec<(f64, f64)> = points.iter().map(|p| (p.x, p.y)).collect();
    curve_strip::curve_strip_points(&pairs, width, height).into_iter().map(|p| KitPoint { x: p.0 as f64, y: p.1 as f64 }).collect()
}

/// The cursor with no button down.
#[uniffi::export]
pub fn curve_strip_hover(curve: CurveValue, x: f64, y: f64, width: f64, height: f64, mods: KitMods, over_handle: bool) -> KitCursor {
    curve_strip::curve_hover(&(&curve).into(), x, y, width, height, mods.into(), over_handle).into()
}

/// Where the level handle sits, 0..1.
#[uniffi::export]
pub fn curve_level_position(curve: CurveValue) -> f64 {
    curve_strip::curve_level_position(&(&curve).into())
}

/// Dragging the level handle: the flat curve at that position.
#[uniffi::export]
pub fn curve_level_from_handle(position: f64) -> CurveValue {
    (&curve_strip::curve_level_from_handle(position)).into()
}

#[uniffi::export]
pub fn curve_level_tooltip(position: f64) -> String {
    curve_strip::curve_level_tooltip(position)
}

/// What to draw during a press.
#[derive(uniffi::Record, Clone)]
pub struct CurveStripPreview {
    pub points: Vec<KitPoint>,
    pub tooltip: Option<String>,
}

/// One press on a curve strip. Held by the view until the release.
#[derive(uniffi::Object)]
pub struct CurveStripGesture {
    inner: Mutex<Option<CurveGesture>>,
}

/// Mouse down on a strip. `nil` for a linked curve, which ignores every gesture, so any drag on it is a value drag.
#[uniffi::export]
pub fn curve_strip_press(curve: CurveValue, x: f64, y: f64, width: f64, height: f64, mods: KitMods) -> Option<Arc<CurveStripGesture>> {
    curve_strip::curve_mouse_down(&(&curve).into(), x, y, width, height, mods.into())
        .map(|g| Arc::new(CurveStripGesture { inner: Mutex::new(Some(g)) }))
}

#[uniffi::export]
impl CurveStripGesture {
    /// Whether the press was taken. When it wasn't, a drag past the system distance is a value drag.
    pub fn captured(&self) -> bool {
        self.inner.lock().as_ref().is_some_and(CurveGesture::captured)
    }

    /// `nil` means nothing changed.
    pub fn mouse_move(&self, x: f64, y: f64, mods: KitMods) -> Option<CurveStripPreview> {
        let mut inner = self.inner.lock();
        let preview = inner.as_mut()?.mouse_move(x, y, mods.into())?;
        Some(CurveStripPreview {
            points: preview.points.iter().map(|p| KitPoint { x: p.0, y: p.1 }).collect(),
            tooltip: preview.tooltip,
        })
    }

    /// The release: what to commit, if anything.
    pub fn mouse_up(&self, mods: KitMods) -> Option<CurveValue> {
        let gesture = self.inner.lock().take()?;
        gesture.mouse_up(mods.into()).map(|c| (&c).into())
    }
}

// Gradient strips: F44 to F50.

/// A blend across the strip's width. Parallel arrays, positions ascending from 0 to 1.
#[derive(uniffi::Record, Clone, PartialEq)]
pub struct GradientBand {
    pub colors: Vec<u32>,
    pub positions: Vec<f32>,
}

impl From<&StripBand> for GradientBand {
    fn from(b: &StripBand) -> GradientBand {
        GradientBand { colors: b.colors.iter().map(|c| c.to_argb_u32()).collect(), positions: b.positions.clone() }
    }
}

#[derive(uniffi::Record, Clone, PartialEq)]
pub struct GradientHandleInfo {
    pub position: f64,
    pub stops: Vec<u32>,
    pub argb: u32,
}

/// What a strip draws for a value. Call once per value, not per draw.
#[uniffi::export]
pub fn gradient_strip_bands(value: GradientValue, discrete: bool) -> Vec<GradientBand> {
    gradient_strip::gradient_strip_bands(&(&value).into(), discrete).iter().map(GradientBand::from).collect()
}

/// One handle per distinct stop position.
#[uniffi::export]
pub fn gradient_strip_handles(value: GradientValue) -> Vec<GradientHandleInfo> {
    gradient_strip::gradient_handles(&(&value).into())
        .into_iter()
        .map(|h| GradientHandleInfo { position: h.position, stops: h.stops, argb: h.color.to_argb_u32() })
        .collect()
}

fn colors_of(argb: &[u32]) -> Vec<Color> {
    argb.iter().map(|c| Color::from_argb_u32(*c)).collect()
}

/// F45: a click on an empty part of the band.
#[uniffi::export]
pub fn gradient_add_stop(value: GradientValue, position: f64, discrete: bool, valid_colors: Vec<u32>) -> GradientValue {
    (&gradient_strip::gradient_add_stop(&(&value).into(), position, discrete, &colors_of(&valid_colors))).into()
}

/// F46: every stop under a handle moves.
#[uniffi::export]
pub fn gradient_move_handle(value: GradientValue, stops: Vec<u32>, position: f64) -> GradientValue {
    (&gradient_strip::gradient_move_handle(&(&value).into(), &stops, position)).into()
}

/// F47.
#[uniffi::export]
pub fn gradient_delete_handle(value: GradientValue, stops: Vec<u32>) -> GradientValue {
    (&gradient_strip::gradient_delete_handle(&(&value).into(), &stops)).into()
}

/// F48, discrete: none ticked deletes, some replace the handle's stops with one per colour.
#[uniffi::export]
pub fn gradient_set_handle_colors(value: GradientValue, stops: Vec<u32>, colors: Vec<u32>) -> GradientValue {
    (&gradient_strip::gradient_set_handle_colors(&(&value).into(), &stops, &colors_of(&colors))).into()
}

/// F48, not discrete: one stop's colour.
#[uniffi::export]
pub fn gradient_set_stop_color(value: GradientValue, stop: u32, argb: u32) -> GradientValue {
    (&gradient_strip::gradient_set_stop_color(&(&value).into(), stop, Color::from_argb_u32(argb))).into()
}

/// F49: `nil` when it wouldn't reverse.
#[uniffi::export]
pub fn gradient_reversed(value: GradientValue) -> Option<GradientValue> {
    gradient_strip::gradient_reverse(&(&value).into()).map(|g| (&g).into())
}

/// `Color.GetBrightness`-free colour text: `R, G, B`, with alpha first when it isn't opaque.
#[uniffi::export]
pub fn kit_color_text(argb: u32) -> String {
    vixen_session::effect_editor::values::color_text(Color::from_argb_u32(argb))
}

// Value drags: F67, F69.

#[uniffi::export]
pub fn curve_drag_text(value: CurveValue) -> String {
    DragPayload::Curve((&value).into()).encode()
}

#[uniffi::export]
pub fn gradient_drag_text(value: GradientValue) -> String {
    DragPayload::Gradient((&value).into()).encode()
}

#[uniffi::export]
pub fn color_drag_text(argb: u32) -> String {
    DragPayload::Color(Color::from_argb_u32(argb)).encode()
}

#[uniffi::export]
pub fn curve_from_drag_text(text: String) -> Option<CurveValue> {
    match DragPayload::decode(&text) {
        Some(DragPayload::Curve(c)) => Some((&c).into()),
        _ => None,
    }
}

#[uniffi::export]
pub fn gradient_from_drag_text(text: String) -> Option<GradientValue> {
    match DragPayload::decode(&text) {
        Some(DragPayload::Gradient(g)) => Some((&g).into()),
        _ => None,
    }
}

#[uniffi::export]
pub fn color_from_drag_text(text: String) -> Option<u32> {
    match DragPayload::decode(&text) {
        Some(DragPayload::Color(c)) => Some(c.to_argb_u32()),
        _ => None,
    }
}

/// `LibraryDragPayloadFactory.Create`, for dragging a library entry out: a copy, not current, linked only when asked.
#[uniffi::export]
pub fn library_curve_drag_text(value: CurveValue, name: String, link: bool) -> String {
    drag::library_curve_payload(&(&value).into(), &name, link).encode()
}

#[uniffi::export]
pub fn library_gradient_drag_text(value: GradientValue, name: String, link: bool) -> String {
    drag::library_gradient_payload(&(&value).into(), &name, link).encode()
}

// Number fields: F24, F25.

/// A number field's limits. `min` and `max` are text so a 64-bit value keeps its digits. A decimal field has `is_decimal`, and rounds to `precision` places.
#[derive(uniffi::Record, Clone, PartialEq)]
pub struct NumberLimits {
    pub is_decimal: bool,
    pub min: String,
    pub max: String,
    pub step: f64,
    pub precision: u32,
}

impl NumberLimits {
    pub fn int(f: IntField) -> NumberLimits {
        NumberLimits { is_decimal: false, min: f.min.to_string(), max: f.max.to_string(), step: f.step, precision: 0 }
    }

    pub fn double(f: DoubleField) -> NumberLimits {
        NumberLimits { is_decimal: true, min: f.min.to_string(), max: f.max.to_string(), step: f.step, precision: f.precision }
    }

    fn field(&self) -> NumberField {
        if self.is_decimal {
            NumberField::Double(DoubleField {
                min: self.min.parse().unwrap_or(0.0),
                max: self.max.parse().unwrap_or(f64::MAX),
                step: self.step,
                precision: self.precision,
            })
        } else {
            NumberField::Int(IntField {
                min: self.min.parse().unwrap_or(0),
                max: self.max.parse().unwrap_or(i32::MAX as i128),
                step: self.step,
            })
        }
    }
}

/// The text a field shows after this is typed in, or `nil` to revert: an optional sign and surrounding spaces, no decimals for a whole number, clamped to the limits.
#[uniffi::export]
pub fn number_field_normalized(limits: NumberLimits, text: String) -> Option<String> {
    limits.field().normalized(&text)
}

/// One press on a number field's drag strip.
#[derive(uniffi::Object)]
pub struct NumberStripGesture {
    inner: Mutex<NumberDrag>,
}

#[uniffi::export]
impl NumberStripGesture {
    /// Mouse down, with the value's text and the pointer in points, y downwards.
    #[uniffi::constructor]
    pub fn press(limits: NumberLimits, value_text: String, x: f64, y: f64) -> Arc<NumberStripGesture> {
        Arc::new(NumberStripGesture { inner: Mutex::new(NumberDrag::press(limits.field(), &value_text, x, y)) })
    }

    /// The new text when this move changed it. Right and up increase.
    pub fn mouse_move(&self, x: f64, y: f64, mods: KitMods) -> Option<String> {
        self.inner.lock().moved(x, y, mods.into())
    }

    /// What the timers and the release commit.
    pub fn hold_text(&self) -> String {
        self.inner.lock().hold_text().to_string()
    }

    pub fn is_dragging(&self) -> bool {
        self.inner.lock().is_dragging()
    }
}

/// `Slider.SnapToTick`.
#[uniffi::export]
pub fn slider_snap(value: f64, min: f64, max: f64, tick: f64, snap: bool) -> f64 {
    numeric::snap_to_tick(value, min, max, tick, snap)
}

/// `Slider`'s automatic tooltip while dragging.
#[uniffi::export]
pub fn slider_drag_tip(value: f64) -> String {
    numeric::slider_drag_tip(value)
}

// Discrete colour pickers: F101.

/// The picker's size: `(width, height)` as `[width, height]`. 92 and 3 for the multiple kind, 72 and 4 for the single one.
#[uniffi::export]
pub fn discrete_picker_size(count: u32, item_width: u32, threshold: u32) -> Vec<u32> {
    let (w, h) = dialogs::discrete_picker_size(count, item_width, threshold);
    vec![w, h]
}

// The Curve Editor dialog: F70 to F78.

/// F73: what an X or Y box holds after this was typed into it.
#[uniffi::export]
pub fn curve_coordinate_text(typed: String, before: String) -> String {
    dialogs::coordinate_text(&typed, &before)
}

/// Everything the dialog shows, after any call.
#[derive(uniffi::Record, Clone)]
pub struct CurveEditorState {
    pub curve: CurveValue,
    pub label: String,
    pub title: String,
    pub read_only: bool,
    pub can_save: bool,
    pub can_load: bool,
    pub can_unlink: bool,
    pub can_edit_library: bool,
    pub can_invert_reverse: bool,
    pub instructions_visible: bool,
    pub can_draw: bool,
    pub x_text: String,
    pub y_text: String,
    pub fields_enabled: bool,
    pub update_enabled: bool,
    pub drawing: bool,
    pub draw_interval: String,
    pub last_function: String,
}

/// The message for an error box, with its title.
#[derive(uniffi::Record, Clone)]
pub struct KitMessage {
    pub message: String,
    pub title: String,
}

/// The Curve Editor dialog's copy of a curve.
#[derive(uniffi::Object)]
pub struct CurveEditorSession {
    inner: Mutex<CurveEditSession>,
}

fn curve_state(s: &CurveEditSession) -> CurveEditorState {
    let mode = s.mode();
    CurveEditorState {
        curve: s.curve().into(),
        label: mode.label,
        title: mode.title,
        read_only: mode.read_only,
        can_save: mode.can_save,
        can_load: mode.can_load,
        can_unlink: mode.can_unlink,
        can_edit_library: mode.can_edit_library,
        can_invert_reverse: mode.can_invert_reverse,
        instructions_visible: mode.instructions_visible,
        can_draw: mode.can_draw,
        x_text: s.x_text().to_string(),
        y_text: s.y_text().to_string(),
        fields_enabled: s.fields_enabled(),
        update_enabled: s.update_enabled(),
        drawing: s.is_drawing(),
        draw_interval: s.draw_interval().to_string(),
        last_function: s.last_function().to_string(),
    }
}

#[uniffi::export]
impl CurveEditorSession {
    /// `library_name` is `LibraryCurveName`, given when the dialog edits a library entry.
    #[uniffi::constructor]
    pub fn new(curve: CurveValue, library_name: Option<String>) -> Arc<CurveEditorSession> {
        Arc::new(CurveEditorSession { inner: Mutex::new(CurveEditSession::open(&(&curve).into(), library_name.as_deref())) })
    }

    pub fn state(&self) -> CurveEditorState {
        curve_state(&self.inner.lock())
    }

    /// In curve units; `points_per_unit_x` and `_y` turn distances into screen points for the 7 pt nearness.
    pub fn press(&self, x: f64, y: f64, mods: KitMods, points_per_unit_x: f64, points_per_unit_y: f64) -> CurveEditorState {
        let mut s = self.inner.lock();
        s.press(x, y, mods.into(), points_per_unit_x, points_per_unit_y);
        curve_state(&s)
    }

    pub fn drag(&self, x: f64, y: f64, mods: KitMods) -> CurveEditorState {
        let mut s = self.inner.lock();
        s.drag(x, y, mods.into());
        curve_state(&s)
    }

    pub fn release(&self, y: f64) -> CurveEditorState {
        let mut s = self.inner.lock();
        s.release(y);
        curve_state(&s)
    }

    pub fn hover(&self, x: f64, y: f64, points_per_unit_x: f64, points_per_unit_y: f64) -> KitCursor {
        self.inner.lock().hover(x, y, points_per_unit_x, points_per_unit_y).into()
    }

    /// Escape during a point drag. False means no drag was running.
    pub fn cancel_drag(&self) -> bool {
        self.inner.lock().cancel_drag()
    }

    /// The Update button.
    pub fn update_coordinates(&self, x_text: String, y_text: String) -> CurveEditorState {
        let mut s = self.inner.lock();
        s.update_coordinates(&x_text, &y_text);
        curve_state(&s)
    }

    pub fn reverse(&self) -> CurveEditorState {
        let mut s = self.inner.lock();
        s.reverse();
        curve_state(&s)
    }

    pub fn invert(&self) -> CurveEditorState {
        let mut s = self.inner.lock();
        s.invert();
        curve_state(&s)
    }

    pub fn draw_begin(&self) -> CurveEditorState {
        let mut s = self.inner.lock();
        s.draw_begin();
        curve_state(&s)
    }

    /// The Draw Interval box. True when it was capped at 20, for the balloon.
    pub fn set_draw_interval(&self, text: String) -> bool {
        self.inner.lock().set_draw_interval(&text)
    }

    /// The f(x) dialog's Generate. An error box's text comes back; the graph is left empty then, as upstream does.
    pub fn apply_function(&self, function: String) -> Option<KitMessage> {
        self.inner.lock().apply_function(&function).err().map(|e| KitMessage { message: e.message, title: e.title })
    }

    /// F77, with the chosen entry.
    pub fn load_from_library(&self, name: String, entry: CurveValue) -> CurveEditorState {
        let mut s = self.inner.lock();
        s.load_from_library(&name, &(&entry).into());
        curve_state(&s)
    }

    pub fn unlink(&self) -> CurveEditorState {
        let mut s = self.inner.lock();
        s.unlink();
        curve_state(&s)
    }

    /// After Edit Library Curve's OK.
    pub fn refresh_from_library(&self, entry: CurveValue) -> CurveEditorState {
        let mut s = self.inner.lock();
        s.refresh_from_library(&(&entry).into());
        curve_state(&s)
    }

    /// What OK returns.
    pub fn result(&self) -> CurveValue {
        self.inner.lock().curve().into()
    }

    // STUDIO-BEGIN curve-editor-restore-ffi: the live curve editor's Undo and Redo (B24)
    /// Undo and Redo in Studio's live curve editor: the dialog's curve becomes `curve` again.
    pub fn restore(&self, curve: CurveValue) -> CurveEditorState {
        let mut s = self.inner.lock();
        s.restore(&(&curve).into());
        curve_state(&s)
    }
    // STUDIO-END curve-editor-restore-ffi
}

// The Color Gradient Editor dialog: F87 to F92.

#[derive(uniffi::Record, Clone)]
pub struct GradientFaderInfo {
    pub position: f64,
    pub x: f64,
    pub colors: Vec<u32>,
    pub selected: bool,
}

/// Everything the dialog shows, after any call.
#[derive(uniffi::Record, Clone)]
pub struct GradientEditorState {
    pub gradient: GradientValue,
    pub bands: Vec<GradientBand>,
    pub faders: Vec<GradientFaderInfo>,
    /// The selected fader's diamond: its x and whether it's selected.
    pub diamond_x: Option<f64>,
    pub diamond_selected: bool,
    pub label: String,
    pub title: String,
    pub read_only: bool,
    pub can_save: bool,
    pub can_load: bool,
    pub can_unlink: bool,
    pub can_edit_library: bool,
    pub has_selection: bool,
    pub focus_selected: bool,
    pub swatch_argb: u32,
    pub location: i32,
    pub discrete: bool,
    pub valid_colors: Vec<u32>,
    pub selected_colors: Vec<u32>,
}

/// The Color Gradient Editor dialog's copy of a gradient.
#[derive(uniffi::Object)]
pub struct GradientEditorSession {
    inner: Mutex<GradientEditSession>,
    width: Mutex<f64>,
}

impl GradientEditorSession {
    fn state_of(&self, s: &GradientEditSession) -> GradientEditorState {
        let width = *self.width.lock();
        let mode = s.mode();
        let diamond = s.diamond(width);
        GradientEditorState {
            gradient: s.gradient().into(),
            bands: s.bands().iter().map(GradientBand::from).collect(),
            faders: s
                .faders(width)
                .into_iter()
                .map(|f| GradientFaderInfo {
                    position: f.position,
                    x: f.x,
                    colors: f.colors.iter().map(|c| c.to_argb_u32()).collect(),
                    selected: f.selected,
                })
                .collect(),
            diamond_x: diamond.map(|d| d.x),
            diamond_selected: diamond.is_some_and(|d| d.selected),
            label: mode.label,
            title: mode.title,
            read_only: mode.read_only,
            can_save: mode.can_save,
            can_load: mode.can_load,
            can_unlink: mode.can_unlink,
            can_edit_library: mode.can_edit_library,
            has_selection: s.has_selection(),
            focus_selected: s.focus_selected(),
            swatch_argb: s.swatch_color().to_argb_u32(),
            location: s.location_value(),
            discrete: s.discrete(),
            valid_colors: s.valid_colors().iter().map(|c| c.to_argb_u32()).collect(),
            selected_colors: s.selected_colors().iter().map(|c| c.to_argb_u32()).collect(),
        }
    }
}

#[uniffi::export]
impl GradientEditorSession {
    #[uniffi::constructor]
    pub fn new(gradient: GradientValue, library_name: Option<String>, discrete: bool, valid_colors: Vec<u32>) -> Arc<GradientEditorSession> {
        Arc::new(GradientEditorSession {
            inner: Mutex::new(GradientEditSession::open(&(&gradient).into(), library_name.as_deref(), discrete, &colors_of(&valid_colors))),
            width: Mutex::new(421.0),
        })
    }

    /// The bar control's width, for the faders' positions.
    pub fn state(&self, width: f64) -> GradientEditorState {
        *self.width.lock() = width;
        let s = self.inner.lock();
        self.state_of(&s)
    }

    pub fn press(&self, x: f64, y: f64, width: f64, height: f64) -> GradientEditorState {
        *self.width.lock() = width;
        let mut s = self.inner.lock();
        s.press(x, y, width, height);
        self.state_of(&s)
    }

    pub fn drag(&self, x: f64, y: f64, width: f64, height: f64) -> GradientEditorState {
        *self.width.lock() = width;
        let mut s = self.inner.lock();
        s.drag(x, y, width, height);
        self.state_of(&s)
    }

    pub fn release(&self, x: f64, y: f64, width: f64, height: f64) -> GradientEditorState {
        *self.width.lock() = width;
        let mut s = self.inner.lock();
        s.release(x, y, width, height);
        self.state_of(&s)
    }

    pub fn hover(&self, x: f64, y: f64, width: f64, height: f64) -> KitCursor {
        self.inner.lock().hover(x, y, width, height).into()
    }

    /// A double-click here opens the colour edit: it needs a fader under the pointer.
    pub fn fader_under(&self, x: f64, y: f64, width: f64, height: f64) -> bool {
        self.inner.lock().fader_under(x, y, width, height)
    }

    pub fn set_location(&self, value: i32) -> GradientEditorState {
        let mut s = self.inner.lock();
        s.set_location(value);
        self.state_of(&s)
    }

    pub fn delete_selected(&self) -> GradientEditorState {
        let mut s = self.inner.lock();
        s.delete_selected();
        self.state_of(&s)
    }

    pub fn set_selected_color(&self, argb: u32) -> GradientEditorState {
        let mut s = self.inner.lock();
        s.set_selected_color(Color::from_argb_u32(argb));
        self.state_of(&s)
    }

    /// F90, discrete.
    pub fn set_selected_colors(&self, colors: Vec<u32>) -> GradientEditorState {
        let mut s = self.inner.lock();
        s.set_selected_colors(&colors_of(&colors));
        self.state_of(&s)
    }

    pub fn load_from_library(&self, name: String, entry: GradientValue) -> GradientEditorState {
        let mut s = self.inner.lock();
        s.load_from_library(&name, &(&entry).into());
        self.state_of(&s)
    }

    pub fn unlink(&self) -> GradientEditorState {
        let mut s = self.inner.lock();
        s.unlink();
        self.state_of(&s)
    }

    pub fn refresh_from_library(&self, entry: GradientValue) -> GradientEditorState {
        let mut s = self.inner.lock();
        s.refresh_from_library(&(&entry).into());
        self.state_of(&s)
    }

    /// What OK returns.
    pub fn result(&self) -> GradientValue {
        self.inner.lock().gradient().into()
    }

    // STUDIO-BEGIN gradient-editor-restore-ffi: the live gradient editor's Undo and Redo (B27)
    /// Undo and Redo in Studio's live gradient editor: the dialog's gradient becomes `gradient` again.
    pub fn restore(&self, gradient: GradientValue) -> GradientEditorState {
        let mut s = self.inner.lock();
        s.restore(&(&gradient).into());
        self.state_of(&s)
    }
    // STUDIO-END gradient-editor-restore-ffi
}

// The libraries: what the dialogs and the sequencer's library panels share.

#[derive(uniffi::Record, Clone, PartialEq)]
pub struct LibraryCurveItem {
    pub name: String,
    pub curve: CurveValue,
}

#[derive(uniffi::Record, Clone, PartialEq)]
pub struct LibraryGradientItem {
    pub name: String,
    pub gradient: GradientValue,
}

#[uniffi::export]
impl VixenCore {
    /// Export's `PreviewStop` and `ResumePreview`: the Effect Preview stops now and stays off until this is let go. Real playback needs no call: it takes the engine by itself, and the preview comes back after it.
    pub fn hold_effect_preview(&self, held: bool) {
        self.inner.playback.hold_effect_preview(held);
    }

    /// The curve library, in its order.
    pub fn library_curves(&self) -> Vec<LibraryCurveItem> {
        self.inner.library_curves().curves.iter().map(|(name, c)| LibraryCurveItem { name: name.clone(), curve: c.into() }).collect()
    }

    /// The gradient library, in its order.
    pub fn library_gradients(&self) -> Vec<LibraryGradientItem> {
        self.inner
            .library_gradients()
            .gradients
            .iter()
            .map(|(name, g)| LibraryGradientItem { name: name.clone(), gradient: g.into() })
            .collect()
    }

    /// `AddCurveAsync`: writes or replaces an entry, updates linked effects in open sequences and saves off the main thread. True if the name was there already.
    pub fn library_add_curve(&self, name: String, curve: CurveValue) -> bool {
        let existed = self.inner.add_library_curve(&name, (&curve).into());
        libraries::save_libraries(&self.inner);
        existed
    }

    /// `AddColorGradientAsync`.
    pub fn library_add_gradient(&self, name: String, gradient: GradientValue) -> bool {
        let existed = self.inner.add_library_gradient(&name, (&gradient).into());
        libraries::save_libraries(&self.inner);
        existed
    }

    /// F82: a curve dropped on the Curve Library window and named, placed at the target item's index (`nil` for the end). The written entry is the one placed, also when it overwrote one.
    pub fn library_insert_curve(&self, name: String, curve: CurveValue, index: Option<u32>) -> bool {
        let existed = self.inner.insert_library_curve(&name, (&curve).into(), index.map(|i| i as usize));
        libraries::save_libraries(&self.inner);
        existed
    }

    pub fn library_insert_gradient(&self, name: String, gradient: GradientValue, index: Option<u32>) -> bool {
        let existed = self.inner.insert_library_gradient(&name, (&gradient).into(), index.map(|i| i as usize));
        libraries::save_libraries(&self.inner);
        existed
    }

    /// F82's reorder by a drag within the window: the selected names go to the target's index, as upstream's Move does; `nil` target for the end.
    pub fn library_move_curves(&self, selected: Vec<String>, target: Option<String>) {
        self.inner.move_library_curves(&selected, target.as_deref());
        libraries::save_libraries(&self.inner);
    }

    pub fn library_move_gradients(&self, selected: Vec<String>, target: Option<String>) {
        self.inner.move_library_gradients(&selected, target.as_deref());
        libraries::save_libraries(&self.inner);
    }

    /// Removes entries in one update. Linked effects keep their values and become independent.
    pub fn library_remove_curves(&self, names: Vec<String>) {
        self.inner.remove_library_curves(&names);
        libraries::save_libraries(&self.inner);
    }

    pub fn library_remove_gradients(&self, names: Vec<String>) {
        self.inner.remove_library_gradients(&names);
        libraries::save_libraries(&self.inner);
    }

    /// Rewrites the library in this order.
    pub fn library_reorder_curves(&self, names: Vec<String>) {
        self.inner.reorder_library_curves(&names);
        libraries::save_libraries(&self.inner);
    }

    pub fn library_reorder_gradients(&self, names: Vec<String>) {
        self.inner.reorder_library_gradients(&names);
        libraries::save_libraries(&self.inner);
    }
}
