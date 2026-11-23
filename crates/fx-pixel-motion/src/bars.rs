//! Port of VixenModules.Effect.Bars: flat and zig zag bars, in string and location mode, with rotation.
//!
//! The C# keeps a tile of the repeating pattern in a frame buffer and walks it with a moving offset. The
//! delegates it passes around (`InitializeTileYPosition`, `CalculatePixelColor`,
//! `ConvertFromLocationToStringCoordinates`) become small enums here.
//!
//! Where the C# would throw (an empty colour list, a tile read outside the tile) the render stays dark or
//! reads an empty pixel instead of panicking.

use crate::generated::bars as gen;
use vixen_effects::prelude::*;

pub static DESCRIPTOR: EffectDescriptor = EffectDescriptor {
    render,
    visibility: Some(visibility),
    param_changed: Some(param_changed),
    param_changed_ctx: Some(studio_param_changed_ctx), // STUDIO: bars-step-marks picks a mark collection
    supports_location: true,
    ported: true,
    ..gen::BASE
};

/// `InitAllAttributes` and `TargetPositioningChanged`: orientation, movement type, bar type and
/// highlight attributes.
fn visibility(p: &Params, _t: &TargetInfo, v: &mut Visibility) {
    let locations = p.enum_name("TargetPositioning") == "Locations";
    v.set("StringOrientation", !locations);
    v.set("TargetPositioning", true);

    let speed = p.enum_name("MovementType") == "Speed";
    v.set("SpeedCurve", speed);
    v.set("Speed", !speed);

    let flat = p.enum_name("BarType") == "Flat";
    v.set("Repeat", flat);
    v.set("MovementType", flat);
    v.set("ZigZagAmplitude", !flat);
    v.set("ZigZagPeriod", !flat);
    v.set("ZigZagBarThickness", !flat);
    v.set("ZigZagSpacing", !flat);

    v.set("HighlightPercentage", p.boolean("Highlight") && locations);

    // STUDIO-BEGIN bars-step-visibility: Step on Marks replaces the movement settings while it is on (#67)
    let steps = flat && p.boolean("StepToMarks");
    v.set("StepToMarks", flat);
    v.set("StepEveryMs", steps);
    v.set("MarkCollectionId", steps && p.int("StepEveryMs") <= 0);
    v.set("StepsPerCycle", steps);
    if steps {
        v.set("MovementType", false);
        v.set("Speed", false);
        v.set("SpeedCurve", false);
    }
    // STUDIO-END bars-step-visibility
}

// STUDIO-BEGIN bars-step-marks-select: turning Step on Marks on, or the sequence's collections changing, picks a collection
/// Like the C# effects that read marks: while stepping, keep the chosen collection if it still exists, else take the first.
fn studio_param_changed_ctx(c: &ParamChange, p: &mut Params) {
    if matches!(c.name, MARK_COLLECTIONS | "StepToMarks") {
        normalize_mark_selection(p, c.info, p.boolean("StepToMarks"), None, true);
    }
}
// STUDIO-END bars-step-marks-select

/// The `BarType` setter: zig zags only support the speed movement type and need a slower default speed.
fn param_changed(name: &str, p: &mut Params) {
    if name == "BarType" {
        if p.enum_name("BarType") == "ZigZag" {
            p.set("MovementType", ParamValue::Enum("Speed".to_string()));
            p.set("SpeedCurve", ParamValue::Curve(Curve::from_points(vec![(0.0, 10.0), (100.0, 10.0)])));
        } else {
            p.set("MovementType", ParamValue::Enum("Iterations".to_string()));
            p.set("SpeedCurve", ParamValue::Curve(Curve::from_points(vec![(0.0, 50.0), (100.0, 50.0)])));
        }
    }
}

/// `BarDirection`, in declaration order because the C# compares with `<` and `>`.
#[derive(Copy, Clone, PartialEq, PartialOrd)]
enum Dir {
    Up,
    Down,
    Expand,
    Compress,
    Left,
    Right,
    HExpand,
    HCompress,
    AlternateUp,
    AlternateDown,
    AlternateLeft,
    AlternateRight,
}

impl Dir {
    fn from_name(n: &str) -> Dir {
        match n {
            "Down" => Dir::Down,
            "Expand" => Dir::Expand,
            "Compress" => Dir::Compress,
            "Left" => Dir::Left,
            "Right" => Dir::Right,
            "HExpand" => Dir::HExpand,
            "HCompress" => Dir::HCompress,
            "AlternateUp" => Dir::AlternateUp,
            "AlternateDown" => Dir::AlternateDown,
            "AlternateLeft" => Dir::AlternateLeft,
            "AlternateRight" => Dir::AlternateRight,
            _ => Dir::Up,
        }
    }

    fn is_alternate(self) -> bool {
        self >= Dir::AlternateUp
    }
}

/// Which tile `InitializeTileYPosition` and friends work on.
#[derive(Copy, Clone, PartialEq)]
enum TileKind {
    Flat,
    ZigZag,
}

/// `ConvertFromLocationToStringCoordinates`.
#[derive(Copy, Clone, PartialEq)]
enum Convert {
    /// `ConvertFromHorizontalLocationToStringCoordinatesFlip`
    HorizontalFlip,
    /// `ConvertFromVerticalLocationToStringCoordinates`
    Vertical,
}

/// `CalculatePixelColor`.
#[derive(Copy, Clone, PartialEq)]
enum ColorCalc {
    Moving,
    Alternating,
}

/// A `RotateTransform` as WPF applies it to a point, with the buffer offsets of `GetRotatedPosition`.
struct Rotation {
    cos: f64,
    sin: f64,
    dx: f64,
    dy: f64,
    x_offset: i32,
    y_offset: i32,
}

impl Rotation {
    fn apply(&self, x: i32, y: i32) -> (i32, i32) {
        let px = (x + self.x_offset) as f64;
        let py = (y + self.y_offset) as f64;
        // Matrix.MultiplyPoint with (m11, m12, m21, m22) = (cos, sin, -sin, cos).
        let nx = py * -self.sin + self.dx;
        let ny = px * self.sin + self.dy;
        let tx = px * self.cos + nx;
        let ty = py * self.cos + ny;
        (tx.round_ties_even() as i32, ty.round_ties_even() as i32)
    }
}

struct Bars {
    zig_zag: bool,
    direction: Dir,
    iterations: bool,
    speed: i32,
    repeat: i32,
    highlight: bool,
    highlight_percentage: i32,
    show_3d: bool,
    locations: bool,
    angle_is_zero: bool,
    level_curve: Curve,
    speed_curve: Curve,
    rotation_curve: Curve,
    colors: Vec<ColorGradient>,
    zig_zag_amplitude_setting: i32,
    zig_zag_period_setting: i32,
    zig_zag_thickness_setting: i32,
    zig_zag_spacing_setting: i32,

    // Geometry of the display element, from `PixelInfo`.
    ht: i32,
    wi: i32,
    ht_offset: i32,
    wi_offset: i32,
    frame_time: i32,
    total_ms: f64,

