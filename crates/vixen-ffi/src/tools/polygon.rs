//! One cloned Polygon Editor draft, independent of the originating sequence until OK.
use crate::{core::FfiError, sequence::SequencerSession};
use parking_lot::Mutex;
use std::sync::Arc;
use vixen_session::{
    sequence::{EffectId, SequenceSession},
    tools::polygon::{self, Editor, Kind, Mode, Point},
};

#[derive(uniffi::Record)]
pub struct PolygonEdPoint {
    pub x: f64,
    pub y: f64,
}
impl From<Point> for PolygonEdPoint {
    fn from(p: Point) -> Self {
        Self { x: p.x, y: p.y }
    }
}
#[derive(uniffi::Record)]
pub struct PolygonEdShape {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub fill: String,
    pub points: Vec<PolygonEdPoint>,
    pub selected_points: Vec<u32>,
    pub selected: bool,
    pub start_argb: u32,
    pub angle: f64,
    pub center: PolygonEdPoint,
    pub width: f64,
    pub height: f64,
}
#[derive(uniffi::Record)]
pub struct PolygonEdRow {
    pub label: u32,
    pub x: f64,
    pub y: f64,
    pub editable: bool,
}
#[derive(uniffi::Record)]
pub struct PolygonEdFrame {
    pub time: f64,
    pub selected: bool,
    pub has_shape: bool,
}
#[derive(uniffi::Record)]
pub struct PolygonEdState {
    pub width: f64,
    pub height: f64,
    pub display_x: f64,
    pub display_y: f64,
    pub display_width: f64,
    pub display_height: f64,
    pub show_display: bool,
    pub mode: String,
    pub can_add: bool,
    pub can_copy: bool,
    pub can_delete: bool,
    pub free_form: bool,
    pub time_mode: bool,
    pub show_labels: bool,
    pub label: Option<String>,
    pub shapes: Vec<PolygonEdShape>,
    pub rows: Vec<PolygonEdRow>,
    pub partial: Vec<PolygonEdPoint>,
    pub ghost: Option<PolygonEdPoint>,
    pub band: Vec<PolygonEdPoint>,
    pub handles: Vec<PolygonEdPoint>,
    pub frames: Vec<PolygonEdFrame>,
    pub selected_frame: u32,
}
#[derive(uniffi::Object)]
pub struct PolygonEditor {
    draft: Mutex<Editor>,
    target: Option<(Arc<Mutex<SequenceSession>>, Vec<EffectId>)>,
    finished: Mutex<bool>,
    live: Option<polygon::studio_live::LiveTarget>, // STUDIO: the live editor's effects from before it opened (B8)
}
mod studio_live; // STUDIO: the live Polygon Editor's commands (B8)

