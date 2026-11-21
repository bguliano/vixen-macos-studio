//! Polygon Editor draft. Geometry stays in logical canvas coordinates; OK performs one ID-preserving Morph update.
//! References: PolygonEditorViewModel.cs and Morph.cs, specialized-editors F70–F93.
mod interaction;
mod morph;
pub mod studio_live; // STUDIO: the live Polygon Editor writes as it changes, one undo step at Done (B8)
#[cfg(test)]
mod tests;
pub use morph::{apply_to_sequence, morph_defaults, open_from_sequence};
use uuid::Uuid;
use vixen_effects::framework::Params;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Polygon,
    Line,
    Ellipse,
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Polygon => "Polygon",
            Self::Line => "Line",
            Self::Ellipse => "Ellipse",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Select,
    Polygon,
    Line,
    Ellipse,
    AddPoint,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MorphMode {
    Pattern,
    FreeForm,
    TimeBased,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub id: Uuid,
    pub kind: Kind,
    pub label: String,
    pub fill: String,
    pub points: Vec<Point>,
    pub angle: f64,
    pub center: Point,
    pub width: f64,
    pub height: f64,
    pub start_side: i32,
    /// A new or toggled line uses Lime; loaded/converted/pasted lines start Green.
    pub start_lime: bool,
    pub time: f64,
    pub original: Option<Params>,
}
impl Shape {
    pub fn new(kind: Kind, points: Vec<Point>, time: f64, solid: bool) -> Self {
        let mut s = Self {
            id: Uuid::new_v4(),
            kind,
            label: String::new(),
            fill: if solid { "Solid" } else { "Wipe" }.into(),
            points,
            angle: 0.,
            center: Point::default(),
            width: 0.,
            height: 0.,
            start_side: 0,
            start_lime: true,
            time,
            original: None,
        };
        s.recalculate();
        s
    }
    pub fn recalculate(&mut self) {
        let (lo, hi) = bounds(&self.points);
        self.center = Point::new((lo.x + hi.x) / 2., (lo.y + hi.y) / 2.);
        if self.kind == Kind::Ellipse && self.points.len() == 4 {
            let z = (4 - self.start_side.rem_euclid(4)) as usize % 4;
            let d = |a: Point, b: Point| (a.x - b.x).hypot(a.y - b.y);
            self.width = d(self.points[z], self.points[(z + 1) % 4]);
            self.height = d(self.points[(z + 1) % 4], self.points[(z + 2) % 4]);
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub time: f64,
    pub shape: Option<Uuid>,
}
#[derive(Clone, Debug)]
pub struct Editor {
    pub mode: Mode,
    pub morph_mode: MorphMode,
    pub shapes: Vec<Shape>,
    pub frames: Vec<Frame>,
    pub frame: usize,
    pub width: f64,
    pub height: f64,
    pub display_width: f64,
    pub display_height: f64,
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub x_scale: f64,
    pub y_scale: f64,
    pub show_display: bool,
    pub show_labels: bool,
    pub selected: Option<Uuid>,
    pub points: Vec<(Uuid, usize)>,
    pub partial: Vec<Point>,
    partial_label: Option<String>,
    pub ghost: Option<Point>,
    pub band: Option<(Point, Point)>,
    drag: Option<Drag>,
}
#[derive(Clone, Debug)]
enum Drag {
    Point(Uuid, usize),
    Band(Point),
    Move(Point),
    Handle {
        handle: u32,
        previous: Point,
        center: Point,
    },
}

pub fn bounds(points: &[Point]) -> (Point, Point) {
    if points.is_empty() {
        return (Point::default(), Point::default());
    }
    let mut lo = points[0];
    let mut hi = lo;
    for p in points {
        lo.x = lo.x.min(p.x);
        lo.y = lo.y.min(p.y);
        hi.x = hi.x.max(p.x);
        hi.y = hi.y.max(p.y)
    }
    (lo, hi)
}
pub fn rotate(p: Point, center: Point, angle: f64) -> Point {
    let (s, c) = angle.to_radians().sin_cos();
    let (x, y) = (p.x - center.x, p.y - center.y);
    Point::new(center.x + x * c - y * s, center.y + x * s + y * c)
}
impl Editor {
    pub fn new(
        mut shapes: Vec<Shape>,
        morph_mode: MorphMode,
        size: (f64, f64),
        display: (f64, f64),
        screen: (f64, f64),
    ) -> Self {
        let (width, height) = (size.0.max(1.), size.1.max(1.));
        let factor = (screen.0 * 0.7 / width)
            .min(screen.1 * 0.7 / height)
            .max(0.01);
        let (cw, ch) = (
            (width * factor).ceil().max(2.),
            (height * factor).ceil().max(2.),
        );
        let (xs, ys) = (
            (cw - 1.) / if width == 1. { 1. } else { width - 1. },
            (ch - 1.) / if height == 1. { 1. } else { height - 1. },
        );
        for s in &mut shapes {
            scale_shape(s, xs, ys);
        }
        // The source loads by kind, preserving order inside each kind (including unsorted times).
        shapes.sort_by_key(|s| match s.kind {
            Kind::Polygon => 0,
            Kind::Line => 1,
            Kind::Ellipse => 2,
        });
        let mut frames = if morph_mode == MorphMode::TimeBased {
            shapes
                .iter()
                .map(|s| Frame {
                    time: s.time,
                    shape: Some(s.id),
                })
                .collect()
        } else {
            Vec::new()
        };
        if morph_mode == MorphMode::TimeBased && frames.is_empty() {
            frames.push(Frame {
                time: 0.,
                shape: None,
            });
        }
        let mut ed = Self {
            mode: Mode::Select,
            morph_mode,
            shapes,
            frames,
            frame: 0,
            width,
            height,
            display_width: display.0,
            display_height: display.1,
            canvas_width: cw,
            canvas_height: ch,
            x_scale: xs,
            y_scale: ys,
            show_display: size != display,
            show_labels: false,
            selected: None,
            points: Vec::new(),
            partial: Vec::new(),
            partial_label: None,
            ghost: None,
            band: None,
            drag: None,
        };
        if ed.time_mode() {
            ed.select_frame(0);
        } else {
            for kind in [Kind::Polygon, Kind::Line, Kind::Ellipse] {
                let ids: Vec<_> = ed
                    .shapes
                    .iter()
                    .filter(|s| s.kind == kind)
                    .map(|s| s.id)
                    .collect();
                if ids.len() == 1 {
                    ed.select_shape(Some(ids[0]));
                }
            }
        }
        ed
    }
    pub fn time_mode(&self) -> bool {
        self.morph_mode == MorphMode::TimeBased
    }
    pub fn can_add(&self) -> bool {
        self.morph_mode != MorphMode::Pattern
            && (!self.time_mode()
                || self
                    .frames
                    .get(self.frame)
                    .is_some_and(|f| f.shape.is_none()))
    }
    pub fn visible(&self, s: &Shape) -> bool {
        !self.time_mode() || self.frames.get(self.frame).and_then(|f| f.shape) == Some(s.id)
    }
    pub fn selected_shape(&self) -> Option<&Shape> {
        self.shapes.iter().find(|s| Some(s.id) == self.selected)
    }
    pub fn selected_shape_mut(&mut self) -> Option<&mut Shape> {
        self.shapes.iter_mut().find(|s| Some(s.id) == self.selected)
    }
    pub fn select_shape(&mut self, id: Option<Uuid>) {
        self.selected = id;
        self.points = self
            .selected_shape()
            .map(|s| (0..s.points.len()).map(|i| (s.id, i)).collect())
            .unwrap_or_default();
    }
    pub fn set_mode(&mut self, mode: Mode) {
        if matches!(mode, Mode::Polygon | Mode::Line | Mode::Ellipse) && !self.can_add() {
            return;
        }
        self.mode = mode;
        self.partial.clear();
        // DrawPolygon deselects its first draft; DrawLine retains the new line selection.
        self.partial_label = (mode == Mode::Line).then(String::new);
        self.ghost = None;
        self.band = None;
        self.drag = None;
        if mode != Mode::Select {
            self.select_shape(None);
        }
    }
    pub fn label(&self) -> Option<&str> {
        self.selected_shape()
            .map(|s| s.label.as_str())
            .or(self.partial_label.as_deref())
    }
    pub fn set_label(&mut self, label: String) {
        if let Some(s) = self.selected_shape_mut() {
            s.label = label;
        } else if let Some(partial) = &mut self.partial_label {
            *partial = label;
        }
    }
    pub fn clamp(&self, p: Point) -> Point {
        Point::new(
            p.x.clamp(0., self.canvas_width - 1.),
            p.y.clamp(0., self.canvas_height - 1.),
        )
    }
    pub fn point_rows(&self) -> Vec<(usize, f64, f64, bool)> {
        self.points
            .iter()
            .filter_map(|(id, i)| {
                let s = self.shapes.iter().find(|s| s.id == *id)?;
                let p = s.points.get(*i)?;
                Some((
                    *i + 1,
                    (p.x / self.x_scale).round_ties_even() + 1.,
                    (self.height - 1. - p.y / self.y_scale).round_ties_even() + 1.,
                    s.kind != Kind::Ellipse,
                ))
            })
            .collect()
    }
    pub fn set_point(&mut self, row: usize, axis: &str, value: &str) -> Result<(), String> {
        let name = if axis == "X" { "X" } else { "Y" };
        let n = value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or_else(|| format!("Not valid {name} value."))?;
        if n < 1. {
            return Err(format!("{name} value must be greater or equal to 1."));
        }
        if n > if axis == "X" { self.width } else { self.height } {
            return Err(format!("{name} value is larger than display element."));
        }
        let Some(&(id, i)) = self.points.get(row) else {
            return Ok(());
        };
        let Some(s) = self
            .shapes
            .iter_mut()
            .find(|s| s.id == id && s.kind != Kind::Ellipse)
        else {
            return Ok(());
        };
        if let Some(p) = s.points.get_mut(i) {
            if axis == "X" {
                p.x = (n - 1.) * self.x_scale
            } else {
                p.y = (self.height - n) * self.y_scale
            };
            s.recalculate();
        }
        Ok(())
    }
    pub fn finish(&self) -> Vec<Shape> {
        let mut shapes = self.shapes.clone();
        shapes.retain(|s| match s.kind {
            Kind::Polygon => s.points.len() >= 3,
            Kind::Line => s.points.len() == 2,
            Kind::Ellipse => s.points.len() == 4,
        });
        for s in &mut shapes {
            scale_shape(s, 1. / self.x_scale, 1. / self.y_scale);
            if s.kind != Kind::Ellipse {
                for p in &mut s.points {
                    p.x = p.x.round_ties_even().clamp(0., self.width - 1.);
                    p.y = p.y.round_ties_even().clamp(0., self.height - 1.);
                }
                s.recalculate();
            }
            if self.time_mode() {
                s.time = self
                    .frames
                    .iter()
                    .find(|f| f.shape == Some(s.id))
                    .map_or(0., |f| f.time);
            } else {
                s.time = 0.;
            }
            if s.kind == Kind::Polygon && s.fill == "Wipe" && !matches!(s.points.len(), 3 | 4) {
                s.fill = "Solid".into();
            }
        }
        shapes
    }
    pub fn add_shape(&mut self, mut shape: Shape) {
        if !self.can_add() {
            return;
        }
        if shape.kind == Kind::Polygon
            && shape.fill == "Wipe"
            && !matches!(shape.points.len(), 3 | 4)
        {
            shape.fill = "Solid".into();
        }
        if self.time_mode() {
            shape.time = self.frames[self.frame].time;
            self.frames[self.frame].shape = Some(shape.id);
        }
        let id = shape.id;
        self.shapes.push(shape);
        if self.time_mode() {
            self.set_mode(Mode::Select);
            self.select_shape(Some(id));
        }
    }
    pub fn delete(&mut self) {
        if self.morph_mode != MorphMode::Pattern
            && self
                .selected_shape()
                .is_some_and(|s| s.kind != Kind::Polygon || self.points.len() == s.points.len())
        {
            let id = self.selected.take();
            self.shapes.retain(|s| Some(s.id) != id);
            for f in &mut self.frames {
                if f.shape == id {
                    f.shape = None;
                }
            }
            self.points.clear();
            return;
        }
        let selected = self.points.clone();
        for s in &mut self.shapes {
            if s.kind != Kind::Polygon {
                continue;
            }
            let mut indices: Vec<_> = selected
                .iter()
                .filter(|(id, _)| *id == s.id)
                .map(|(_, i)| *i)
                .collect();
            indices.sort_unstable();
            indices.dedup();
            for i in indices.into_iter().rev() {
                if s.points.len() > 3 && i < s.points.len() {
                    s.points.remove(i);
                }
            }
            s.recalculate();
        }
        self.select_shape(None);
    }
    pub fn toggle_start(&mut self) {
        if self.time_mode() {
            return;
        }
        if let Some(s) = self.selected_shape_mut() {
            if !s.points.is_empty() {
                s.points.rotate_left(1);
            }
            if s.kind == Kind::Ellipse {
                s.start_side = (s.start_side + 1).rem_euclid(4);
            }
            if s.kind == Kind::Line {
                s.start_lime = true;
                self.select_shape(None);
            }
        }
    }
    pub fn convert(&mut self, kind: Kind) {
        if self.mode != Mode::Select {
            return;
        }
        if self.morph_mode == MorphMode::FreeForm {
            let Some(s) = self.selected_shape() else {
                return;
            };
            if s.kind == kind {
                return;
            }
            let id = s.id;
            self.shapes.retain(|s| s.id != id);
        } else {
            if self
                .shapes
                .iter()
                .any(|s| self.visible(s) && s.kind == kind)
            {
                return;
            }
            let id = self.frames.get(self.frame).and_then(|f| f.shape);
            if self.time_mode() {
                self.shapes.retain(|s| Some(s.id) != id);
                self.frames[self.frame].shape = None;
            } else {
                self.shapes.clear();
            }
        }
        self.select_shape(None);
        let (w, h) = (self.canvas_width, self.canvas_height);
        let points = match kind {
            Kind::Polygon => vec![
                Point::new(0., 0.),
                Point::new(w - 1., 0.),
                Point::new(w - 1., h - 1.),
                Point::new(0., h - 1.),
            ],
            Kind::Line => vec![
                self.clamp(Point::new(w / 2., h * 0.2 - 1.)),
                self.clamp(Point::new(w / 2., h * 0.8 - 1.)),
            ],
            Kind::Ellipse => self.ellipse_points(Point::new(w / 2., h / 2.)),
        };
        let mut s = Shape::new(kind, points, 0., self.time_mode());
        if kind == Kind::Line {
            s.start_lime = false;
        }
        let id = s.id;
        if self.time_mode() {
            self.frames[self.frame].shape = Some(id);
        }
        self.shapes.push(s);
        if kind != Kind::Polygon {
            self.select_shape(Some(id));
        }
    }
    fn ellipse_points(&self, p: Point) -> Vec<Point> {
        let d = self.canvas_width.min(self.canvas_height) * 0.1;
        [
            (p.x - d, p.y - d),
            (p.x + d, p.y - d),
            (p.x + d, p.y + d),
            (p.x - d, p.y + d),
        ]
        .into_iter()
        .map(|(x, y)| self.clamp(Point::new(x.trunc(), y.trunc())))
        .collect()
    }
    pub fn select_frame(&mut self, index: usize) {
        if index >= self.frames.len() {
            return;
        }
        self.frame = index;
        self.set_mode(Mode::Select);
        self.select_shape(self.frames[index].shape);
    }
    pub fn add_frame(&mut self, bar_width: f64) {
        if !self.time_mode() {
            return;
        }
        let adjusted = (bar_width - 10.).max(1.);
        let time = ((self.frames[self.frame].time * adjusted).trunc() + 24.) / adjusted;
        let time = time.min(1.);
        let current = self.frames[self.frame].shape;
        let shape = self
            .shapes
            .iter()
            .find(|s| Some(s.id) == current)
            .cloned()
            .map(|mut s| {
                s.id = Uuid::new_v4();
                s.time = time;
                let id = s.id;
                self.shapes.push(s);
                id
            });
        let i = self.frame + 1;
        self.frames.insert(i, Frame { time, shape });
        self.select_frame(i);
    }
    pub fn delete_frame(&mut self) {
        if self.frames.len() <= 1 {
            return;
        }
        let f = self.frames.remove(self.frame);
        if let Some(id) = f.shape {
            self.shapes.retain(|s| s.id != id);
        }
        self.select_frame(self.frame.saturating_sub(1));
    }
    pub fn move_frame(&mut self, time: f64) {
        self.move_frame_value(time, None);
    }
    pub fn move_frame_at(&mut self, position: f64, bar_width: f64) {
        if !position.is_finite() || !bar_width.is_finite() {
            return;
        }
        let adjusted = (bar_width - 10.).max(1.);
        let pixel = position.trunc().clamp(4., (adjusted + 4.).trunc());
        self.move_frame_value((pixel - 4.) / adjusted, Some(adjusted));
    }
    fn move_frame_value(&mut self, time: f64, adjusted: Option<f64>) {
        if !self.time_mode() || !time.is_finite() {
            return;
        }
        self.frames[self.frame].time = time.clamp(0., 1.);
        let id = self.frames[self.frame].shape;
        let index = self.frame;
        let mut indexed: Vec<_> = self.frames.drain(..).enumerate().collect();
        indexed.sort_by(|a, b| {
            if let Some(width) = adjusted {
                (a.1.time * width)
                    .trunc()
                    .total_cmp(&(b.1.time * width).trunc())
            } else {
                a.1.time.total_cmp(&b.1.time)
            }
        });
        self.frame = indexed.iter().position(|(i, _)| *i == index).unwrap_or(0);
        self.frames = indexed.into_iter().map(|(_, f)| f).collect();
        if let Some(s) = self.shapes.iter_mut().find(|s| Some(s.id) == id) {
            s.time = time.clamp(0., 1.);
        }
    }
}
fn scale_shape(s: &mut Shape, x: f64, y: f64) {
    if s.kind == Kind::Ellipse {
        let center = s.center;
        for p in &mut s.points {
            let p0 = rotate(*p, center, -s.angle);
            *p = Point::new(p0.x * x, p0.y * y);
        }
        s.center = Point::new(center.x * x, center.y * y);
        for p in &mut s.points {
            *p = rotate(*p, s.center, s.angle);
        }
    } else {
        for p in &mut s.points {
            p.x *= x;
            p.y *= y;
        }
    }
    s.recalculate();
}