    // State of `_position` and the tiles.
    position: f64,
    neg_position: bool,
    static_flat: PixelFrameBuffer,
    flat_bars_bar_height: i32,
    static_zig_zag: PixelFrameBuffer,
    static_zig_zag_height: i32,
    height_of_tile: i32,
    width_of_tile: i32,
    y_tile_start: i32,
    zig_zag_thickness: i32,
    zig_zag_spacing: i32,
    zig_zag_amplitude: i32,
    zig_zag_period: i32,
    scale_value: i32,
    buffer_ht: i32,
    buffer_wi: i32,
    length: i32,
    /// Set when the C# would throw on every frame (no colours, empty tiles).
    broken: bool,
    /// A read the C# would have thrown on. Interior mutability because colour lookups take `&self`.
    threw: std::cell::Cell<bool>,

    // Values every pixel of one frame shares.
    frame_level: f64,
    frame_flat_speed: f64,
    frame_interval_pos: f64,

    /// The enlarged buffer string mode renders into when rotating.
    virtual_buffer: PixelFrameBuffer,

    /// Step on Marks: the effect's marks and the steps per cycle, when on (flat bars only).
    studio_steps: Option<(vixen_effects::studio_mark_steps::MarkSteps, i32)>, // STUDIO: bars-step-marks
}

fn render(ctx: &RenderCtx, p: &Params) -> EffectIntents {
    let rotation_curve = p.curve("RotationAngle");
    let pts = &rotation_curve.points;
    let angle_is_zero = pts.len() == 2 && pts[0] == (0.0, 50.0) && pts[1] == (100.0, 50.0);
    let mut e = Bars {
        zig_zag: p.enum_name("BarType") == "ZigZag",
        direction: Dir::from_name(p.enum_name("Direction")),
        iterations: p.enum_name("MovementType") == "Iterations",
        speed: p.int("Speed"),
        repeat: p.int("Repeat"),
        highlight: p.boolean("Highlight"),
        highlight_percentage: p.int("HighlightPercentage"),
        show_3d: p.boolean("Show3D"),
        locations: TargetPositioning::from_name(p.enum_name("TargetPositioning")) == TargetPositioning::Locations,
        angle_is_zero,
        level_curve: p.curve("LevelCurve"),
        speed_curve: p.curve("SpeedCurve"),
        rotation_curve,
        colors: p.gradients("Colors"),
        zig_zag_amplitude_setting: p.int("ZigZagAmplitude"),
        zig_zag_period_setting: p.int("ZigZagPeriod"),
        zig_zag_thickness_setting: p.int("ZigZagBarThickness"),
        zig_zag_spacing_setting: p.int("ZigZagSpacing"),
        ht: 0,
        wi: 0,
        ht_offset: 0,
        wi_offset: 0,
        frame_time: 50,
        total_ms: 0.0,
        position: 0.0,
        neg_position: false,
        static_flat: PixelFrameBuffer::new(0, 0),
        flat_bars_bar_height: 0,
        static_zig_zag: PixelFrameBuffer::new(0, 0),
        static_zig_zag_height: 0,
        height_of_tile: 0,
        width_of_tile: 0,
        y_tile_start: 0,
        zig_zag_thickness: 0,
        zig_zag_spacing: 0,
        zig_zag_amplitude: 0,
        zig_zag_period: 0,
        scale_value: 0,
        buffer_ht: 0,
        buffer_wi: 0,
        length: 0,
        broken: false,
        threw: std::cell::Cell::new(false),
        frame_level: 1.0,
        frame_flat_speed: 0.0,
        frame_interval_pos: 0.0,
        virtual_buffer: PixelFrameBuffer::new(0, 0),
        studio_steps: None, // STUDIO: bars-step-marks
    };
    // STUDIO-BEGIN bars-step-marks-render: the marks (or the Step Every interval) Step on Marks counts, taken once per render
    if !e.zig_zag && p.boolean("StepToMarks") {
        let marks = ctx.mark_collection(p.guid("MarkCollectionId"));
        let every = p.int("StepEveryMs");
        let steps = vixen_effects::studio_mark_steps::MarkSteps::for_effect(marks, every, ctx.start_time, ctx.time_span);
        e.studio_steps = Some((steps, p.int("StepsPerCycle")));
    }
    // STUDIO-END bars-step-marks-render
    let out = render_pixel_effect(ctx, p, &mut e);
    // An exception in a pixel effect means `_elementData` is never assigned, so the whole effect is dark.
    if e.broken || e.threw.get() {
        return EffectIntents::new();
    }
    out
}

/// `GetColorAt` on a tile or virtual buffer. Out of range throws in the C#, which `threw` records.
#[inline]
fn get_safe(threw: &std::cell::Cell<bool>, b: &PixelFrameBuffer, x: i32, y: i32) -> Color {
    if x >= 0 && x < b.width() && y >= 0 && y < b.height() {
        b.get_color_at(x, y)
    } else {
        threw.set(true);
        Color::EMPTY
    }
}

/// `x % m` where the C# would throw on a zero divisor.
#[inline]
fn modulo(x: i32, m: i32) -> i32 {
    if m == 0 {
        0
    } else {
        x.wrapping_rem(m)
    }
}

impl Bars {
    // ---------------------------------------------------------------- shared helpers

    #[inline]
    fn interval_position(&self, frame: i32) -> f64 {
        if self.total_ms == 0.0 {
            0.0
        } else {
            (frame * self.frame_time) as f64 / self.total_ms
        }
    }

    /// The per-frame values the `GetEffectTimeIntervalPosition(frame) * 100` lookups share.
    fn begin_frame(&mut self, frame: i32) {
        let pos = self.interval_position(frame);
        let factor = pos * 100.0;
        self.frame_interval_pos = pos;
        self.frame_level = self.level_curve.get_value(factor) / 100.0;
        self.frame_flat_speed = self.calculate_flat_bar_speed(factor);
    }

    fn calculate_flat_bar_speed(&self, pos: f64) -> f64 {
        scale_curve_to_value(self.speed_curve.get_value(pos), 15.0, -15.0)
    }

    fn calculate_rotation_angle(&self, pos: f64) -> f64 {
        scale_curve_to_value(self.rotation_curve.get_value(pos), 180.0, -180.0)
    }

    /// `GetRotationAngle`.
    fn get_rotation_angle(&self, frame: i32) -> f64 {
        self.calculate_rotation_angle(self.interval_position(frame) * 100.0)
    }

    fn calculate_zig_zag_speed(&self, pos: f64) -> f64 {
        // Scale the speed based on the smaller of the width and height of the display element.
        let new_scale_value = (self.scale_value as f64 * 0.20) as i32;
        scale_curve_to_value(self.speed_curve.get_value(pos), new_scale_value as f64, 0.0)
    }

    /// `GetZigZagYValue`.
    fn get_zig_zag_y_value(&self, frame: i32) -> i32 {
        (frame as f64 * (self.frame_time as f64 / 50.0) * self.calculate_zig_zag_speed(self.interval_position(frame))) as i32
    }

