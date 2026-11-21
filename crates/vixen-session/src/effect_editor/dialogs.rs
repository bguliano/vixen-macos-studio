//! The two editor dialogs without their windows: `CurveEditor` (F70 to F78) and `ColorGradientEditor` with `GradientEditPanel` and `GradientEdit`
//! (F87 to F92). Each session edits its own copy and hands it back on OK.

use super::function::{evaluate_curve_function_into, FunctionError};
use super::gradient_strip::{focus_position, gradient_strip_bands, set_focus_position, StripBand};
use super::{Mods, StripCursor};
use vixen_core::gradient::ColorPoint;
use vixen_core::{Color, ColorGradient, Curve};

/// `GraphPane.Default.NearestTol`, in screen points.
const NEAREST_TOLERANCE: f64 = 7.0;

/// F71: what the dialog's controls show for the curve's state.
#[derive(Clone, Debug, PartialEq)]
pub struct CurveEditMode {
    pub label: String,
    pub title: String,
    pub read_only: bool,
    pub can_save: bool,
    pub can_load: bool,
    pub can_unlink: bool,
    pub can_edit_library: bool,
    pub can_invert_reverse: bool,
    pub instructions_visible: bool,
    /// Draw Curve and f(x) Curve, off for a linked curve (FIX, F76).
    pub can_draw: bool,
}

/// The Curve Editor dialog's copy, its picked point and its gestures. Graph positions are in curve units; the pixel sizes come in for the 7 pt nearness.
#[derive(Clone, Debug)]
pub struct CurveEditSession {
    curve: Curve,
    /// `LibraryCurveName`, set when the dialog is editing a library entry.
    library_curve_name: Option<String>,
    /// `DragEditingPair`.
    picked: Option<usize>,
    x_text: String,
    y_text: String,
    fields_enabled: bool,
    update_enabled: bool,
    /// `_isEditing`: the point being dragged, where it started, and where the pointer started.
    editing: Option<(usize, (f64, f64), (f64, f64))>,
    draw_mode: bool,
    temp_x: f64,
    previous_curve_y: f64,
    draw_interval: String,
    last_function: String,
}

/// F73's `NumericTextBox`, with 0 and 100 as its limits: anything but digits and one decimal point is stripped as typed, and a keystroke that would pass 100 is undone.
pub fn coordinate_text(typed: &str, before: &str) -> String {
    let mut seen_point = false;
    let cleaned: String = typed
        .chars()
        .filter(|c| {
            if *c == '.' && !seen_point {
                seen_point = true;
                return true;
            }
            c.is_ascii_digit()
        })
        .collect();
    if cleaned.parse::<f64>().is_ok_and(|v| v > 100.0) {
        return before.to_string();
    }
    cleaned
}

/// `Convert` `ToString("0.####")`.
pub fn four_places(v: f64) -> String {
    let rounded = (v * 10_000.0).round() / 10_000.0;
    format!("{rounded}")
}

impl CurveEditSession {
    /// `new CurveEditor(curve)`, with `LibraryCurveName` when it's a library entry.
    pub fn open(curve: &Curve, library_curve_name: Option<&str>) -> CurveEditSession {
        let mut s = CurveEditSession {
            curve: curve.clone(),
            library_curve_name: library_curve_name.map(str::to_string),
            picked: None,
            x_text: String::new(),
            y_text: String::new(),
            fields_enabled: false,
            update_enabled: false,
            editing: None,
            draw_mode: false,
            temp_x: 0.0,
            previous_curve_y: 0.0,
            draw_interval: "5".to_string(),
            last_function: String::new(),
        };
        s.populate();
        s
    }

    /// `PopulateFormWithCurve`'s reset of the picked point and fields.
    fn populate(&mut self) {
        self.picked = None;
        self.x_text.clear();
        self.y_text.clear();
        self.update_enabled = false;
        self.fields_enabled = !self.curve.is_current_library_curve && !self.curve.is_library_reference();
    }

    pub fn mode(&self) -> CurveEditMode {
        let linked = self.curve.is_library_reference();
        if self.curve.is_current_library_curve {
            let (label, title) = match &self.library_curve_name {
                Some(name) => (format!("This curve is the library curve: {name}"), format!("Curve Editor: Library Curve {name}")),
                None => ("This curve is a library curve.".to_string(), "Curve Editor: Library Curve".to_string()),
            };
            return CurveEditMode {
                label,
                title,
                read_only: false,
                can_save: false,
                can_load: false,
                can_unlink: false,
                can_edit_library: false,
                can_invert_reverse: true,
                instructions_visible: true,
                can_draw: true,
            };
        }
        CurveEditMode {
            label: if linked {
                format!("This curve is linked to the library curve: {}", self.curve.library_reference_name)
            } else {
                "This curve is not linked to any in the library.".to_string()
            },
            title: "Curve Editor".to_string(),
            read_only: linked,
            can_save: !linked,
            can_load: true,
            can_unlink: linked,
            can_edit_library: linked,
            can_invert_reverse: !linked,
            instructions_visible: !linked,
            can_draw: !linked,
        }
    }

    pub fn curve(&self) -> &Curve {
        &self.curve
    }

    pub fn x_text(&self) -> &str {
        &self.x_text
    }

    pub fn y_text(&self) -> &str {
        &self.y_text
    }

    pub fn fields_enabled(&self) -> bool {
        self.fields_enabled
    }

    pub fn update_enabled(&self) -> bool {
        self.update_enabled
    }

    pub fn is_drawing(&self) -> bool {
        self.draw_mode
    }

    pub fn draw_interval(&self) -> &str {
        &self.draw_interval
    }

    pub fn last_function(&self) -> &str {
        &self.last_function
    }

    /// F76's text box: digits only, 2 characters, over 20 becomes 20. Returns true when it was capped, for the balloon.
    pub fn set_draw_interval(&mut self, text: &str) -> bool {
        let digits: String = text.chars().filter(char::is_ascii_digit).take(2).collect();
        let capped = digits.parse::<i32>().is_ok_and(|v| v > 20);
        self.draw_interval = if capped { "20".to_string() } else { digits };
        capped
    }

