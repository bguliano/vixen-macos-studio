//! Port of VixenModules.Effect.Spiral.

use crate::generated::spiral as gen;
use vixen_effects::prelude::*;

pub static DESCRIPTOR: EffectDescriptor = EffectDescriptor {
    render,
    visibility: Some(visibility),
    param_changed_ctx: Some(studio_param_changed_ctx), // STUDIO: spiral-step-marks picks a mark collection
    supports_location: true,
    ported: true,
    ..gen::BASE
};

/// `InitAllAttributes`: orientation, then `UpdateDirectionAttribute`, then `UpdateMovementTypeAttribute`.
/// The movement rule runs last, so it decides whether `Speed` (the iteration count) is shown.
fn visibility(p: &Params, _t: &TargetInfo, v: &mut Visibility) {
    let speed = p.enum_name("MovementType") == "Speed";
    v.set("StringOrientation", p.enum_name("TargetPositioning") == "Strings");
    v.set("TargetPositioning", true);
    v.set("Speed", p.enum_name("Direction") != "None");
    v.set("SpeedCurve", speed);
    v.set("Speed", !speed);
    v.set("Direction", !speed);

    // STUDIO-BEGIN spiral-step-visibility: Step on Marks replaces the movement settings while it is on (B51)
    let steps = p.boolean("StepToMarks");
    v.set("StepToMarks", true);
    v.set("MarkCollectionId", steps);
    v.set("StepsPerCycle", steps);
    if steps {
        v.set("MovementType", false);
        v.set("Speed", false);
        v.set("SpeedCurve", false);
        v.set("Direction", true);
    }
    // STUDIO-END spiral-step-visibility
}

// STUDIO-BEGIN spiral-step-marks-select: turning Step on Marks on, or the sequence's collections changing, picks a collection
/// Like the C# effects that read marks: while stepping, keep the chosen collection if it still exists, else take the first.
fn studio_param_changed_ctx(c: &ParamChange, p: &mut Params) {
    if matches!(c.name, MARK_COLLECTIONS | "StepToMarks") {
        normalize_mark_selection(p, c.info, p.boolean("StepToMarks"), None, true);
    }
}
// STUDIO-END spiral-step-marks-select

#[derive(Copy, Clone, PartialEq)]
enum Direction {
    Forward,
    None,
    Backwards,
}

struct Spiral {
    direction: Direction,
    movement_speed: bool,
    colors: Vec<ColorGradient>,
    repeat: i32,
    speed: i32,
    blend: bool,
    grow: bool,
    shrink: bool,
    show_3d: bool,
    level_curve: Curve,
    speed_curve: Curve,
    thickness_curve: Curve,
    rotation_curve: Curve,
    position: f64,
    neg_position: bool,
    /// Step on Marks: the effect's marks and the steps per cycle, when on.
    studio_steps: Option<(vixen_effects::studio_mark_steps::MarkSteps, i32)>, // STUDIO: spiral-step-marks
}

fn render(ctx: &RenderCtx, p: &Params) -> EffectIntents {
    // Dividing by the empty list's count throws in the C#, and a pixel effect that throws has no intents at all. Rendering
    // by location checks for no colours first, so it finishes with every frame dark.
    if p.gradients("Colors").is_empty() && p.enum_name("TargetPositioning") != "Locations" {
        return EffectIntents::new();
    }
    let mut e = Spiral {
        direction: match p.enum_name("Direction") {
            "None" => Direction::None,
            "Backwards" => Direction::Backwards,
            _ => Direction::Forward,
        },
        movement_speed: p.enum_name("MovementType") == "Speed",
        colors: p.gradients("Colors"),
        repeat: p.int("Repeat"),
        speed: p.int("Speed"),
        blend: p.boolean("Blend"),
        grow: p.boolean("Grow"),
        shrink: p.boolean("Shrink"),
        show_3d: p.boolean("Show3D"),
        level_curve: p.curve("LevelCurve"),
        speed_curve: p.curve("SpeedCurve"),
        thickness_curve: p.curve("ThicknessCurve"),
        rotation_curve: p.curve("RotationCurve"),
        position: 0.0,
        neg_position: false,
        studio_steps: None, // STUDIO: spiral-step-marks
    };
    // STUDIO-BEGIN spiral-step-marks-render: the marks Step on Marks counts, taken once per render
    if p.boolean("StepToMarks") {
        let marks = ctx.mark_collection(p.guid("MarkCollectionId"));
        let steps = vixen_effects::studio_mark_steps::MarkSteps::new(marks, ctx.start_time, ctx.time_span);
        e.studio_steps = Some((steps, p.int("StepsPerCycle")));
    }
    // STUDIO-END spiral-step-marks-render
    render_pixel_effect(ctx, p, &mut e)
}