    /// `AdjustIntensity`.
    fn adjust_intensity(&self, color: Color) -> Color {
        let level = self.frame_level;
        if level < 1.0 && color != Color::TRANSPARENT {
            let mut hsv = Hsv::from_color(color);
            hsv.set_v(hsv.v * level);
            hsv.to_color()
        } else {
            color
        }
    }

    /// `Colors[idx].GetColorAt(pos)`. An index outside the list throws in the C#, which `threw` records.
    fn color_at(&self, idx: i32, pos: f64) -> Color {
        match usize::try_from(idx).ok().and_then(|i| self.colors.get(i)) {
            Some(g) => g.get_color_at(pos),
            None => {
                self.threw.set(true);
                Color::EMPTY
            }
        }
    }

    /// `GetFlatBarsBufferHeight`.
    fn flat_bars_buffer_height(&self) -> i32 {
        match self.direction {
            Dir::Up | Dir::Down | Dir::Expand | Dir::Compress | Dir::AlternateUp | Dir::AlternateDown => self.ht,
            Dir::Left | Dir::Right | Dir::HExpand | Dir::HCompress | Dir::AlternateLeft | Dir::AlternateRight => self.wi,
        }
    }

    /// `GetHighlightColor`.
    fn get_highlight_color(&self, color_index: i32) -> Color {
        let c = self.color_at(color_index, 0.0);
        let mut hsv = Hsv::from_color(c);
        hsv.set_s(0.0);
        hsv.to_color()
    }

    /// `GetBarColor`.
    fn get_bar_color(&self, color_index: i32, counter: i32, bar_thickness: i32) -> Color {
        let mut high_light_row = 0;
        if self.locations {
            // Make the first % pixels white. `barThickness * HighlightPercentage` is an int product.
            high_light_row = (bar_thickness.wrapping_mul(self.highlight_percentage) as f64 / 100.0) as i32;
        }
        if self.highlight && counter <= high_light_row {
            self.get_highlight_color(color_index)
        } else if self.show_3d {
            self.get_3d_color(color_index, counter, bar_thickness)
        } else {
            self.color_at(color_index, counter as f64 / bar_thickness as f64)
        }
    }

    /// `Get3DColor`.
    fn get_3d_color(&self, color_index: i32, counter: i32, bar_thickness: i32) -> Color {
        let color = self.color_at(color_index, counter as f64 / bar_thickness as f64);
        let mut hsv = Hsv::from_color(color);
        // `(float)(a - b) / c` is float arithmetic.
        let factor = (bar_thickness - counter) as f32 / bar_thickness as f32;
        hsv.set_v(hsv.v * factor as f64);
        hsv.to_color()
    }

    /// `GetRotatedPosition`'s transform for one angle.
    fn rotation(&self, angle: f64, swap_xy: bool) -> Rotation {
        let (y_offset, x_offset) = if swap_xy {
            ((self.length - self.wi) / 2, (self.length - self.ht) / 2)
        } else {
            ((self.length - self.ht) / 2, (self.length - self.wi) / 2)
        };
        let center = (self.length - 1) as f64 / 2.0;
        // RotateTransform(-angle, center, center)
        let radians = ((-angle) % 360.0) * (std::f64::consts::PI / 180.0);
        let (sin, cos) = (radians.sin(), radians.cos());
        let dx = center * (1.0 - cos) + center * sin;
        let dy = center * (1.0 - cos) - center * sin;
        Rotation { cos, sin, dx, dy, x_offset, y_offset }
    }

    // ---------------------------------------------------------------- setup

    fn setup(&mut self, info: &PixelInfo) {
        self.ht = info.buffer_ht();
        self.wi = info.buffer_wi();
        self.ht_offset = info.buffer_ht_offset();
        self.wi_offset = info.buffer_wi_offset();
        self.frame_time = info.frame_time;
        self.total_ms = if info.time_span == TimeSpan::ZERO { 0.0 } else { info.time_span.total_ms() };
        self.position = 0.0;
        self.neg_position = false;
        self.broken = false;

        // The diagonal of the display element, plus 2 as a margin so a rotated pixel doesn't go negative.
        let (ht, wi) = (self.ht as f64, self.wi as f64);
        self.length = ((ht * ht) + (wi * wi)).sqrt().round() as i32 + 2;

        if !self.locations {
            if self.angle_is_zero {
                self.buffer_wi = self.wi;
                self.buffer_ht = self.ht;
            } else {
                // The larger virtual frame buffer.
                self.buffer_wi = self.length;
                self.buffer_ht = self.length;
            }
        } else {
            self.buffer_wi = self.wi;
            self.buffer_ht = self.ht;
        }

        // Every path indexes the colour list.
        if self.colors.is_empty() {
            self.broken = true;
            return;
        }

        if self.zig_zag {
            self.scale_value = self.ht.min(self.wi);
            self.zig_zag_thickness = self.get_zig_zag_thickness(self.scale_value);
            self.zig_zag_spacing = self.get_zig_zag_spacing(self.scale_value);
            self.height_of_tile = self.colors.len() as i32 * (self.zig_zag_spacing + self.zig_zag_thickness);
            self.zig_zag_period = self.get_zig_zag_period(self.scale_value);
            self.width_of_tile = 2 * self.zig_zag_period - 2;
            self.zig_zag_amplitude = self.get_zig_zag_amplitude(self.scale_value);
            self.y_tile_start = self.get_zig_zag_tile_y_start_position();
            self.static_zig_zag_height = self.height_of_tile + self.y_tile_start;
            if self.height_of_tile <= 0 || self.width_of_tile <= 0 || self.static_zig_zag_height <= 0 {
                self.broken = true;
                return;
            }
            self.static_zig_zag = PixelFrameBuffer::new(self.width_of_tile, self.static_zig_zag_height);
            self.setup_render_zig_zag();
        } else {
            self.setup_render_flat();
        }
    }

    fn get_zig_zag_amplitude(&self, scale_value: i32) -> i32 {
        (self.zig_zag_amplitude_setting as f64 / 100.0 * scale_value as f64) as i32
    }

    fn get_zig_zag_period(&self, scale_value: i32) -> i32 {
        let period = (self.zig_zag_period_setting as f64 / 100.0 * scale_value as f64) as i32;
        // Ensure the period is always at least 2 pixels
        period.max(2)
    }

    fn get_zig_zag_spacing(&self, scale_value: i32) -> i32 {
        (self.zig_zag_spacing_setting as f64 / 100.0 * scale_value as f64) as i32
    }

    fn get_zig_zag_thickness(&self, scale_value: i32) -> i32 {
        let thickness = (self.zig_zag_thickness_setting as f64 / 100.0 * scale_value as f64) as i32;
        // Ensure the thickness is always at least one pixel
        if thickness == 0 {
            1
        } else {
            thickness
        }
    }