    /// `FindNearestPoint`, comparing in screen points.
    fn nearest(&self, x: f64, y: f64, points_per_unit_x: f64, points_per_unit_y: f64) -> Option<usize> {
        let mut best = None;
        let mut min = NEAREST_TOLERANCE * NEAREST_TOLERANCE;
        for (i, p) in self.curve.points.iter().enumerate() {
            let dx = (p.0 - x) * points_per_unit_x;
            let dy = (p.1 - y) * points_per_unit_y;
            let d = dx * dx + dy * dy;
            if d < min {
                min = d;
                best = Some(i);
            }
        }
        best
    }

    /// Crosshair within range of a point; arrow elsewhere.
    pub fn hover(&self, x: f64, y: f64, points_per_unit_x: f64, points_per_unit_y: f64) -> StripCursor {
        if !self.mode().read_only && self.nearest(x, y, points_per_unit_x, points_per_unit_y).is_some() {
            StripCursor::Cross
        } else {
            StripCursor::Arrow
        }
    }

    fn fill_fields_from_picked(&mut self) {
        if let Some(p) = self.picked.and_then(|i| self.curve.points.get(i)).copied() {
            self.x_text = four_places(p.0);
            self.y_text = four_places(p.1);
            self.fields_enabled = true;
            self.update_enabled = true;
        }
    }

    /// `PointPairList.Sort()`, keeping the picked and dragged points' identity through it.
    fn sort_tracking(&mut self) {
        let mut order: Vec<usize> = (0..self.curve.points.len()).collect();
        let points = &self.curve.points;
        order.sort_by(|a, b| points[*a].0.total_cmp(&points[*b].0));
        self.curve.points = order.iter().map(|i| points[*i]).collect();
        let remap = |old: usize| order.iter().position(|i| *i == old);
        self.picked = self.picked.and_then(remap);
        if let Some((index, start, mouse)) = self.editing {
            self.editing = remap(index).map(|i| (i, start, mouse));
        }
    }

    fn insert_sorted(&mut self, x: f64, y: f64) {
        self.curve.points.insert(0, (x, y));
        // The insert at 0 moves every index along by one.
        self.picked = self.picked.map(|i| i + 1);
        self.sort_tracking();
    }

    /// `zedGraphControl_MouseDownEvent`, then ZedGraph's own press.
    pub fn press(&mut self, x: f64, y: f64, mods: Mods, points_per_unit_x: f64, points_per_unit_y: f64) {
        if self.mode().read_only {
            return;
        }
        let mut nearest = self.nearest(x, y, points_per_unit_x, points_per_unit_y);
        if mods.command && nearest.is_none() {
            self.insert_sorted(x.clamp(0.0, 100.0), y.clamp(0.0, 100.0));
            // KEEP: upstream looks for the nearest point again for the Alt branch, so Control+Alt away from a point adds one and takes it straight back out.
            if mods.option {
                nearest = self.nearest(x, y, points_per_unit_x, points_per_unit_y);
            }
        }
        if mods.option {
            if let Some(i) = nearest {
                if self.curve.points.len() > 2 {
                    self.curve.points.remove(i);
                    self.picked = match self.picked {
                        Some(p) if p == i => None,
                        Some(p) if p > i => Some(p - 1),
                        other => other,
                    };
                    if self.picked.is_none() {
                        self.update_enabled = false;
                    }
                    self.sort_tracking();
                }
            }
        }
        self.previous_curve_y = y;
        if mods.none() && !self.draw_mode {
            if let Some(i) = nearest {
                self.picked = Some(i);
                self.editing = Some((i, self.curve.points[i], (x, y)));
            }
        }
        if !mods.shift {
            self.fill_fields_from_picked();
        }
    }

    /// `zedGraphControl_PreMouseMoveEvent`, ZedGraph's edit, then `PostMouseMoveEvent`, with the left button down. Positions aren't clamped coming in.
    pub fn drag(&mut self, x: f64, y: f64, mods: Mods) {
        if self.draw_mode {
            self.draw_move(x, y);
            return;
        }
        if let Some((_, start, mouse)) = self.editing {
            // The sort comes before the move, so at the end the list can be one step out of order, as upstream's is.
            self.sort_tracking();
            let Some((index, ..)) = self.editing else { return };
            self.curve.points[index] =
                ((start.0 + x - mouse.0).clamp(0.0, 100.0), (start.1 + y - mouse.1).clamp(0.0, 100.0));
            self.picked = Some(index);
            self.fill_fields_from_picked();
            return;
        }
        // FIX, F72: upstream runs both of these on a linked curve too.
        if self.mode().read_only {
            return;
        }
        if mods.shift {
            let level = y.clamp(0.0, 100.0);
            self.curve.points = vec![(0.0, level), (100.0, level)];
            self.picked = None;
            self.update_enabled = false;
            self.y_text = four_places(level);
            self.x_text.clear();
        } else if mods.none() {
            self.move_whole_curve(y);
            self.previous_curve_y = y;
            if !mods.shift {
                self.fill_fields_from_picked();
            }
        }
    }

    /// Every point's Y moves by the change in pointer Y, stopped once any point reaches a limit in that direction.
    fn move_whole_curve(&mut self, y: f64) {
        let previous = self.previous_curve_y;
        let stopped = self.curve.points.iter().find(|p| (p.1 >= 100.0 && y > previous) || (p.1 <= 0.0 && y < previous)).copied();
        if let Some(p) = stopped {
            let adjust = if p.1 > 100.0 {
                p.1 - 100.0
            } else if p.1 < 0.0 {
                p.1
            } else {
                0.0
            };
            for q in &mut self.curve.points {
                q.1 -= adjust;
            }
        } else {
            for q in &mut self.curve.points {
                q.1 += y - previous;
            }
        }
    }

    /// Mouse up.
    pub fn release(&mut self, y: f64) {
        self.editing = None;
        if self.draw_mode {
            // Y at release isn't clamped upstream: clamp is a GUARD (F76).
            self.curve.points.insert(0, (100.0, y.clamp(0.0, 100.0)));
            self.sort_tracking();
            self.draw_mode = false;
        }
    }

    /// Escape during a point drag: the point goes back (F72).
    pub fn cancel_drag(&mut self) -> bool {
        let Some((index, start, _)) = self.editing.take() else { return false };
        if let Some(p) = self.curve.points.get_mut(index) {
            *p = start;
        }
        true
    }