/// `SpiralFrameState`.
struct FrameState {
    color_count: i32,
    spiral_count: i32,
    delta_strands: i32,
    spiral_thickness: i32,
    adjust_rotation: f64,
    spiral_state: i32,
    level: f64,
}

#[inline]
fn positive_modulo(value: i32, modulus: i32) -> i32 {
    let r = value % modulus;
    if r < 0 {
        r + modulus
    } else {
        r
    }
}

impl Spiral {
    fn calculate_thickness(&self, interval_pos: f64) -> i32 {
        scale_curve_to_value(self.thickness_curve.get_value(interval_pos), 100.0, 1.0).round_ties_even() as i32
    }

    fn calculate_rotation(&self, interval_pos: f64) -> f64 {
        scale_curve_to_value(self.rotation_curve.get_value(interval_pos), 150.0, -150.0)
    }

    fn calculate_speed(&self, interval_pos: f64, info: &PixelInfo) -> f64 {
        scale_curve_to_value(self.speed_curve.get_value(interval_pos), 80.0, -80.0) * info.frame_time as f64 / 50.0
    }

    /// The position and spiral state, shared by both render paths. Returns `(position, spiral_state)`.
    fn position_state(&mut self, interval_pos: f64, interval_pos_factor: f64, wi: i32, info: &PixelInfo) -> (f64, i32) {
        // STUDIO-BEGIN spiral-step-marks-position: Step on Marks sets the rotation from the marks passed
        if let Some(state) = self.studio_step_state(interval_pos, wi, info) {
            return state;
        }
        // STUDIO-END spiral-step-marks-position
        let position;
        if !self.movement_speed {
            position = (interval_pos * self.speed as f64) % 1.0;
        } else {
            self.position += self.calculate_speed(interval_pos_factor, info) / 1000.0;
            if self.position < 0.0 {
                self.neg_position = true;
                position = -self.position;
            } else {
                self.neg_position = false;
                position = self.position;
            }
        }
        let mut spiral_state = (position * wi as f64 * 10.0) as i32;
        if self.movement_speed {
            if self.neg_position {
                spiral_state = -spiral_state;
            }
        } else if self.direction == Direction::Backwards {
            spiral_state = -spiral_state;
        } else if self.direction == Direction::None {
            spiral_state = 0;
        }
        (position, spiral_state)
    }

    // STUDIO-BEGIN spiral-step-marks-helper: each mark turns the spiral 1 / StepsPerCycle of one color pattern
    /// The position and spiral state Step on Marks gives, when it is on. One color pattern is a stripe per colour
    /// (`delta_strands` columns each); the steps keep adding up, so the spiral turns all the way round, gap included.
    fn studio_step_state(&self, interval_pos: f64, wi: i32, info: &PixelInfo) -> Option<(f64, i32)> {
        let (steps, per_cycle) = self.studio_steps.as_ref()?;
        let ms = (interval_pos * info.time_span.total_ms()).round();
        let spiral_count = self.colors.len() as i32 * self.repeat.max(1);
        if wi <= 0 || spiral_count <= 0 {
            return Some((0.0, 0));
        }
        let pattern = (wi / spiral_count * self.colors.len() as i32) as f64;
        let columns = (steps.passed(ms) as f64 * pattern / (*per_cycle).max(1) as f64).round() as i64 % wi as i64;
        let state = match self.direction {
            Direction::Forward => columns as i32 * 10,
            Direction::Backwards => -(columns as i32) * 10,
            Direction::None => 0,
        };
        Some((columns as f64 / wi as f64, state))
    }
    // STUDIO-END spiral-step-marks-helper

    fn thickness_state(&self, position: f64, spiral_gap: i32) -> i32 {
        let gap = spiral_gap as f64;
        if self.grow && self.shrink {
            (if position <= 0.5 { gap * (position * 2.0) } else { gap * ((1.0 - position) * 2.0) }) as i32
        } else if self.grow {
            (gap * position) as i32
        } else if self.shrink {
            (gap * (1.0 - position)) as i32
        } else {
            0
        }
    }