    /// `GetZigZagTileYStartPosition`.
    fn get_zig_zag_tile_y_start_position(&self) -> i32 {
        (self.zig_zag_amplitude + self.zig_zag_thickness - 1 + self.zig_zag_spacing)
            + (self.colors.len() as i32 - 1) * (self.zig_zag_thickness + self.zig_zag_spacing)
    }

    /// `SetupRenderZigZag`: draws the repeating tile.
    fn setup_render_zig_zag(&mut self) {
        let mut color_index = 0;
        let mut counter = 0;
        let mut color = self.get_bar_color(color_index, counter, self.zig_zag_thickness);

        // The slope of the zig zag. A zero amplitude would go negative, so it is a flat bar instead.
        let mut d_zig_zag_increment = (self.zig_zag_amplitude - 1) as f64 / (self.zig_zag_period - 1) as f64;
        if d_zig_zag_increment < 0.0 {
            d_zig_zag_increment = 0.0;
        }

        for y in 0..self.static_zig_zag_height {
            let mut y1 = 0.0;
            let mut increment = d_zig_zag_increment;
            for x in 0..self.width_of_tile {
                if (y as f64 + y1) < self.static_zig_zag_height as f64 {
                    self.static_zig_zag.set_pixel(x, (y as f64 + y1) as i32, color);
                }
                y1 += increment;
                // Done going up: start going down
                if y1 >= (self.zig_zag_amplitude - 1) as f64 {
                    increment = -d_zig_zag_increment;
                } else if y1 <= 0.0 {
                    increment = d_zig_zag_increment;
                }
            }

            counter += 1;
            // Completed either a zig zag bar or a blank space
            if (color != Color::TRANSPARENT && counter == self.zig_zag_thickness)
                || (color == Color::TRANSPARENT && counter == self.zig_zag_spacing)
            {
                counter = 0;
                if color != Color::TRANSPARENT && self.zig_zag_spacing != 0 {
                    color = Color::TRANSPARENT;
                } else {
                    color_index += 1;
                    if color_index == self.colors.len() as i32 {
                        color_index = 0;
                    }
                    color = self.get_bar_color(color_index, counter, self.zig_zag_thickness);
                }
            } else if color != Color::TRANSPARENT {
                color = self.get_bar_color(color_index, counter, self.zig_zag_thickness);
            }
        }
    }

    /// `SetupRenderFlat`.
    fn setup_render_flat(&mut self) {
        let colorcnt = self.colors.len() as i32;
        let mut bar_count = self.repeat.wrapping_mul(colorcnt);
        if bar_count < 1 {
            bar_count = 1;
        }
        let mut bar_ht = self.flat_bars_buffer_height() / bar_count + 1;
        if bar_ht < 1 {
            bar_ht = 1;
        }
        self.flat_bars_bar_height = bar_ht;
        self.height_of_tile = colorcnt * bar_ht;
        if self.height_of_tile < 1 {
            self.height_of_tile = 1;
        }
        // Bars don't vary across the bar so we only need one pixel
        self.width_of_tile = 1;
        self.static_flat = PixelFrameBuffer::new(self.width_of_tile, self.height_of_tile);
        let mut y = 0;
        for color_index in 0..colorcnt {
            for thickness_counter in 0..bar_ht {
                let color = self.get_bar_color(color_index, thickness_counter, bar_ht);
                self.static_flat.set_pixel(0, y, color);
                y += 1;
            }
        }
    }

    // ---------------------------------------------------------------- position updates

    /// `UpdateZigZagPosition`.
    fn update_zig_zag_position(&mut self, frame: i32) {
        let factor = self.interval_position(frame) * 100.0;
        if frame == 0 {
            self.position = self.calculate_zig_zag_speed(factor);
        }
        self.position += self.calculate_zig_zag_speed(factor) / 100.0;
    }

    /// `UpdateFlatPosition`. Uses the speed cached by `begin_frame`; the C# recomputes the same value.
    fn update_flat_position(&mut self, frame: i32) {
        // STUDIO-BEGIN bars-step-marks-position: Step on Marks sets the position from the marks passed
        if let Some(position) = self.studio_step_position(frame) {
            self.position = position;
            return;
        }
        // STUDIO-END bars-step-marks-position
        if self.iterations {
            self.position = (self.frame_interval_pos * self.speed as f64) % 1.0;
        } else {
            if frame == 0 {
                self.position = self.frame_flat_speed;
            }
            self.position += self.frame_flat_speed / 100.0;
        }
    }

    // STUDIO-BEGIN bars-step-marks-helper: each mark moves the pattern 1 / StepsPerCycle of one color cycle
    /// The position Step on Marks gives `frame`, when it is on. A whole color cycle is `1 / Repeat` of the position.
    fn studio_step_position(&self, frame: i32) -> Option<f64> {
        let (steps, per_cycle) = self.studio_steps.as_ref()?;
        let cycle = steps.cycle_position((frame * self.frame_time) as f64, *per_cycle);
        Some(cycle / self.repeat.max(1) as f64)
    }
    // STUDIO-END bars-step-marks-helper

    // ---------------------------------------------------------------- tile position helpers

    /// `InitializeFlatBarTileYPosition`.
    fn init_flat_tile_y(&self, height_of_tile: i32) -> i32 {
        let tile_y = (self.position * self.height_of_tile as f64 * self.repeat as f64) as i32;
        modulo(tile_y, height_of_tile)
    }

    /// `InitializeZigZagTileYPosition`.
    fn init_zig_zag_tile_y(&self, increment: bool, height_of_tile: i32, bar_thickness: i32, frame: i32) -> i32 {
        if increment {
            modulo(self.get_zig_zag_y_value(frame), height_of_tile)
        } else {
            let tile_y = ((self.position * self.colors.len() as f64).floor() * bar_thickness as f64) as i32;
            modulo(tile_y, height_of_tile)
        }
    }

    fn init_tile_y(&self, kind: TileKind, increment: bool, height_of_tile: i32, bar_thickness: i32, frame: i32) -> i32 {
        match kind {
            TileKind::Flat => self.init_flat_tile_y(height_of_tile),
            TileKind::ZigZag => self.init_zig_zag_tile_y(increment, height_of_tile, bar_thickness, frame),
        }
    }

    /// `UpdateZigZagTileYPosition`.
    fn update_zig_zag_tile_y(increment: bool, mut tile_y: i32, height_of_tile: i32) -> i32 {
        if increment {
            tile_y += 1;
            if tile_y == height_of_tile {
                tile_y = 0;
            }
        } else {
            tile_y -= 1;
            if tile_y < 0 {
                tile_y = height_of_tile - 1;
            }
        }
        tile_y
    }

    /// `CalculateZigZagYTilePosition`.
    fn zig_zag_y_tile_position(incrementing: bool, y: i32, movement_y: i32, height_of_tile: i32, y_offset: i32) -> i32 {
        let mut y_tile = if incrementing { modulo(y + movement_y, height_of_tile) } else { y - y_offset - movement_y };
        if y_tile < 0 {
            y_tile += height_of_tile;
        }
        modulo(y_tile, height_of_tile)
    }