#[uniffi::export]
impl SequencerSession {
    pub fn open_polygon_editor(
        &self,
        effects: Vec<u64>,
        param: String,
        screen_width: f64,
        screen_height: f64,
    ) -> Result<Arc<PolygonEditor>, FfiError> {
        if param != "PolygonContainer" {
            return Err(FfiError::new(
                "The selected property is not a PolygonContainer.",
            ));
        }
        let ids: Vec<_> = effects.into_iter().map(EffectId).collect();
        let draft =
            polygon::open_from_sequence(&self.session.lock(), &ids, (screen_width, screen_height))
                .map_err(FfiError::new)?;
        let live = polygon::studio_live::LiveTarget::new(&self.session.lock(), &ids); // STUDIO: live editor (B8)
        Ok(Arc::new(PolygonEditor {
            draft: Mutex::new(draft),
            target: Some((Arc::clone(&self.session), ids)),
            finished: Mutex::new(false),
            live: Some(live), // STUDIO: live editor (B8)
        }))
    }
}
#[uniffi::export]
impl PolygonEditor {
    /// A synthetic editor for local native UI verification, with no sequence or profile mutation.
    #[uniffi::constructor]
    pub fn sample(mode: String) -> Arc<Self> {
        let mode = match mode.as_str() {
            "TimeBased" => polygon::MorphMode::TimeBased,
            "FreeForm" => polygon::MorphMode::FreeForm,
            _ => polygon::MorphMode::Pattern,
        };
        let shape = polygon::Shape::new(
            Kind::Polygon,
            vec![
                Point::new(2., 2.),
                Point::new(17., 2.),
                Point::new(17., 17.),
                Point::new(2., 17.),
            ],
            0.,
            false,
        );
        Arc::new(Self {
            draft: Mutex::new(Editor::new(
                vec![shape],
                mode,
                (20., 20.),
                (20., 20.),
                (1000., 800.),
            )),
            target: None,
            finished: Mutex::new(false),
            live: None, // STUDIO: live editor (B8)
        })
    }
    pub fn state(&self) -> PolygonEdState {
        let e = self.draft.lock();
        let dw = (e.display_width - 1.) * e.x_scale;
        let dh = (e.display_height - 1.) * e.y_scale;
        PolygonEdState {
            width: e.canvas_width,
            height: e.canvas_height,
            display_x: (e.canvas_width - dw) / 2. - 1.,
            display_y: (e.canvas_height - dh) / 2. - 1.,
            display_width: dw,
            display_height: dh,
            show_display: e.show_display,
            mode: format!("{:?}", e.mode),
            can_add: e.can_add(),
            can_copy: e.morph_mode != polygon::MorphMode::Pattern
                && e.selected_shape().is_some_and(|s| {
                    s.kind != Kind::Polygon
                        || (e.mode == Mode::Select && e.points.len() == s.points.len())
                }),
            can_delete: e.morph_mode != polygon::MorphMode::Pattern
                && e.selected_shape()
                    .is_some_and(|s| s.kind != Kind::Polygon || e.points.len() == s.points.len()),
            free_form: e.morph_mode != polygon::MorphMode::Pattern,
            time_mode: e.time_mode(),
            show_labels: e.show_labels,
            label: e.label().map(str::to_owned),
            shapes: e
                .shapes
                .iter()
                .filter(|s| e.visible(s))
                .map(|s| PolygonEdShape {
                    id: s.id.to_string(),
                    kind: s.kind.name().into(),
                    label: s.label.clone(),
                    fill: s.fill.clone(),
                    points: s.points.iter().copied().map(Into::into).collect(),
                    selected_points: e
                        .points
                        .iter()
                        .filter(|(id, _)| *id == s.id)
                        .map(|(_, i)| *i as u32)
                        .collect(),
                    selected: e.selected == Some(s.id),
                    start_argb: if s.start_lime { 0xff00ff00 } else { 0xff008000 },
                    angle: s.angle,
                    center: s.center.into(),
                    width: s.width,
                    height: s.height,
                })
                .collect(),
            rows: e
                .point_rows()
                .into_iter()
                .map(|(label, x, y, editable)| PolygonEdRow {
                    label: label as u32,
                    x,
                    y,
                    editable,
                })
                .collect(),
            partial: e.partial.iter().copied().map(Into::into).collect(),
            ghost: e.ghost.map(Into::into),
            band: e
                .band
                .map(|(a, b)| vec![a.into(), b.into()])
                .unwrap_or_default(),
            handles: e
                .adorner()
                .map(|(_, _, _, h)| h.into_iter().map(Into::into).collect())
                .unwrap_or_default(),
            frames: e
                .frames
                .iter()
                .enumerate()
                .map(|(i, f)| PolygonEdFrame {
                    time: f.time,
                    selected: i == e.frame,
                    has_shape: f.shape.is_some(),
                })
                .collect(),
            selected_frame: e.frame as u32,
        }
    }
    pub fn set_mode(&self, mode: String) {
        self.draft.lock().set_mode(match mode.as_str() {
            "Polygon" => Mode::Polygon,
            "Line" => Mode::Line,
            "Ellipse" => Mode::Ellipse,
            "AddPoint" => Mode::AddPoint,
            _ => Mode::Select,
        });
    }
    pub fn set_label(&self, label: String) {
        self.draft.lock().set_label(label);
    }
    pub fn show_labels(&self, show: bool) {
        self.draft.lock().show_labels = show;
    }
    pub fn pointer_down(&self, x: f64, y: f64) {
        if x.is_finite() && y.is_finite() {
            self.draft.lock().pointer_down(Point::new(x, y));
        }
    }
    pub fn pointer_move(&self, x: f64, y: f64, dragging: bool, shift: bool) {
        if x.is_finite() && y.is_finite() {
            self.draft
                .lock()
                .pointer_move(Point::new(x, y), dragging, shift);
        }
    }
    pub fn pointer_up(&self, x: f64, y: f64, shift: bool) {
        if x.is_finite() && y.is_finite() {
            self.draft.lock().pointer_up(Point::new(x, y), shift);
        }
    }
    pub fn pointer_leave(&self) {
        self.draft.lock().pointer_leave();
    }
    pub fn cursor(&self, x: f64, y: f64) -> String {
        self.draft.lock().cursor(Point::new(x, y)).into()
    }
    pub fn set_point(&self, row: u32, axis: String, value: String) -> Result<(), FfiError> {
        self.draft
            .lock()
            .set_point(row as usize, &axis, &value)
            .map_err(FfiError::new)
    }
    pub fn delete(&self) {
        self.draft.lock().delete();
    }
    pub fn convert(&self, kind: String) {
        self.draft.lock().convert(match kind.as_str() {
            "Line" => Kind::Line,
            "Ellipse" => Kind::Ellipse,
            _ => Kind::Polygon,
        });
    }
    pub fn toggle_start(&self) {
        self.draft.lock().toggle_start();
    }
    pub fn copy(&self) -> Option<String> {
        self.draft.lock().copy()
    }
    pub fn paste(&self, value: String) -> bool {
        self.draft.lock().paste(&value)
    }
    pub fn select_frame(&self, index: u32) {
        self.draft.lock().select_frame(index as usize);
    }
    pub fn add_frame(&self, width: f64) {
        self.draft.lock().add_frame(width);
    }
    pub fn delete_frame(&self) {
        self.draft.lock().delete_frame();
    }
    pub fn move_frame_at(&self, position: f64, width: f64) {
        self.draft.lock().move_frame_at(position, width);
    }
    pub fn move_frame(&self, time: f64) {
        self.draft.lock().move_frame(time);
    }
    pub fn commit(&self) -> Option<String> {
        let mut finished = self.finished.lock();
        if *finished {
            return None;
        }
        *finished = true;
        let Some((session, ids)) = &self.target else {
            return None;
        };
        let draft = self.draft.lock();
        polygon::apply_to_sequence(&mut session.lock(), ids, &draft)
    }
}