    /// `CreateSpiralFrameState`, used by the location path (which tolerates empty colour lists).
    fn frame_state(&mut self, frame: i32, info: &PixelInfo) -> FrameState {
        let wi = info.buffer_wi();
        let interval_pos = info.interval_position(frame);
        let interval_pos_factor = interval_pos * 100.0;
        let color_count = self.colors.len() as i32;
        let repeat = self.repeat.max(1);
        let spiral_count = color_count * repeat;
        let delta_strands = if spiral_count == 0 { 0 } else { wi / spiral_count };
        let mut spiral_thickness = if spiral_count == 0 { 0 } else { (delta_strands * self.calculate_thickness(interval_pos_factor) / 100) + 1 };
        let adjust_rotation = self.calculate_rotation(interval_pos_factor);
        let spiral_gap = delta_strands - spiral_thickness;
        let (position, spiral_state) = self.position_state(interval_pos, interval_pos_factor, wi, info);
        spiral_thickness += self.thickness_state(position, spiral_gap);
        let level = self.level_curve.get_value(interval_pos * 100.0) / 100.0;
        FrameState { color_count, spiral_count, delta_strands, spiral_thickness, adjust_rotation, spiral_state, level }
    }

    /// `TryGetSpiralPixelColor`.
    fn try_get_pixel_color(&self, x: i32, y: i32, s: &FrameState, info: &PixelInfo) -> Option<Hsv> {
        let (wi, ht) = (info.buffer_wi(), info.buffer_ht());
        if x < 0 || y < 0 || x >= wi || y >= ht {
            return None;
        }
        let row_offset = s.spiral_state / 10 + y * (s.adjust_rotation as i32) / ht;
        let target_strand = positive_modulo(x - row_offset, wi);
        for ns in (0..s.spiral_count).rev() {
            let mut thick = positive_modulo(target_strand - ns * s.delta_strands, wi);
            if thick >= s.spiral_thickness {
                continue;
            }
            if s.spiral_thickness > wi {
                thick += ((s.spiral_thickness - 1 - thick) / wi) * wi;
            }
            let color_idx = (ns % s.color_count) as usize;
            return Some(self.create_pixel_color(y, thick, color_idx, s, ht));
        }
        None
    }

    /// `CreateSpiralPixelColor`.
    fn create_pixel_color(&self, y: i32, thick: i32, color_idx: usize, s: &FrameState, ht: i32) -> Hsv {
        let color = if self.blend {
            self.colors[color_idx].get_color_at((ht - y - 1) as f64 / ht as f64)
        } else {
            self.colors[color_idx].get_color_at(thick as f64 / s.spiral_thickness as f64)
        };
        let mut hsv = Hsv::from_color(color);
        if self.show_3d {
            if self.direction != Direction::Backwards {
                hsv.set_v(((thick + 1) as f64 / s.spiral_thickness as f64) as f32 as f64);
            } else {
                hsv.set_v(((s.spiral_thickness - thick) as f64 / s.spiral_thickness as f64) as f32 as f64);
            }
        }
        hsv.set_v(hsv.v * s.level);
        hsv
    }
}

impl PixelEffect for Spiral {
    fn setup_render(&mut self, _info: &PixelInfo) {
        self.position = 0.0;
        self.neg_position = false;
    }