    // ---------------------------------------------------------------- colour calculators

    fn tile(&self, kind: TileKind) -> &PixelFrameBuffer {
        match kind {
            TileKind::Flat => &self.static_flat,
            TileKind::ZigZag => &self.static_zig_zag,
        }
    }

    /// `CalculateMovingColor`.
    #[allow(clippy::too_many_arguments)]
    fn calculate_moving_color(
        &self,
        kind: TileKind,
        x: i32,
        y: i32,
        height_of_tile: i32,
        width_of_tile: i32,
        moves_right: bool,
        y_tile_start_position: i32,
        movement_y: i32,
    ) -> Color {
        let y_tile = Self::zig_zag_y_tile_position(moves_right, y, movement_y, height_of_tile, 0);
        let x_tile = modulo(x, width_of_tile);
        let tile = self.tile(kind);
        if moves_right {
            get_safe(&self.threw, tile, x_tile, y_tile + y_tile_start_position)
        } else {
            get_safe(&self.threw, tile, x_tile, (height_of_tile - 1) - y_tile + y_tile_start_position)
        }
    }

    /// `CalculateAlternatingColor`.
    fn calculate_alternating_color(&mut self, y: i32, frame: i32, height_of_tile: i32) -> Color {
        let bar_count = self.repeat.wrapping_mul(self.colors.len() as i32);
        if bar_count == 0 {
            // Division by zero in the C#.
            return Color::EMPTY;
        }
        let bar_ht = self.flat_bars_buffer_height() / bar_count + 1;
        self.get_flat_bars_alternating_color(y, frame, height_of_tile, bar_count, bar_ht)
    }

    /// `GetFlatBarsAlternatingColor`. Like the C#, this moves `_position` on every call.
    fn get_flat_bars_alternating_color(&mut self, mut y: i32, frame: i32, block_ht: i32, bar_count: i32, bar_ht: i32) -> Color {
        if self.direction == Dir::AlternateUp || self.direction == Dir::AlternateLeft {
            // Need to flip the coordinates around
            let h = self.flat_bars_buffer_height();
            y = h - y;
            if h > 0 {
                while y < 0 {
                    y += h;
                }
            }
        }
        self.update_flat_position(frame);

        // The following math was copied from the original effect algorithm prior to adding rotation
        let f_offset = ((self.position * bar_count as f64).floor() * bar_ht as f64) as i32;
        let index_adjust = 1;
        let n = y.wrapping_add(f_offset);
        let color_idx = modulo(n + index_adjust, block_ht) / bar_ht.max(1);
        self.get_bar_color(color_idx, modulo(n + index_adjust, bar_ht), bar_ht)
    }

    // ---------------------------------------------------------------- location rendering

    /// The two `ConvertFrom...LocationToStringCoordinates` methods.
    fn convert_location(&self, convert: Convert, x_in: i32, y_in: i32) -> (i32, i32) {
        match convert {
            // Swaps the input, then flips the y coordinate within the buffer width.
            Convert::HorizontalFlip => {
                let y = ((self.wi_offset - x_in) + (self.buffer_wi - 1 + self.wi_offset)).abs();
                (y_in - self.ht_offset, y - self.wi_offset)
            }
            Convert::Vertical => {
                let y = ((self.ht_offset - y_in) + (self.buffer_ht - 1 + self.ht_offset)).abs();
                (x_in - self.wi_offset, y - self.ht_offset)
            }
        }
    }

    /// `RenderEffectByLocation(frame, ...)`, the common routine for moving and alternating bars.
    #[allow(clippy::too_many_arguments)]
    fn render_by_location(
        &mut self,
        frame: i32,
        buf: &mut PixelLocationFrameBuffer,
        nodes: &[ElementLocation],
        kind: TileKind,
        y_tile_start_position: i32,
        bar_thickness: i32,
        left_right: bool,
        moves_right: bool,
        swap_xy: bool,
        convert: Convert,
        color_calc: ColorCalc,
    ) {
        let (height_of_tile, width_of_tile) = (self.height_of_tile, self.width_of_tile);
        let mut angle = self.get_rotation_angle(frame);
        if swap_xy {
            angle = -angle;
        }
        let movement_y = self.init_tile_y(kind, left_right, height_of_tile, bar_thickness, frame);
        let rotation = if angle != 0.0 { Some(self.rotation(angle, swap_xy)) } else { None };
        for node in nodes {
            let (mut x, mut y) = self.convert_location(convert, node.x, node.y);
            if let Some(r) = &rotation {
                (x, y) = r.apply(x, y);
            }
            let color = match color_calc {
                ColorCalc::Moving => {
                    self.calculate_moving_color(kind, x, y, height_of_tile, width_of_tile, moves_right, y_tile_start_position, movement_y)
                }
                ColorCalc::Alternating => self.calculate_alternating_color(y, frame, height_of_tile),
            };
            buf.set_pixel(node.x, node.y, self.adjust_intensity(color));
        }
    }

    /// `RenderEffectLocationExpandCompress`.
    #[allow(clippy::too_many_arguments)]
    fn render_location_expand_compress(
        &mut self,
        frame: i32,
        buf: &mut PixelLocationFrameBuffer,
        nodes: &[ElementLocation],
        kind: TileKind,
        down_increases: bool,
        y_tile_start_position: i32,
        angle: f64,
        swap_xy: bool,
        convert: Convert,
        mut height: i32,
    ) {
        let (height_of_tile, width_of_tile) = (self.height_of_tile, self.width_of_tile);
        let movement_y = self.init_tile_y(kind, true, height_of_tile, 0, frame);
        let rotation = if angle != 0.0 { Some(self.rotation(angle, swap_xy)) } else { None };
        let compress = self.direction == Dir::Compress || self.direction == Dir::HCompress;
        for node in nodes {
            let (mut x, mut y) = self.convert_location(convert, node.x, node.y);
            if let Some(r) = &rotation {
                (x, y) = r.apply(x, y);
                // The logical frame buffer is the enlarged square one while rotating.
                height = self.length;
            }
            // Are we in the lower half of the display element
            let down = y < height / 2;
            let mut y_tile = Self::zig_zag_y_tile_position(if down_increases { down } else { !down }, y, movement_y, height_of_tile, height / 2);
            let repeating_x = modulo(x, width_of_tile);
            if down {
                y_tile = modulo(height / 2 - y_tile, height_of_tile);
                if y_tile < 0 {
                    y_tile += height_of_tile;
                }
            }
            let tile = self.tile(kind);
            let color = if compress {
                get_safe(&self.threw, tile, repeating_x, y_tile + y_tile_start_position)
            } else {
                get_safe(&self.threw, tile, repeating_x, (height_of_tile - 1) - y_tile + y_tile_start_position)
            };
            buf.set_pixel(node.x, node.y, self.adjust_intensity(color));
        }
    }