    /// F73's Update: whichever field parses goes onto the picked point. No sort (KEEP).
    pub fn update_coordinates(&mut self, x_text: &str, y_text: &str) {
        self.x_text = x_text.to_string();
        self.y_text = y_text.to_string();
        let Some(index) = self.picked else { return };
        let Some(p) = self.curve.points.get_mut(index) else { return };
        if let Ok(x) = x_text.trim().parse::<f64>() {
            if (0.0..=100.0).contains(&x) {
                p.0 = x;
            }
        }
        if let Ok(y) = y_text.trim().parse::<f64>() {
            if (0.0..=100.0).contains(&y) {
                p.1 = y;
            }
        }
    }

    /// F74.
    pub fn reverse(&mut self) {
        for p in &mut self.curve.points {
            p.0 = 100.0 - p.0;
        }
        self.sort_tracking();
    }

    /// F74.
    pub fn invert(&mut self) {
        for p in &mut self.curve.points {
            p.1 = 100.0 - p.1;
        }
    }

    /// `btnDraw_Click`: clears the curve and starts drawing.
    pub fn draw_begin(&mut self) {
        self.curve.points.clear();
        self.picked = None;
        self.update_enabled = false;
        self.temp_x = 0.0;
        self.draw_mode = true;
    }

    fn draw_move(&mut self, x: f64, y: f64) {
        let threshold = self.draw_interval.parse::<f64>().unwrap_or(0.0);
        if self.curve.points.is_empty() && self.temp_x < x - 1.0 {
            self.curve.points.insert(0, (0.0, y.clamp(0.0, 100.0)));
        }
        let (x, y) = (x.clamp(0.0, 100.0), y.clamp(0.0, 100.0));
        if self.temp_x < x - threshold {
            self.curve.points.insert(0, (x, y));
            self.sort_tracking();
            self.temp_x = x;
        }
    }

    /// `btnFunctionCurve_Click`: clears first, retaining any points evaluated before a later error (F75).
    pub fn apply_function(&mut self, function: &str) -> Result<(), FunctionError> {
        self.curve.points.clear();
        self.picked = None;
        self.update_enabled = false;
        if !function.is_empty() {
            self.last_function = function.to_string();
        }
        evaluate_curve_function_into(function, &self.draw_interval, &mut self.curve.points)?;
        Ok(())
    }

    /// F77: the dialog's curve becomes a copy of the entry, linked to its name.
    pub fn load_from_library(&mut self, name: &str, entry: &Curve) {
        self.curve = entry.clone();
        self.curve.library_reference_name = name.to_string();
        self.curve.is_current_library_curve = false;
        self.populate();
    }

    /// F78's Unlink: keeps the points and loses the link.
    pub fn unlink(&mut self) {
        self.curve.library_reference_name.clear();
        self.populate();
    }

    /// F78's Edit Library Curve, after its OK: the points refresh from the entry.
    pub fn refresh_from_library(&mut self, entry: &Curve) {
        self.curve.points = entry.points.clone();
        self.populate();
    }

    // STUDIO-BEGIN curve-editor-restore: the live curve editor's Undo puts an earlier curve back (B24)
    /// Studio's Undo and Redo in the live curve editor: the dialog's curve becomes `curve` again, its link included, and
    /// any drag or drawing in progress ends.
    pub fn restore(&mut self, curve: &Curve) {
        self.curve = curve.clone();
        self.editing = None;
        self.draw_mode = false;
        self.populate();
    }
    // STUDIO-END curve-editor-restore
}

/// F92: what the dialog's Library group shows for the gradient's state.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientEditMode {
    pub label: String,
    pub title: String,
    pub read_only: bool,
    pub can_save: bool,
    pub can_load: bool,
    pub can_unlink: bool,
    pub can_edit_library: bool,
}

/// One fader under the bar: a distinct position, its colours, and whether it's selected.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientFader {
    pub position: f64,
    /// `PosToRectangle`'s x.
    pub x: f64,
    pub colors: Vec<Color>,
    pub selected: bool,
}

/// The diamond of the selected fader.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GradientDiamond {
    pub x: f64,
    pub selected: bool,
}

/// The dialog's gradient copy and selection. Positions are in points against the bar control's own bounds, y downwards.
#[derive(Clone, Debug)]
pub struct GradientEditSession {
    gradient: ColorGradient,
    library_item_name: Option<String>,
    discrete: bool,
    valid_colors: Vec<Color>,
    /// Stop indexes.
    selection: Vec<usize>,
    /// `FocusSelection`: the diamond, not the fader.
    focus_selected: bool,
    offset: (f64, f64),
    pressed: bool,
}

/// `_border`.
pub const GRADIENT_BORDER: f64 = 10.0;

impl GradientEditSession {
    pub fn open(gradient: &ColorGradient, library_item_name: Option<&str>, discrete: bool, valid_colors: &[Color]) -> GradientEditSession {
        GradientEditSession {
            gradient: gradient.clone(),
            library_item_name: library_item_name.map(str::to_string),
            discrete,
            valid_colors: valid_colors.to_vec(),
            selection: Vec::new(),
            focus_selected: false,
            offset: (0.0, 0.0),
            pressed: false,
        }
    }

    pub fn gradient(&self) -> &ColorGradient {
        &self.gradient
    }

    pub fn discrete(&self) -> bool {
        self.discrete
    }

    pub fn valid_colors(&self) -> &[Color] {
        &self.valid_colors
    }

    pub fn mode(&self) -> GradientEditMode {
        if self.gradient.is_current_library_gradient {
            let (label, title) = match &self.library_item_name {
                Some(name) => (
                    format!("This gradient is the library gradient: {name}"),
                    format!("Color Gradient Editor: Library Gradient {name}"),
                ),
                None => ("This gradient is a library gradient.".to_string(), "Color Gradient Editor: Library Gradient".to_string()),
            };
            return GradientEditMode { label, title, read_only: false, can_save: false, can_load: false, can_unlink: false, can_edit_library: false };
        }
        let linked = self.gradient.is_library_reference();
        GradientEditMode {
            label: if linked {
                format!("This gradient is linked to the library: {}", self.gradient.library_reference_name)
            } else {
                "This gradient is not linked to any in the library.".to_string()
            },
            title: "Color Gradient Editor".to_string(),
            read_only: linked,
            can_save: !linked,
            can_load: true,
            can_unlink: linked,
            can_edit_library: linked,
        }
    }