    fn render_effect(&mut self, frame: i32, info: &PixelInfo, buf: &mut PixelFrameBuffer) {
        let (wi, ht) = (info.buffer_wi(), info.buffer_ht());
        let color_count = self.colors.len() as i32;
        let spiral_count = color_count.wrapping_mul(self.repeat);
        // The C# divides by `spiralCount`, which throws when it is zero.
        if wi <= 0 || ht <= 0 || spiral_count <= 0 {
            return;
        }
        let interval_pos = info.interval_position(frame);
        let interval_pos_factor = interval_pos * 100.0;
        let delta_strands = wi / spiral_count;
        let mut spiral_thickness = (delta_strands * self.calculate_thickness(interval_pos_factor) / 100) + 1;
        let adjust_rotation = self.calculate_rotation(interval_pos_factor) as i32;
        let spiral_gap = delta_strands - spiral_thickness;
        let (position, spiral_state) = self.position_state(interval_pos, interval_pos_factor, wi, info);
        spiral_thickness += self.thickness_state(position, spiral_gap);
        let level = self.level_curve.get_value(interval_pos * 100.0) / 100.0;

        for ns in 0..spiral_count {
            let strand_base = ns * delta_strands;
            let color_idx = (ns % color_count) as usize;
            for thick in 0..spiral_thickness.max(0) {
                let strand = (strand_base + thick) % wi;
                // Without blending the colour depends only on the strand, so sample it once.
                let flat_color = if self.blend { Color::EMPTY } else { self.colors[color_idx].get_color_at(thick as f64 / spiral_thickness as f64) };
                for y in 0..ht {
                    let mut x = (strand + (spiral_state / 10) + (y * adjust_rotation / ht)) % wi;
                    if x < 0 {
                        x += wi;
                    }
                    let color = if self.blend { self.colors[color_idx].get_color_at((ht - y - 1) as f64 / ht as f64) } else { flat_color };
                    let mut hsv = Hsv::from_color(color);
                    if self.show_3d {
                        if self.direction != Direction::Backwards {
                            hsv.set_v(((thick + 1) as f64 / spiral_thickness as f64) as f32 as f64);
                        } else {
                            hsv.set_v(((spiral_thickness - thick) as f64 / spiral_thickness as f64) as f32 as f64);
                        }
                    }
                    hsv.set_v(hsv.v * level);
                    buf.set_pixel_hsv(x, y, hsv);
                }
            }
        }
    }

    fn render_effect_by_location(&mut self, num_frames: i32, info: &PixelInfo, buf: &mut PixelLocationFrameBuffer) {
        if info.buffer_wi() == 0 || info.buffer_ht() == 0 || self.colors.is_empty() {
            return;
        }
        let locs = buf.element_locations().to_vec();
        for frame in 0..num_frames {
            buf.current_frame = frame;
            let state = self.frame_state(frame, info);
            if state.spiral_count == 0 || state.spiral_thickness == 0 {
                continue;
            }
            for l in &locs {
                let mut local_y = ((info.buffer_ht_offset() - l.y) + (info.buffer_ht() - 1 + info.buffer_ht_offset())).abs();
                local_y -= info.buffer_ht_offset();
                let local_x = l.x - info.buffer_wi_offset();
                if let Some(hsv) = self.try_get_pixel_color(local_x, local_y, &state, info) {
                    buf.set_pixel_hsv(l.x, l.y, hsv);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixen_effects::testing;

    #[test]
    fn smoke() {
        testing::smoke_test(&DESCRIPTOR);
        testing::assert_produces_light(&DESCRIPTOR);
    }

    #[test]
    fn speed_mode_shows_speed_curve_and_hides_direction() {
        let mut p = DESCRIPTOR.new_params();
        p.set("MovementType", ParamValue::Enum("Speed".to_string()));
        let visible: Vec<_> = DESCRIPTOR.visible_params(&p).into_iter().map(|i| DESCRIPTOR.params[i].name).collect();
        assert!(visible.contains(&"SpeedCurve"));
        assert!(!visible.contains(&"Direction"));
        assert!(!visible.contains(&"Speed"));
    }

    fn spiral() -> Spiral {
        Spiral {
            direction: Direction::None,
            movement_speed: false,
            colors: vec![ColorGradient::solid(Color::RED)],
            repeat: 1,
            speed: 1,
            blend: false,
            grow: false,
            shrink: false,
            show_3d: false,
            level_curve: Curve::flat100(),
            speed_curve: Curve::flat(70.0),
            thickness_curve: Curve::flat(60.0),
            rotation_curve: Curve::flat(50.0),
            position: 0.0,
            neg_position: false,
            studio_steps: None, // STUDIO: spiral-step-marks
        }
    }

    #[test]
    fn one_strand_covers_thickness_columns() {
        // 10 wide: delta = 10, thickness curve 60 -> 60, thickness = 10 * 60 / 100 + 1 = 7, no rotation.
        let info = crate::test_util::info(10, 4, 1000);
        let mut e = spiral();
        let s = e.frame_state(0, &info);
        assert_eq!(s.spiral_thickness, 7);
        for x in 0..10 {
            let lit = e.try_get_pixel_color(x, 0, &s, &info).is_some();
            assert_eq!(lit, x < 7, "x = {x}");
        }
    }
}