    /// The direction switch shared by `RenderEffectByLocationFlat` and `RenderEffectByLocationZigZag`.
    fn render_location_frame(&mut self, frame: i32, buf: &mut PixelLocationFrameBuffer, nodes: &[ElementLocation]) {
        let d = self.direction;
        let kind = if self.zig_zag { TileKind::ZigZag } else { TileKind::Flat };
        let (y_start, thickness) = if self.zig_zag {
            (self.y_tile_start, self.zig_zag_thickness + self.zig_zag_spacing)
        } else {
            (0, self.flat_bars_bar_height)
        };
        let color_calc = if self.zig_zag || !d.is_alternate() { ColorCalc::Moving } else { ColorCalc::Alternating };
        match d {
            Dir::Up | Dir::Down | Dir::AlternateUp | Dir::AlternateDown => self.render_by_location(
                frame,
                buf,
                nodes,
                kind,
                y_start,
                thickness,
                d == Dir::Down || d == Dir::Up,
                d == Dir::Down || d == Dir::AlternateDown,
                false,
                Convert::Vertical,
                color_calc,
            ),
            Dir::Left | Dir::Right | Dir::AlternateLeft | Dir::AlternateRight => self.render_by_location(
                frame,
                buf,
                nodes,
                kind,
                y_start,
                thickness,
                d == Dir::Left || d == Dir::Right,
                d == Dir::Right || d == Dir::AlternateRight,
                true,
                Convert::HorizontalFlip,
                color_calc,
            ),
            Dir::Expand | Dir::Compress => {
                let angle = self.get_rotation_angle(frame);
                let h = self.buffer_ht;
                self.render_location_expand_compress(frame, buf, nodes, kind, d == Dir::Expand, y_start, angle, false, Convert::Vertical, h)
            }
            Dir::HExpand | Dir::HCompress => {
                let angle = -self.get_rotation_angle(frame);
                let h = self.buffer_wi;
                self.render_location_expand_compress(frame, buf, nodes, kind, d == Dir::HExpand, y_start, angle, true, Convert::HorizontalFlip, h)
            }
        }
    }

    // ---------------------------------------------------------------- string rendering

    /// `RenderEffectStringsInternal`.
    fn render_strings_internal<B: PixelBuffer>(&mut self, frame: i32, buf: &mut B) {
        if self.zig_zag {
            self.render_strings_zig_zag(frame, buf);
        } else {
            self.render_flat(frame, buf);
        }
    }

    /// `RenderEffectStringsZigZag`.
    fn render_strings_zig_zag<B: PixelBuffer>(&mut self, frame: i32, buf: &mut B) {
        let d = self.direction;
        if d.is_alternate() {
            self.update_zig_zag_position(frame);
        }
        let (bht, bwi) = (self.buffer_ht, self.buffer_wi);
        match d {
            Dir::Up | Dir::Down | Dir::AlternateUp | Dir::AlternateDown => {
                self.render_zig_zag(frame, buf, d == Dir::Down || d == Dir::Up, d == Dir::Down || d == Dir::AlternateDown, bht, bwi, false)
            }
            Dir::Left | Dir::Right | Dir::AlternateLeft | Dir::AlternateRight => {
                self.render_zig_zag(frame, buf, d == Dir::Left || d == Dir::Right, d == Dir::Left || d == Dir::AlternateLeft, bwi, bht, true)
            }
            Dir::Expand => self.render_zig_zag_expand_compress(frame, buf, true, bht, bwi, false),
            Dir::Compress => self.render_zig_zag_expand_compress(frame, buf, false, bht, bwi, false),
            Dir::HExpand => self.render_zig_zag_expand_compress(frame, buf, true, bwi, bht, true),
            Dir::HCompress => self.render_zig_zag_expand_compress(frame, buf, false, bwi, bht, true),
        }
    }

    /// `SetPixelHorizontal` / `SetPixelVertical`.
    #[inline]
    fn set_tile_pixel<B: PixelBuffer>(&self, buf: &mut B, horizontal: bool, x: i32, y: i32, tile_x: i32, tile_y: i32) {
        let color = self.adjust_intensity(get_safe(&self.threw, &self.static_zig_zag, tile_x, tile_y + self.y_tile_start));
        if horizontal {
            buf.set_pixel(y, x, color);
        } else {
            buf.set_pixel(x, y, color);
        }
    }

    /// `RenderEffectZigZag`.
    #[allow(clippy::too_many_arguments)]
    fn render_zig_zag<B: PixelBuffer>(
        &mut self,
        frame: i32,
        buf: &mut B,
        left_right: bool,
        move_right: bool,
        y_length: i32,
        x_length: i32,
        horizontal: bool,
    ) {
        let height_of_tile = self.height_of_tile;
        let bar_thickness = self.zig_zag_thickness + self.zig_zag_spacing;
        let mut tile_y = self.init_zig_zag_tile_y(left_right, height_of_tile, bar_thickness, frame);
        for y in 0..y_length {
            for x in 0..x_length {
                let tile_x = modulo(x, self.width_of_tile);
                self.set_tile_pixel(buf, horizontal, x, y, tile_x, tile_y);
            }
            tile_y = Self::update_zig_zag_tile_y(move_right, tile_y, height_of_tile);
        }
    }

    /// `RenderEffectZigZagExpandCompress`.
    fn render_zig_zag_expand_compress<B: PixelBuffer>(
        &mut self,
        frame: i32,
        buf: &mut B,
        expand: bool,
        y_length: i32,
        x_length: i32,
        horizontal: bool,
    ) {
        let height_of_tile = self.height_of_tile;
        let mut tile_y = modulo(self.get_zig_zag_y_value(frame), height_of_tile);
        // The left half of the display element
        for y in (0..=(y_length / 2 - 1)).rev() {
            for x in 0..x_length {
                let tile_x = modulo(x, self.width_of_tile);
                self.set_tile_pixel(buf, horizontal, x, y, tile_x, tile_y);
            }
            tile_y = Self::update_zig_zag_tile_y(!expand, tile_y, height_of_tile);
        }
        tile_y = modulo(self.get_zig_zag_y_value(frame), height_of_tile);
        // The right half
        for y in (y_length / 2)..y_length {
            for x in 0..x_length {
                let tile_x = modulo(x, self.width_of_tile);
                self.set_tile_pixel(buf, horizontal, x, y, tile_x, tile_y);
            }
            tile_y = Self::update_zig_zag_tile_y(!expand, tile_y, height_of_tile);
        }
    }