    pub fn bands(&self) -> Vec<StripBand> {
        gradient_strip_bands(&self.gradient, self.discrete)
    }

    /// `PosToRectangle(pos).X`.
    fn fader_x(position: f64, width: f64) -> f64 {
        (position * (width - GRADIENT_BORDER * 2.0 - 1.0)).trunc()
    }

    /// One fader per distinct position, in sorted order.
    pub fn faders(&self, width: f64) -> Vec<GradientFader> {
        let colors = self.gradient.colors();
        let mut order: Vec<usize> = (0..colors.len()).collect();
        order.sort_by(|a, b| colors[*a].position.total_cmp(&colors[*b].position));
        let mut out: Vec<GradientFader> = Vec::new();
        for i in order {
            let p = &colors[i];
            let selected = self.selection.contains(&i) && !self.focus_selected;
            match out.last_mut() {
                Some(last) if last.position == p.position => {
                    last.colors.push(p.rgb_color());
                    last.selected |= selected;
                }
                _ => out.push(GradientFader {
                    position: p.position,
                    x: Self::fader_x(p.position, width),
                    colors: vec![p.rgb_color()],
                    selected,
                }),
            }
        }
        out
    }

    /// The selection's focus marker; hidden for the first stop.
    pub fn diamond(&self, width: f64) -> Option<GradientDiamond> {
        let first = *self.selection.first()?;
        let position = focus_position(&self.gradient, first)?;
        Some(GradientDiamond { x: Self::fader_x(position, width), selected: self.focus_selected })
    }

    /// `HitsSlider` with `GetFaderPolygon` or `GetDiamondPolygon`, in the control's own points.
    fn hits(position: f64, diamond: bool, x: f64, y: f64, width: f64, height: f64) -> bool {
        let left = Self::fader_x(position, width);
        let right = left + GRADIENT_BORDER * 2.0;
        let bottom = height - 1.0;
        let polygon: Vec<(f64, f64)> = if diamond {
            let m = left + GRADIENT_BORDER;
            vec![(m - 5.0, bottom - 8.0), (m, bottom - 3.0), (m + 5.0, bottom - 8.0), (m, bottom - 13.0)]
        } else {
            vec![
                (left + 1.0, bottom),
                (right - 1.0, bottom),
                (right, bottom - 1.0),
                (right, bottom - 16.0),
                (left + GRADIENT_BORDER, bottom - 20.0),
                (left, bottom - 16.0),
                (left, bottom - 1.0),
            ]
        };
        point_in_polygon(x, y, &polygon)
    }

    /// `GetFadersUnderMouse`: the selection if it's still under the pointer, then its diamond, then whichever faders are there.
    fn faders_under(&self, x: f64, y: f64, width: f64, height: f64) -> (Vec<usize>, bool) {
        let colors = self.gradient.colors();
        if !self.selection.is_empty() {
            if self.selection.iter().all(|i| colors.get(*i).is_some_and(|p| Self::hits(p.position, false, x, y, width, height))) {
                return (self.selection.clone(), false);
            }
            let all_diamond = self
                .selection
                .iter()
                .all(|i| focus_position(&self.gradient, *i).is_some_and(|f| Self::hits(f, true, x, y, width, height)));
            if all_diamond {
                return (self.selection.clone(), true);
            }
        }
        let mut order: Vec<usize> = (0..colors.len()).collect();
        order.sort_by(|a, b| colors[*a].position.total_cmp(&colors[*b].position));
        let mut target: Option<f64> = None;
        let mut out = Vec::new();
        for i in order {
            let p = colors[i].position;
            if Self::hits(p, false, x, y, width, height) {
                let wanted = *target.get_or_insert(p);
                if p == wanted {
                    out.push(i);
                }
            }
        }
        (out, false)
    }

    /// `PointToPos`.
    fn point_to_pos(x: f64, width: f64) -> f64 {
        ((x - GRADIENT_BORDER) / (width - GRADIENT_BORDER * 2.0 - 1.0).max(1.0)).clamp(0.0, 1.0)
    }

    /// `OnMouseDown`.
    pub fn press(&mut self, x: f64, y: f64, width: f64, height: f64) {
        if self.mode().read_only {
            return;
        }
        self.pressed = true;
        let (under, focus) = self.faders_under(x, y, width, height);
        self.selection = under;
        self.focus_selected = focus;
        self.offset = (0.0, 0.0);
        if let Some(first) = self.selection.first() {
            // `HitsSlider`'s offset: from the pointer to the fader's or diamond's centre line.
            let p = if focus { focus_position(&self.gradient, *first) } else { self.gradient.colors().get(*first).map(|c| c.position) };
            if let Some(p) = p {
                self.offset = (x - (Self::fader_x(p, width) + GRADIENT_BORDER), 0.0);
            }
            return;
        }
        // Below the bar's area, away from any fader: add. `area.Bottom` is `height - border`.
        if y > height - GRADIENT_BORDER {
            let pos = Self::point_to_pos(x, width);
            let mut new_points = Vec::new();
            if self.discrete {
                // Copies of the stops at the nearest position at or before the pointer, on top of them until dragged.
                let sorted = self.gradient.sorted_colors();
                let mut target = -1.0;
                let mut colors: Vec<Color> = Vec::new();
                for p in &sorted {
                    if target < 0.0 || (p.position < pos && p.position != target) {
                        target = p.position;
                        colors.clear();
                    }
                    if p.position == target {
                        colors.push(p.rgb_color());
                    }
                    if p.position > target {
                        break;
                    }
                }
                new_points.extend(colors.iter().map(|c| ColorPoint::new(*c, target)));
            } else {
                new_points.push(ColorPoint::new(self.gradient.get_color_at_f32(pos as f32), pos));
            }
            let mut points = self.gradient.colors().to_vec();
            let start = points.len();
            points.extend(new_points);
            self.selection = (start..points.len()).collect();
            self.gradient.set_colors(points);
        }
    }

    /// `UpdateSelection`, on each move and on release. A pointer outside the control is ignored, not a removal.
    pub fn drag(&mut self, x: f64, y: f64, width: f64, height: f64) {
        if !self.pressed || self.selection.is_empty() {
            return;
        }
        let pos = Self::point_to_pos(x - self.offset.0, width);
        if self.focus_selected {
            if let Some(first) = self.selection.first().copied() {
                set_focus_position(&mut self.gradient, first, pos);
            }
            return;
        }
        if !(0.0..width).contains(&x) || !(0.0..height).contains(&y) {
            return;
        }
        self.set_selection_position(pos);
    }