    /// `RenderEffectFlat`: the original string algorithm, which doesn't use the tile.
    fn render_flat<B: PixelBuffer>(&mut self, frame: i32, buf: &mut B) {
        let colorcnt = self.colors.len() as i32;
        let mut bar_count = self.repeat.wrapping_mul(colorcnt);
        let interval_pos_factor = self.interval_position(frame) * 100.0;
        self.neg_position = false;

        if self.iterations {
            self.position = (self.interval_position(frame) * self.speed as f64) % 1.0;
        } else {
            if frame == 0 {
                self.position = self.calculate_flat_bar_speed(interval_pos_factor);
            }
            self.position += self.calculate_flat_bar_speed(interval_pos_factor) / 100.0;
            if self.position < 0.0 {
                self.neg_position = true;
                self.position = -self.position;
            }
        }
        // STUDIO-BEGIN bars-step-marks-flat: Step on Marks replaces the movement's position
        if let Some(position) = self.studio_step_position(frame) {
            self.position = position;
            self.neg_position = false;
        }
        // STUDIO-END bars-step-marks-flat

        if bar_count < 1 {
            bar_count = 1;
        }
        let level = self.level_curve.get_value(interval_pos_factor) / 100.0;
        let (buffer_wi, buffer_ht) = (self.buffer_wi, self.buffer_ht);
        let d = self.direction;
        let neg = self.neg_position;

        if d < Dir::Left || d == Dir::AlternateUp || d == Dir::AlternateDown {
            let mut bar_ht = self.ht / bar_count + 1;
            if bar_ht < 1 {
                bar_ht = 1;
            }
            let half_ht = buffer_ht / 2;
            let mut block_ht = colorcnt * bar_ht;
            if block_ht < 1 {
                block_ht = 1;
            }
            let mut f_offset = (self.position * block_ht as f64 * self.repeat as f64) as i32;
            if d == Dir::AlternateUp || d == Dir::AlternateDown {
                f_offset = ((self.position * bar_count as f64).floor() * bar_ht as f64) as i32;
            }
            let index_adjust = 1;
            for y in 0..buffer_ht {
                let n = y.wrapping_add(f_offset);
                // We need the integer division here to make things work
                let color_idx = modulo(n + index_adjust, block_ht) / bar_ht;
                let color_position = modulo(n + index_adjust, bar_ht) as f64 / bar_ht as f64;
                let mut c = self.color_at(color_idx, color_position);
                c = self.flat_shade(c, level, modulo(n + index_adjust, bar_ht), bar_ht);

                match d {
                    Dir::Down | Dir::AlternateDown => {
                        // down
                        let row = if neg { buffer_ht - y - 1 } else { y };
                        for x in 0..buffer_wi {
                            buf.set_pixel(x, row, c);
                        }
                    }
                    Dir::Compress => {
                        // The C# comments say "expand" here
                        let draw = if neg { y <= half_ht } else { y >= half_ht };
                        if draw {
                            for x in 0..buffer_wi {
                                buf.set_pixel(x, y, c);
                                buf.set_pixel(x, buffer_ht - y - 1, c);
                            }
                        }
                    }
                    Dir::Expand => {
                        // The C# comments say "compress" here
                        let draw = if !neg { y <= half_ht } else { y >= half_ht };
                        if draw {
                            for x in 0..buffer_wi {
                                buf.set_pixel(x, y, c);
                                buf.set_pixel(x, buffer_ht - y - 1, c);
                            }
                        }
                    }
                    _ => {
                        // up & AlternateUp
                        let row = if !neg { buffer_ht - y - 1 } else { y };
                        for x in 0..buffer_wi {
                            buf.set_pixel(x, row, c);
                        }
                    }
                }
            }
        } else {
            let mut bar_wi = self.wi / bar_count + 1;
            if bar_wi < 1 {
                bar_wi = 1;
            }
            let half_wi = buffer_wi / 2;
            let mut block_wi = colorcnt * bar_wi;
            if block_wi < 1 {
                block_wi = 1;
            }
            let mut f_offset = (self.position * block_wi as f64 * self.repeat as f64) as i32;
            if d > Dir::AlternateDown {
                f_offset = ((self.position * bar_count as f64).floor() * bar_wi as f64) as i32;
            }
            for x in 0..buffer_wi {
                let n = x.wrapping_add(f_offset);
                // We need the integer division here to make things work
                let color_idx = modulo(n + 1, block_wi) / bar_wi;
                let color_position = modulo(n + 1, bar_wi) as f64 / bar_wi as f64;
                let mut c = self.color_at(color_idx, color_position);
                c = self.flat_shade(c, level, modulo(n + 1, bar_wi), bar_wi);

                match d {
                    Dir::Right | Dir::AlternateRight => {
                        for y in 0..buffer_ht {
                            buf.set_pixel(buffer_wi - x - 1, y, c);
                        }
                    }
                    Dir::HExpand => {
                        if x <= half_wi {
                            for y in 0..buffer_ht {
                                buf.set_pixel(x, y, c);
                                buf.set_pixel(buffer_wi - x - 1, y, c);
                            }
                        }
                    }
                    Dir::HCompress => {
                        if x >= half_wi {
                            for y in 0..buffer_ht {
                                buf.set_pixel(x, y, c);
                                buf.set_pixel(buffer_wi - x - 1, y, c);
                            }
                        }
                    }
                    _ => {
                        // left & AlternateLeft
                        for y in 0..buffer_ht {
                            buf.set_pixel(x, y, c);
                        }
                    }
                }
            }
        }
    }

    /// The highlight, 3D and level handling of `RenderEffectFlat`. `m` is `(n + 1) % barSize`.
    fn flat_shade(&self, c: Color, level: f64, m: i32, bar_size: i32) -> Color {
        if self.highlight || self.show_3d {
            let mut hsv = Hsv::from_color(c);
            if self.highlight && m == 0 {
                hsv.set_s(0.0);
            }
            if self.show_3d {
                // `(float)(barHt - m - 1) / barHt` is float arithmetic.
                let f = (bar_size - m - 1) as f32 / bar_size as f32;
                hsv.set_v(hsv.v * f as f64);
            }
            hsv.set_v(hsv.v * level);
            hsv.to_color()
        } else if level < 1.0 {
            let mut hsv = Hsv::from_color(c);
            hsv.set_v(hsv.v * level);
            hsv.to_color()
        } else {
            c
        }
    }
}

impl PixelEffect for Bars {
    fn setup_render(&mut self, info: &PixelInfo) {
        self.setup(info);
    }

    fn render_effect(&mut self, frame: i32, info: &PixelInfo, buf: &mut PixelFrameBuffer) {
        let _ = info;
        if self.broken {
            return;
        }
        self.begin_frame(frame);
        let angle = self.get_rotation_angle(frame);
        if angle == 0.0 {
            // Render the effect in the original frame buffer
            self.render_strings_internal(frame, buf);
        } else {
            // A virtual frame buffer large enough to rotate the original one within it. Fresh buffers in
            // C# hold `Color.Empty` everywhere.
            let mut virtual_buffer = std::mem::replace(&mut self.virtual_buffer, PixelFrameBuffer::new(0, 0));
            if virtual_buffer.width() != self.buffer_wi || virtual_buffer.height() != self.buffer_ht {
                virtual_buffer = PixelFrameBuffer::new(self.buffer_wi, self.buffer_ht);
            } else {
                virtual_buffer.fill(Color::EMPTY);
            }
            self.render_strings_internal(frame, &mut virtual_buffer);

            let rotation = self.rotation(angle, false);
            for x in 0..self.wi {
                for y in 0..self.ht {
                    let (rx, ry) = rotation.apply(x, y);
                    buf.set_pixel(x, y, get_safe(&self.threw, &virtual_buffer, rx, ry));
                }
            }
            self.virtual_buffer = virtual_buffer;
        }
    }