    fn set_selection_position(&mut self, pos: f64) {
        let mut points = self.gradient.colors().to_vec();
        for i in &self.selection {
            if let Some(p) = points.get_mut(*i) {
                p.position = pos;
            }
        }
        self.gradient.set_colors(points);
    }

    /// FIX, F88: upstream's release writes the selected stops too, so a plain click snaps them to the pointer. Only a drag moves them here.
    pub fn release(&mut self, _x: f64, _y: f64, _width: f64, _height: f64) {
        self.pressed = false;
    }

    /// The cursor with no button down.
    pub fn hover(&self, x: f64, y: f64, width: f64, height: f64) -> StripCursor {
        if self.mode().read_only {
            return StripCursor::Arrow;
        }
        if !self.faders_under(x, y, width, height).0.is_empty() {
            StripCursor::ResizeHorizontal
        } else if y > height - GRADIENT_BORDER {
            StripCursor::Hand
        } else {
            StripCursor::Arrow
        }
    }

    /// Whether a double-click here opens the colour edit: it needs a fader under the pointer.
    pub fn fader_under(&self, x: f64, y: f64, width: f64, height: f64) -> bool {
        !self.mode().read_only && !self.faders_under(x, y, width, height).0.is_empty()
    }

    pub fn has_selection(&self) -> bool {
        !self.selection.is_empty()
    }

    pub fn focus_selected(&self) -> bool {
        self.focus_selected
    }

    /// F89's swatch: the selection's first colour, `DimGray` when none or a diamond is selected.
    pub fn swatch_color(&self) -> Color {
        if self.focus_selected {
            return Color::DIM_GRAY;
        }
        self.selection.first().and_then(|i| self.gradient.colors().get(*i)).map_or(Color::DIM_GRAY, ColorPoint::rgb_color)
    }

    /// F89's Location: `(int)(pos * 100)`, or the focus when a diamond is selected.
    pub fn location_value(&self) -> i32 {
        let Some(p) = self.selection.first().and_then(|i| self.gradient.colors().get(*i)) else { return 0 };
        ((if self.focus_selected { p.focus } else { p.position }) * 100.0) as i32
    }

    /// F89's Location: `value / 100` on all selected stops' position, or their focus.
    pub fn set_location(&mut self, value: i32) {
        let v = (value as f64 / 100.0).clamp(0.0, 1.0);
        let mut points = self.gradient.colors().to_vec();
        for i in &self.selection {
            if let Some(p) = points.get_mut(*i) {
                if self.focus_selected {
                    p.focus = v;
                } else {
                    p.position = v;
                }
            }
        }
        self.gradient.set_colors(points);
    }

    /// F89's Delete and the Delete key: every selected stop. Doesn't guard the last stop (F94). Disabled with a diamond or read-only.
    pub fn delete_selected(&mut self) {
        if self.focus_selected || self.mode().read_only || self.selection.is_empty() {
            return;
        }
        let selected = std::mem::take(&mut self.selection);
        let points = self.gradient.colors().iter().enumerate().filter(|(i, _)| !selected.contains(i)).map(|(_, p)| *p).collect();
        self.gradient.set_colors(points);
    }

    /// The colours of the selection, for the discrete picker, in stop order.
    pub fn selected_colors(&self) -> Vec<Color> {
        self.selection.iter().filter_map(|i| self.gradient.colors().get(*i)).map(ColorPoint::rgb_color).collect()
    }

    /// The first selected stop's colour changes, keeping its place (F90, not discrete).
    pub fn set_selected_color(&mut self, color: Color) {
        let Some(first) = self.selection.first().copied() else { return };
        let mut points = self.gradient.colors().to_vec();
        if let Some(p) = points.get_mut(first) {
            p.color = vixen_core::color::Xyz::from_color(color);
        }
        self.gradient.set_colors(points);
    }

    /// F90, discrete: none deletes the selection, some replace it with one stop per colour at the first's position, which are then selected.
    pub fn set_selected_colors(&mut self, colors: &[Color]) {
        if colors.is_empty() {
            self.delete_selected();
            return;
        }
        let Some(position) = self.selection.first().and_then(|i| self.gradient.colors().get(*i)).map(|p| p.position) else { return };
        let selected = std::mem::take(&mut self.selection);
        let mut points: Vec<ColorPoint> =
            self.gradient.colors().iter().enumerate().filter(|(i, _)| !selected.contains(i)).map(|(_, p)| *p).collect();
        let start = points.len();
        points.extend(colors.iter().map(|c| ColorPoint::new(*c, position)));
        self.selection = (start..points.len()).collect();
        self.gradient.set_colors(points);
    }

    /// F93's Load Preset: a deep copy linked to the chosen name.
    pub fn load_from_library(&mut self, name: &str, entry: &ColorGradient) {
        self.gradient = entry.clone();
        self.gradient.library_reference_name = name.to_string();
        self.gradient.is_current_library_gradient = false;
        self.selection.clear();
        self.focus_selected = false;
    }

    /// F93's Unlink: keeps the stops and drops the link.
    pub fn unlink(&mut self) {
        self.gradient.library_reference_name.clear();
    }

    /// F93's Edit Library Gradient, after its OK.
    pub fn refresh_from_library(&mut self, entry: &ColorGradient) {
        let name = self.gradient.library_reference_name.clone();
        self.gradient = entry.clone();
        self.gradient.library_reference_name = name;
        self.gradient.is_current_library_gradient = false;
        self.selection.clear();
        self.focus_selected = false;
    }
}

fn point_in_polygon(x: f64, y: f64, polygon: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (xi, yi) = polygon[i];
        let (xj, yj) = polygon[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// F101's window size for a discrete colour picker: `(width, height)`.
pub fn discrete_picker_size(n: u32, item_width: u32, threshold: u32) -> (u32, u32) {
    let n_real = n as f64;
    let mut root = (n_real.sqrt() + 0.5).round() as i64;
    if (root - 1) * (root - 1) == n as i64 {
        root -= 1;
    }
    if n < threshold {
        root = 1;
    } else if n < 5 {
        root = 2;
    }
    let mut scroll = 0;
    if root > 8 {
        root = 8;
        scroll = 10;
    }
    let root_u = root.max(1) as u32;
    let width = (root_u * item_width + 10 + scroll).max((3 * item_width + 10).max(250));
    let delta = (root_u * root_u) as i64 - n as i64;
    let height_adjust = (delta / root_u as i64).max(0) as u32;
    let height = (root_u - height_adjust.min(root_u)) * 72 + 75;
    (width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixen_core::gradient::AlphaPoint;

    const SCALE: f64 = 5.0;

    fn session() -> CurveEditSession {
        CurveEditSession::open(&Curve::ramp_up(), None)
    }

    fn cmd() -> Mods {
        Mods { command: true, ..Mods::default() }
    }

    fn opt() -> Mods {
        Mods { option: true, ..Mods::default() }
    }

    fn shift() -> Mods {
        Mods { shift: true, ..Mods::default() }
    }

    #[test]
    fn modes_follow_the_curves_state() {
        let s = session();
        let m = s.mode();
        assert_eq!(m.label, "This curve is not linked to any in the library.");
        assert_eq!(m.title, "Curve Editor");
        assert!(m.can_save && m.can_load && !m.can_unlink && !m.can_edit_library && m.can_invert_reverse && m.instructions_visible);
        assert!(s.fields_enabled());

        let mut linked = Curve::ramp_up();
        linked.library_reference_name = "Ramp".to_string();
        let s = CurveEditSession::open(&linked, None);
        let m = s.mode();
        assert_eq!(m.label, "This curve is linked to the library curve: Ramp");
        assert!(m.read_only && !m.can_save && m.can_load && m.can_unlink && m.can_edit_library && !m.can_invert_reverse && !m.can_draw);
        assert!(!s.fields_enabled());

        let mut entry = Curve::ramp_up();
        entry.is_current_library_curve = true;
        let s = CurveEditSession::open(&entry, Some("Ramp"));
        assert_eq!(s.mode().title, "Curve Editor: Library Curve Ramp");
        assert_eq!(s.mode().label, "This curve is the library curve: Ramp");
        assert!(!s.mode().can_load && !s.mode().read_only && s.mode().can_invert_reverse);
        assert!(!s.fields_enabled());
        let s = CurveEditSession::open(&entry, None);
        assert_eq!(s.mode().title, "Curve Editor: Library Curve");
    }

    // STUDIO-BEGIN curve-editor-restore-test: Undo in the live curve editor (B24)
    #[test]
    fn restore_puts_an_earlier_curve_back_and_ends_a_drag() {
        let start = Curve::from_points(vec![(0.0, 0.0), (50.0, 50.0), (100.0, 100.0)]);
        let mut s = CurveEditSession::open(&start, None);
        s.press(50.5, 50.5, Mods::default(), SCALE, SCALE);
        s.drag(60.5, 40.5, Mods::default());
        assert_ne!(s.curve().points, start.points);
        s.restore(&start);
        assert_eq!(s.curve().points, start.points);
        assert!(!s.cancel_drag(), "the drag ended");
        assert!(!s.update_enabled() && s.x_text().is_empty() && s.fields_enabled());

        let mut linked = Curve::ramp_up();
        linked.library_reference_name = "Ramp".to_string();
        s.restore(&linked);
        assert!(s.mode().read_only && !s.fields_enabled());
        s.restore(&start);
        assert!(!s.mode().read_only && s.curve().library_reference_name.is_empty());
    }
    // STUDIO-END curve-editor-restore-test

    #[test]
    fn dragging_a_point_is_by_delta_clamped_and_resorts() {
        let mut s = CurveEditSession::open(&Curve::from_points(vec![(0.0, 0.0), (50.0, 50.0), (100.0, 100.0)]), None);
        s.press(50.5, 50.5, Mods::default(), SCALE, SCALE);
        assert_eq!((s.x_text(), s.y_text()), ("50", "50"));
        assert!(s.update_enabled());
        s.drag(60.5, 40.5, Mods::default());
        assert_eq!(s.curve().points[1], (60.0, 40.0));
        assert_eq!(s.x_text(), "60");
        // Clamped at the right.
        s.drag(150.5, 40.5, Mods::default());
        assert_eq!(s.curve().points[1], (100.0, 40.0));
        s.release(40.5);

        // Past the next point: the sort happens on the following move, and the drag stays on the same point.
        let mut s = CurveEditSession::open(&Curve::from_points(vec![(0.0, 0.0), (50.0, 50.0), (60.0, 60.0), (100.0, 100.0)]), None);
        s.press(50.0, 50.0, Mods::default(), SCALE, SCALE);
        s.drag(70.0, 50.0, Mods::default());
        assert_eq!(s.curve().points, vec![(0.0, 0.0), (70.0, 50.0), (60.0, 60.0), (100.0, 100.0)]);
        s.drag(75.0, 50.0, Mods::default());
        assert_eq!(s.curve().points, vec![(0.0, 0.0), (60.0, 60.0), (75.0, 50.0), (100.0, 100.0)]);
        assert_eq!(s.x_text(), "75");
        s.release(50.0);
    }

    #[test]
    fn escape_restores_the_dragged_point() {
        let mut s = session();
        s.press(0.0, 0.0, Mods::default(), SCALE, SCALE);
        s.drag(10.0, 10.0, Mods::default());
        assert_eq!(s.curve().points[0], (10.0, 10.0));
        assert!(s.cancel_drag());
        assert_eq!(s.curve().points[0], (0.0, 0.0));
        assert!(!s.cancel_drag());
    }

    #[test]
    fn command_adds_away_from_a_point_and_option_removes_near_one() {
        let mut s = session();
        s.press(50.0, 20.0, cmd(), SCALE, SCALE);
        assert_eq!(s.curve().points, vec![(0.0, 0.0), (50.0, 20.0), (100.0, 100.0)]);
        // Near a point: no add.
        s.press(50.5, 20.5, cmd(), SCALE, SCALE);
        assert_eq!(s.curve().points.len(), 3);
        s.press(50.0, 20.0, opt(), SCALE, SCALE);
        assert_eq!(s.curve().points, vec![(0.0, 0.0), (100.0, 100.0)]);
        // Never below two.
        s.press(0.0, 0.0, opt(), SCALE, SCALE);
        assert_eq!(s.curve().points.len(), 2);
    }

    #[test]
    fn command_option_away_from_a_point_adds_then_removes() {
        let mut s = session();
        s.press(50.0, 20.0, Mods { command: true, option: true, ..Mods::default() }, SCALE, SCALE);
        assert_eq!(s.curve().points, vec![(0.0, 0.0), (100.0, 100.0)]);
        // Outside the graph the added point is clamped out of the pointer's reach, so it stays.
        s.press(50.0, 130.0, Mods { command: true, option: true, ..Mods::default() }, SCALE, SCALE);
        assert_eq!(s.curve().points, vec![(0.0, 0.0), (50.0, 100.0), (100.0, 100.0)]);
    }

    #[test]
    fn move_whole_curve_and_flatten() {
        let mut s = CurveEditSession::open(&Curve::from_points(vec![(0.0, 20.0), (100.0, 40.0)]), None);
        s.press(50.0, 50.0, Mods::default(), SCALE, SCALE);
        s.drag(50.0, 60.0, Mods::default());
        assert_eq!(s.curve().points, vec![(0.0, 30.0), (100.0, 50.0)]);
        // Going down stops at 0: first to 0, overshoot corrected.
        s.drag(50.0, 25.0, Mods::default());
        assert_eq!(s.curve().points, vec![(0.0, -5.0), (100.0, 15.0)]);
        s.drag(50.0, 20.0, Mods::default());
        assert_eq!(s.curve().points, vec![(0.0, 0.0), (100.0, 20.0)]);
        s.drag(50.0, 80.0, shift());
        assert_eq!(s.curve().points, vec![(0.0, 80.0), (100.0, 80.0)]);
        assert_eq!((s.x_text(), s.y_text()), ("", "80"));
    }

    #[test]
    fn a_linked_curve_ignores_drags() {
        let mut linked = Curve::ramp_up();
        linked.library_reference_name = "Ramp".to_string();
        let mut s = CurveEditSession::open(&linked, None);
        s.press(50.0, 50.0, Mods::default(), SCALE, SCALE);
        s.drag(50.0, 60.0, Mods::default());
        s.drag(50.0, 60.0, shift());
        assert_eq!(s.curve().points, Curve::ramp_up().points);
    }

    #[test]
    fn update_writes_what_parses_and_does_not_sort() {
        let mut s = CurveEditSession::open(&Curve::from_points(vec![(0.0, 0.0), (50.0, 50.0), (100.0, 100.0)]), None);
        s.press(50.0, 50.0, Mods::default(), SCALE, SCALE);
        s.release(50.0);
        s.update_coordinates("120", "25.5");
        assert_eq!(s.curve().points[1], (50.0, 25.5));
        s.update_coordinates("75", "abc");
        assert_eq!(s.curve().points[1], (75.0, 25.5));
    }

    #[test]
    fn invert_reverse_draw_function() {
        let mut s = CurveEditSession::open(&Curve::from_points(vec![(0.0, 10.0), (100.0, 40.0)]), None);
        s.invert();
        assert_eq!(s.curve().points, vec![(0.0, 90.0), (100.0, 60.0)]);
        s.reverse();
        assert_eq!(s.curve().points, vec![(0.0, 60.0), (100.0, 90.0)]);

        s.draw_begin();
        assert!(s.curve().points.is_empty() && s.is_drawing());
        s.press(10.0, 10.0, Mods::default(), SCALE, SCALE);
        s.drag(10.0, 10.0, Mods::default());
        assert_eq!(s.curve().points, vec![(0.0, 10.0), (10.0, 10.0)]);
        // Under the interval of 5: nothing added.
        s.drag(14.0, 30.0, Mods::default());
        assert_eq!(s.curve().points.len(), 2);
        s.drag(16.0, 130.0, Mods::default());
        assert_eq!(s.curve().points[2], (16.0, 100.0));
        s.release(150.0);
        assert_eq!(*s.curve().points.last().unwrap(), (100.0, 100.0));
        assert!(!s.is_drawing());

        let mut s = session();
        assert!(s.apply_function("").is_err());
        assert!(s.curve().points.is_empty());
        s.apply_function("x").unwrap();
        assert_eq!(s.curve().points.len(), 21);
        assert_eq!(s.last_function(), "x");
    }

    #[test]
    fn function_runtime_failure_keeps_points_already_generated() {
        let mut s = session();
        s.set_draw_interval("20");
        let expression = "if(x < 50, x, unknown(0))";
        let error = s.apply_function(expression).unwrap_err();
        assert_eq!(error.title, "Function Errors");
        assert_eq!(s.curve().points, vec![(0.0, 0.0), (20.0, 20.0), (40.0, 40.0)]);
        assert_eq!(s.last_function(), expression);
        assert!(s.apply_function("X").is_err());
        assert!(s.curve().points.is_empty());
    }

    #[test]
    fn the_coordinate_boxes_take_numbers_up_to_a_hundred() {
        assert_eq!(coordinate_text("12a.5.6", ""), "12.56");
        assert_eq!(coordinate_text("101", "10"), "10");
        assert_eq!(coordinate_text("100", "10"), "100");
        assert_eq!(coordinate_text("", "10"), "");
    }

    #[test]
    fn the_draw_interval_box() {
        let mut s = session();
        assert_eq!(s.draw_interval(), "5");
        assert!(s.set_draw_interval("25"));
        assert_eq!(s.draw_interval(), "20");
        assert!(!s.set_draw_interval("1a"));
        assert_eq!(s.draw_interval(), "1");
        assert!(!s.set_draw_interval(""));
        assert_eq!(s.draw_interval(), "");
    }

    #[test]
    fn library_load_and_unlink() {
        let mut s = session();
        let mut entry = Curve::flat(50.0);
        entry.is_current_library_curve = true;
        s.load_from_library("Half", &entry);
        assert_eq!(s.curve().library_reference_name, "Half");
        assert!(!s.curve().is_current_library_curve);
        assert!(s.mode().read_only);
        s.unlink();
        assert_eq!(s.curve().points, Curve::flat(50.0).points);
        assert!(!s.mode().read_only);
    }

    fn gradient_session(discrete: bool) -> GradientEditSession {
        let g = ColorGradient::from_parts(
            vec![ColorPoint::new(Color::RED, 0.0), ColorPoint::new(Color::BLUE, 1.0)],
            vec![AlphaPoint::new(255, 0.0), AlphaPoint::new(255, 1.0)],
        );
        GradientEditSession::open(&g, None, discrete, &[Color::RED, Color::BLUE])
    }

    const GW: f64 = 421.0;
    const GH: f64 = 100.0;

    #[test]
    fn pressing_below_the_bar_adds_a_stop_and_drags_it() {
        let mut s = gradient_session(false);
        // Bar area width is 400, so the middle is x = 210.
        s.press(210.0, GH - 5.0, GW, GH);
        assert_eq!(s.gradient().colors().len(), 3);
        assert_eq!(s.gradient().colors()[2].position, 0.5);
        assert!(s.has_selection());
        s.drag(310.0, GH - 5.0, GW, GH);
        assert_eq!(s.gradient().colors()[2].position, 0.75);
        // Outside the control: ignored.
        s.drag(GW + 10.0, GH - 5.0, GW, GH);
        assert_eq!(s.gradient().colors()[2].position, 0.75);
        s.release(310.0, GH - 5.0, GW, GH);
        assert_eq!(s.location_value(), 75);
    }

    #[test]
    fn discrete_add_copies_the_stops_before_the_pointer() {
        let mut s = gradient_session(true);
        s.press(210.0, GH - 5.0, GW, GH);
        let colors = s.gradient().colors();
        assert_eq!(colors.len(), 3);
        assert_eq!(colors[2].position, 0.0);
        assert_eq!(colors[2].rgb_color(), Color::RED);
    }

    #[test]
    fn selecting_a_fader_and_its_diamond() {
        let mut s = gradient_session(false);
        // The second fader is at x 400..420, base at GH-1.
        s.press(410.0, GH - 5.0, GW, GH);
        assert_eq!(s.selected_colors(), vec![Color::BLUE]);
        assert_eq!(s.swatch_color(), Color::BLUE);
        assert_eq!(s.gradient().colors().len(), 2);
        s.release(410.0, GH - 5.0, GW, GH);
        let d = s.diamond(GW).unwrap();
        assert_eq!(d.x, 200.0);
        assert!(!d.selected);
        // The diamond's centre, at x 210, drawn 3 to 13 above the base.
        s.press(210.0, GH - 1.0 - 8.0, GW, GH);
        assert!(s.focus_selected());
        assert_eq!(s.swatch_color(), Color::DIM_GRAY);
        s.drag(110.0, GH - 1.0 - 8.0, GW, GH);
        assert_eq!(s.gradient().colors()[1].focus, 0.25);
        assert_eq!(s.location_value(), 25);
        s.release(110.0, GH - 1.0 - 8.0, GW, GH);
        // Delete does nothing with a diamond.
        s.delete_selected();
        assert_eq!(s.gradient().colors().len(), 2);
    }

    #[test]
    fn the_first_stop_has_no_diamond_and_delete_can_empty_it() {
        let mut s = gradient_session(false);
        s.press(10.0, GH - 5.0, GW, GH);
        assert!(s.diamond(GW).is_none());
        s.release(10.0, GH - 5.0, GW, GH);
        s.delete_selected();
        assert_eq!(s.gradient().colors().len(), 1);
        s.press(410.0, GH - 5.0, GW, GH);
        s.delete_selected();
        assert!(s.gradient().colors().is_empty());
        assert!(s.bands().len() == 1);
    }

    #[test]
    fn discrete_colour_replacement() {
        let mut s = gradient_session(true);
        s.press(410.0, GH - 5.0, GW, GH);
        s.set_selected_colors(&[Color::RED, Color::BLUE]);
        assert_eq!(s.gradient().colors().len(), 3);
        assert!(s.gradient().colors().iter().skip(1).all(|p| p.position == 1.0));
        s.set_selected_colors(&[]);
        assert_eq!(s.gradient().colors().len(), 1);
    }

    #[test]
    fn test_59_a_click_on_a_fader_with_no_drag_changes_nothing() {
        // FIX, F88: upstream's release writes the selected stops wherever the pointer is.
        let mut s = gradient_session(false);
        let start = s.gradient().clone();
        s.press(410.0, GH - 5.0, GW, GH);
        s.release(300.0, GH - 5.0, GW, GH);
        assert_eq!(s.gradient().colors(), start.colors());
        // A release far away after a press on nothing changes nothing either.
        s.press(210.0, 20.0, GW, GH);
        s.release(10.0, GH - 5.0, GW, GH);
        assert_eq!(s.gradient().colors(), start.colors());
    }

    #[test]
    fn gradient_modes() {
        let s = gradient_session(false);
        assert_eq!(s.mode().label, "This gradient is not linked to any in the library.");
        let mut g = ColorGradient::default();
        g.library_reference_name = "Lib".to_string();
        let mut s = GradientEditSession::open(&g, None, false, &[]);
        assert!(s.mode().read_only && s.mode().can_unlink && s.mode().can_edit_library);
        assert_eq!(s.mode().label, "This gradient is linked to the library: Lib");
        let before = s.gradient().colors().len();
        s.press(210.0, GH - 5.0, GW, GH);
        assert_eq!(s.gradient().colors().len(), before);
        assert_eq!(s.hover(210.0, GH - 5.0, GW, GH), StripCursor::Arrow);
        s.unlink();
        assert!(!s.mode().read_only);
        let mut g = ColorGradient::default();
        g.is_current_library_gradient = true;
        let s = GradientEditSession::open(&g, Some("L"), false, &[]);
        assert_eq!(s.mode().title, "Color Gradient Editor: Library Gradient L");
        assert!(!s.mode().read_only && !s.mode().can_save && !s.mode().can_load);
    }

    #[test]
    fn picker_sizes() {
        // Multiple, item width 92, threshold 3.
        assert_eq!(discrete_picker_size(2, 92, 3), (286, 75 + 72));
        assert_eq!(discrete_picker_size(3, 92, 3), (286, 2 * 72 + 75));
        assert_eq!(discrete_picker_size(9, 92, 3), (286, 3 * 72 + 75));
        // Past 8 columns: a scroll bar's width.
        assert_eq!(discrete_picker_size(100, 92, 3).0, 8 * 92 + 10 + 10);
        // Single, item width 72, threshold 4.
        assert_eq!(discrete_picker_size(3, 72, 4), (250, 75 + 72));
    }
}