    fn render_effect_by_location(&mut self, num_frames: i32, _info: &PixelInfo, buf: &mut PixelLocationFrameBuffer) {
        if self.broken {
            return;
        }
        let nodes = buf.element_locations().to_vec();
        for frame in 0..num_frames {
            buf.current_frame = frame;
            self.begin_frame(frame);
            if self.zig_zag {
                if self.direction.is_alternate() {
                    self.update_zig_zag_position(frame);
                }
            } else if !self.direction.is_alternate() {
                // So that the bars move
                self.update_flat_position(frame);
            }
            self.render_location_frame(frame, buf, &nodes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{frame, lit};
    use vixen_effects::testing;
    use vixen_effects::testing::{add_locations, grid, render_with};

    #[test]
    fn smoke() {
        testing::smoke_test(&DESCRIPTOR);
        testing::assert_produces_light(&DESCRIPTOR);
    }

    fn set_enum(p: &mut Params, name: &str, v: &str) {
        p.set(name, ParamValue::Enum(v.to_string()));
    }

    #[test]
    fn every_direction_and_bar_type_renders_in_strings_and_locations() {
        let dirs = [
            "Up", "Down", "Expand", "Compress", "Left", "Right", "HExpand", "HCompress", "AlternateUp", "AlternateDown",
            "AlternateLeft", "AlternateRight",
        ];
        let mut tree = ElementTree::new();
        let g = grid(&mut tree, "G", 8, 25);
        let located = grid(&mut tree, "L", 4, 10);
        add_locations(&mut tree, located);
        for bar in ["Flat", "ZigZag"] {
            for d in dirs {
                for rotate in [false, true] {
                    let edit = |p: &mut Params| {
                        set_enum(p, "BarType", bar);
                        set_enum(p, "Direction", d);
                        p.set("Highlight", ParamValue::Bool(true));
                        p.set("Show3D", ParamValue::Bool(true));
                        if rotate {
                            p.set("RotationAngle", ParamValue::Curve(Curve::from_points(vec![(0.0, 80.0), (100.0, 20.0)])));
                        }
                    };
                    let out = render_with(&DESCRIPTOR, &tree, &[g], 1000, edit);
                    assert!(!out.is_empty(), "{bar} {d} strings");
                    let edit2 = |p: &mut Params| {
                        set_enum(p, "TargetPositioning", "Locations");
                        set_enum(p, "BarType", bar);
                        set_enum(p, "Direction", d);
                        if rotate {
                            p.set("RotationAngle", ParamValue::Curve(Curve::from_points(vec![(0.0, 80.0), (100.0, 20.0)])));
                        }
                    };
                    let out = render_with(&DESCRIPTOR, &tree, &[located], 1000, edit2);
                    assert!(!out.is_empty(), "{bar} {d} locations");
                }
            }
        }
    }

    #[test]
    fn flat_up_with_three_colours_stacks_bars_top_to_bottom() {
        // 3 colours, repeat 1 on 30 pixels: bar height 30/3+1 = 11. Up draws row y at buffer row
        // ht - y - 1, so the first colour (red) is at the top, and the default position at frame 0 is 0.
        let f = frame(&DESCRIPTOR, 4, 30, 1000, 0, |_| {});
        // n = y + 0; colour index = ((y + 1) % 33) / 11, drawn at row 29 - y.
        // y = 0..9 is red (idx 0), y = 10..20 green (idx 1), y = 21..31 blue (idx 2).
        assert_eq!(f[0][29], RgbValue { r: 255, g: 0, b: 0 });
        assert_eq!(f[0][29 - 15], RgbValue { r: 0, g: 255, b: 0 });
        assert_eq!(f[0][29 - 25], RgbValue { r: 0, g: 0, b: 255 });
        // Every string shows the same bars.
        assert_eq!(f[0], f[3]);
    }

    #[test]
    fn bar_type_change_updates_movement_and_speed() {
        let mut p = DESCRIPTOR.new_params();
        set_enum(&mut p, "BarType", "ZigZag");
        param_changed("BarType", &mut p);
        assert_eq!(p.enum_name("MovementType"), "Speed");
        assert_eq!(p.curve("SpeedCurve").get_value(0.0), 10.0);
        set_enum(&mut p, "BarType", "Flat");
        param_changed("BarType", &mut p);
        assert_eq!(p.enum_name("MovementType"), "Iterations");
        assert_eq!(p.curve("SpeedCurve").get_value(0.0), 50.0);
    }

    #[test]
    fn zig_zag_visibility_hides_flat_settings() {
        let mut p = DESCRIPTOR.new_params();
        set_enum(&mut p, "BarType", "ZigZag");
        let visible: Vec<_> = DESCRIPTOR.visible_params(&p).into_iter().map(|i| DESCRIPTOR.params[i].name).collect();
        assert!(visible.contains(&"ZigZagAmplitude") && !visible.contains(&"Repeat") && !visible.contains(&"MovementType"));
        set_enum(&mut p, "BarType", "Flat");
        p.set("Highlight", ParamValue::Bool(true));
        set_enum(&mut p, "TargetPositioning", "Locations");
        let visible: Vec<_> = DESCRIPTOR.visible_params(&p).into_iter().map(|i| DESCRIPTOR.params[i].name).collect();
        assert!(visible.contains(&"HighlightPercentage") && !visible.contains(&"StringOrientation"));
    }

    #[test]
    fn zig_zag_tile_has_alternating_colour_and_gap_rows() {
        // Thickness 2, spacing 2, amplitude 0: flat bars of 2 rows in red, a 2 row gap, then green...
        // on a 40x40 element (scale 40): thickness 5% = 2, spacing 5% = 2.
        let f = frame(&DESCRIPTOR, 40, 40, 1000, 0, |p| {
            set_enum(p, "BarType", "ZigZag");
            p.set("ZigZagAmplitude", ParamValue::Int(0));
            p.set("ZigZagBarThickness", ParamValue::Int(5));
            p.set("ZigZagSpacing", ParamValue::Int(5));
            p.set("SpeedCurve", ParamValue::Curve(Curve::flat(0.0)));
        });
        // Each row is one colour across its whole width.
        let row = |y: usize| -> Vec<RgbValue> { f.iter().map(|s| s[y]).collect() };
        assert!(lit(&f) > 0);
        for y in 0..40 {
            let r = row(y);
            assert!(r.windows(2).all(|w| w[0] == w[1]), "row {y} not uniform");
        }
        // Gaps (transparent) render dark, so dark and lit rows both exist.
        let dark_rows = (0..40).filter(|y| row(*y)[0] == RgbValue::BLACK).count();
        assert!(dark_rows > 0 && dark_rows < 40);
    }
}
