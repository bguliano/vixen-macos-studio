//! Port of `EffectRasterizer` and `IntentRasterizer`: draws an effect's intents into the picture shown on its
//! timeline block. Thumbnails are computed in Rust off the main thread, and Swift caches them as images.

use vixen_core::{Color, EffectIntents, Intent, IntentData, IntentNode, TimeSpan, Uuid};

/// Premultiplied RGBA8, top row first, ready for a `CGImage`.
#[derive(Clone, Debug)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Pixels {
    pub fn transparent(width: u32, height: u32) -> Pixels {
        Pixels { width, height, rgba: vec![0; width as usize * height as usize * 4] }
    }

    /// Source-over a straight-alpha colour onto one pixel.
    #[inline]
    fn blend(&mut self, x: usize, y: usize, c: [f32; 4]) {
        let i = (y * self.width as usize + x) * 4;
        let px = &mut self.rgba[i..i + 4];
        let a = c[3];
        let inv = 1.0 - a;
        px[0] = (c[0] * a + px[0] as f32 * inv + 0.5) as u8;
        px[1] = (c[1] * a + px[1] as f32 * inv + 0.5) as u8;
        px[2] = (c[2] * a + px[2] as f32 * inv + 0.5) as u8;
        px[3] = (a * 255.0 + px[3] as f32 * inv + 0.5) as u8;
    }
}

/// What to draw: the visible part of one effect.
pub struct ThumbnailRequest<'a> {
    pub intents: &'a EffectIntents,
    /// `effect.TargetNodes.GetElements()`, in order. Each becomes a band of the picture.
    pub elements: &'a [Uuid],
    pub effect_span: TimeSpan,
    pub visible_start: TimeSpan,
    pub visible_end: TimeSpan,
    /// How wide the whole effect is at the current zoom, in pixels.
    pub overall_width: f64,
    pub width: u32,
    pub height: u32,
}

/// `EffectRasterizer.Rasterize`.
pub fn rasterize(r: &ThumbnailRequest) -> Pixels {
    let mut out = Pixels::transparent(r.width, r.height);
    if r.width == 0 || r.height == 0 || r.elements.is_empty() {
        return out;
    }
    let width = r.width as f64;
    let height = r.height as f64;

    if r.intents.map.is_empty() {
        return out;
    }

    // Partition the target list itself. It can contain many more entries than the rendered intents (VIX-4032).
    let row_count = r.elements.len().min((height / 2.0) as usize + 1);
    let height_per_element = height / row_count as f64;
    for row_index in 0..row_count {
        let y = row_index as f64 * height_per_element;
        let first_element = row_index * r.elements.len() / row_count;
        let end_element = (row_index + 1) * r.elements.len() / row_count;
        let element_intents = r.elements[first_element..end_element].iter().find_map(|element| {
            r.intents.nodes_for(element).filter(|n| has_visible_intent(n.iter(), r.visible_start, r.visible_end))
        });
        if let Some(nodes) = element_intents {
            // Determine whether parallel intents are used on this element.
            let mut stack: Vec<Vec<&IntentNode>> = vec![vec![&nodes[0]]];
            for node in &nodes[1..] {
                match stack.iter_mut().find(|t| node.start >= t.last().map(|l| l.end).unwrap_or(TimeSpan::MIN)) {
                    Some(t) => t.push(node),
                    None => stack.push(vec![node]),
                }
            }
            let mut skip = 0;
            // A base or minimum level intent, as chase, spin and twinkle use, draws at full size so the
            // pulses can be drawn over the top.
            if stack.len() > 1
                && stack[0].len() == 1
                && stack[0][0].intent.span == r.effect_span
                && stack[1][0].intent.span != r.effect_span
            {
                draw_intent(&mut out, &stack[0][0].intent, (0.0, y as f32, width as f32, height_per_element as f32), r.visible_start, stack[0][0].intent.span);
                skip = 1;
            }

            // Each displayed lane needs its own pixel band. Preserve the base layer drawn above.
            let lane_count = stack.len() - skip;
            let displayed_lanes = lane_count.min((height_per_element as usize).max(1));
            let h = height_per_element as f32 / displayed_lanes as f32;
            for lane_index in 0..displayed_lanes {
                let first_lane = skip + lane_index * lane_count / displayed_lanes;
                let end_lane = skip + (lane_index + 1) * lane_count / displayed_lanes;
                let mut row = &stack[first_lane];
                let mut i = first_lane;
                while i + 1 < end_lane && !has_visible_intent(row.iter().copied(), r.visible_start, r.visible_end) {
                    row = &stack[i + 1];
                    i += 1;
                }
                for node in row {
                    if node.end < r.visible_start || node.start > r.visible_end {
                        continue;
                    }
                    let visible_intent_start = if node.start < r.visible_start { r.visible_start - node.start } else { TimeSpan::ZERO };
                    let visible_intent_end = if node.end > r.visible_end { r.visible_end - node.start } else { node.intent.span };
                    let mut start_x = r.overall_width * percentage(node.start, r.effect_span);
                    let mut width_x = r.overall_width * percentage(node.intent.span, r.effect_span);
                    width_x *= (visible_intent_end.total_ms() - visible_intent_start.total_ms()) / node.intent.span.total_ms();
                    if visible_intent_start == TimeSpan::ZERO {
                        start_x -= r.overall_width * percentage(r.visible_start, r.effect_span);
                    } else {
                        start_x = 0.0;
                    }
                    draw_intent(
                        &mut out,
                        &node.intent,
                        (start_x as f32, y as f32 + h * lane_index as f32, width_x as f32, h),
                        visible_intent_start,
                        visible_intent_end,
                    );
                }
            }
        }
    }
    out
}

/// `HasVisibleIntent`: whether any intent overlaps the visible interval.
fn has_visible_intent<'n>(intents: impl IntoIterator<Item = &'n IntentNode>, visible_start: TimeSpan, visible_end: TimeSpan) -> bool {
    intents.into_iter().any(|n| n.end > visible_start && n.start < visible_end)
}

#[inline]
fn percentage(offset: TimeSpan, total: TimeSpan) -> f64 {
    offset.ticks() as f64 / total.ticks() as f64
}

/// The colour an intent shows at a time: `FullColorWithAlpha` of its state.
fn color_at(intent: &Intent, t: TimeSpan) -> Color {
    match intent.state_at(t) {
        vixen_core::StateValue::Lighting(v) => v.full_color_with_alpha(),
        vixen_core::StateValue::Rgb(v) => v.full_color_with_alpha(),
        vixen_core::StateValue::Discrete(v) => v.full_color_with_alpha(),
        _ => Color::EMPTY,
    }
}

const ONE_TICK: TimeSpan = TimeSpan::from_ticks(1);

/// `IntentRasterizer.Rasterize`. Fixture range and command intents draw nothing, since the dispatch has no
/// handler for them.
fn draw_intent(out: &mut Pixels, intent: &Intent, rect: (f32, f32, f32, f32), start_offset: TimeSpan, end_time: TimeSpan) {
    if rect.2.abs() < f32::EPSILON || rect.3.abs() < f32::EPSILON {
        return;
    }
    match &intent.data {
        IntentData::ArrayRgb(_) | IntentData::ArrayLighting(_) | IntentData::ArrayDiscrete(_) => {
            draw_static_array(out, intent, rect, start_offset, end_time);
        }
        IntentData::Lighting { .. }
        | IntentData::StaticLighting(_)
        | IntentData::Rgb { .. }
        | IntentData::StaticRgb(_)
        | IntentData::Discrete { .. }
        | IntentData::StaticDiscrete(_) => {
            let start = color_at(intent, start_offset);
            let end = color_at(intent, end_time - if end_time < intent.span { TimeSpan::ZERO } else { ONE_TICK });
            draw_gradient(out, start, end, rect);
        }
        IntentData::Range { .. } | IntentData::StaticRange(_) | IntentData::Command(_) => {}
    }
}

/// `DrawStaticArrayIntent`: sampled values, drawn as one gradient per 50 ms.
fn draw_static_array(out: &mut Pixels, intent: &Intent, rect: (f32, f32, f32, f32), start_offset: TimeSpan, timespan: TimeSpan) {
    let span_ms = (timespan - start_offset).total_ms();
    let n_chunks = 1 + (span_ms as i64) / 50;
    if n_chunks <= 0 {
        return;
    }
    let rect_width = rect.2 / n_chunks as f32;
    let mut ts_start = start_offset;
    for i in 1..=n_chunks {
        let ts_end = TimeSpan::from_ms(span_ms / n_chunks as f64 * i as f64) + start_offset;
        let start = color_at(intent, ts_start);
        let end = color_at(intent, ts_end);
        draw_gradient(out, start, end, (rect.0 + (i - 1) as f32 * rect_width, rect.1, rect_width, rect.3));
        ts_start = ts_end;
    }
}

#[inline]
fn to_float(c: Color) -> [f32; 4] {
    [c.r as f32, c.g as f32, c.b as f32, c.a as f32 / 255.0]
}

/// `DrawGradient`: a horizontal `LinearGradientBrush` with gamma correction.
fn draw_gradient(out: &mut Pixels, start: Color, end: Color, rect: (f32, f32, f32, f32)) {
    let (x, y, w, h) = rect;
    if !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite() || !(x + w).is_finite() || !(y + h).is_finite()
        || w <= 0.0 || h <= 0.0 || x + w <= x || y + h <= y
    {
        return;
    }
    // GDI+ fills a pixel when its centre is inside the rectangle.
    let x0 = (x - 0.5).ceil().max(0.0) as usize;
    let x1 = ((x + w - 0.5).ceil().max(0.0) as usize).min(out.width as usize);
    let y0 = (y - 0.5).ceil().max(0.0) as usize;
    let y1 = ((y + h - 0.5).ceil().max(0.0) as usize).min(out.height as usize);
    if x0 >= x1 || y0 >= y1 {
        // A band or chunk narrower than a pixel still shows, so very short effects and dense sampling don't vanish.
        if x1 == x0 && (x0 as u32) < out.width && y0 < y1 {
            let c = to_float(start);
            for py in y0..y1 {
                out.blend(x0, py, c);
            }
        }
        return;
    }
    if start == end {
        let c = to_float(start);
        for py in y0..y1 {
            for px in x0..x1 {
                out.blend(px, py, c);
            }
        }
        return;
    }
    // The original widens the gradient a little to avoid edge artefacts.
    let offset = x * 0.004;
    let grad_x = x - offset;
    let grad_right = grad_x + (w + 2.0 * offset);
    let grad_w = grad_right - grad_x;
    // A gradient too thin for the brush fills with the start colour instead (VIX-4032).
    if !grad_x.is_finite() || !grad_right.is_finite() || !grad_w.is_finite() || grad_w < 1.0 / 256.0 {
        let c = to_float(start);
        for py in y0..y1 {
            for px in x0..x1 {
                out.blend(px, py, c);
            }
        }
        return;
    }
    let (s, e) = (to_float(start), to_float(end));
    // Gamma correction: interpolate at 2.2 and convert back.
    let lin = |v: f32| (v / 255.0).powf(2.2);
    let (sl, el) = ([lin(s[0]), lin(s[1]), lin(s[2])], [lin(e[0]), lin(e[1]), lin(e[2])]);
    for px in x0..x1 {
        let t = ((px as f32 + 0.5 - grad_x) / grad_w).clamp(0.0, 1.0);
        let c = [
            (sl[0] + (el[0] - sl[0]) * t).powf(1.0 / 2.2) * 255.0,
            (sl[1] + (el[1] - sl[1]) * t).powf(1.0 / 2.2) * 255.0,
            (sl[2] + (el[2] - sl[2]) * t).powf(1.0 / 2.2) * 255.0,
            s[3] + (e[3] - s[3]) * t,
        ];
        for py in y0..y1 {
            out.blend(px, py, c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixen_core::{FrameArray, Intent, RgbValue};

    fn ms(v: i64) -> TimeSpan {
        TimeSpan::from_ms_i(v)
    }

    fn px(p: &Pixels, x: usize, y: usize) -> [u8; 4] {
        let i = (y * p.width as usize + x) * 4;
        [p.rgba[i], p.rgba[i + 1], p.rgba[i + 2], p.rgba[i + 3]]
    }

    fn request<'a>(intents: &'a EffectIntents, elements: &'a [Uuid], span: i64, w: u32, h: u32) -> ThumbnailRequest<'a> {
        ThumbnailRequest {
            intents,
            elements,
            effect_span: ms(span),
            visible_start: TimeSpan::ZERO,
            visible_end: ms(span),
            overall_width: w as f64,
            width: w,
            height: h,
        }
    }

    #[test]
    fn a_static_effect_fills_its_block() {
        let id = Uuid::new_v4();
        let mut e = EffectIntents::new();
        e.add_intent_for_element(id, Intent::static_lighting(Color::RED, 1.0, ms(1000)), TimeSpan::ZERO);
        let ids = [id];
        let p = rasterize(&request(&e, &ids, 1000, 100, 20));
        assert_eq!(px(&p, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&p, 99, 19), [255, 0, 0, 255]);
    }

    #[test]
    fn half_brightness_shows_through_alpha() {
        let id = Uuid::new_v4();
        let mut e = EffectIntents::new();
        e.add_intent_for_element(id, Intent::static_lighting(Color::WHITE, 0.5, ms(1000)), TimeSpan::ZERO);
        let ids = [id];
        let p = rasterize(&request(&e, &ids, 1000, 10, 10));
        let c = px(&p, 5, 5);
        assert!((c[3] as i32 - 127).abs() <= 1, "{c:?}");
    }

    #[test]
    fn a_ramp_grows_brighter_left_to_right() {
        let id = Uuid::new_v4();
        let mut e = EffectIntents::new();
        e.add_intent_for_element(id, Intent::lighting(Color::WHITE, Color::WHITE, 0.0, 1.0, ms(1000)), TimeSpan::ZERO);
        let ids = [id];
        let p = rasterize(&request(&e, &ids, 1000, 100, 4));
        assert!(px(&p, 5, 1)[3] < px(&p, 50, 1)[3]);
        assert!(px(&p, 50, 1)[3] < px(&p, 95, 1)[3]);
    }

    #[test]
    fn elements_become_bands() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut e = EffectIntents::new();
        e.add_intent_for_element(a, Intent::static_lighting(Color::RED, 1.0, ms(100)), TimeSpan::ZERO);
        e.add_intent_for_element(b, Intent::static_lighting(Color::BLUE, 1.0, ms(100)), TimeSpan::ZERO);
        let ids = [a, b];
        let p = rasterize(&request(&e, &ids, 100, 10, 20));
        assert_eq!(px(&p, 5, 2), [255, 0, 0, 255]);
        assert_eq!(px(&p, 5, 17), [0, 0, 255, 255]);
    }

    #[test]
    fn sampled_pixel_effects_show_their_frames() {
        let id = Uuid::new_v4();
        let mut values = vec![RgbValue { r: 255, g: 0, b: 0 }; 10];
        values.extend(vec![RgbValue { r: 0, g: 0, b: 255 }; 10]);
        let mut e = EffectIntents::new();
        e.add_intent_for_element(
            id,
            Intent::new(IntentData::ArrayRgb(FrameArray::from_vec(values, ms(50))), ms(1000)),
            TimeSpan::ZERO,
        );
        let ids = [id];
        let p = rasterize(&request(&e, &ids, 1000, 200, 10));
        assert!(px(&p, 10, 5)[0] > 200, "{:?}", px(&p, 10, 5));
        assert!(px(&p, 190, 5)[2] > 200, "{:?}", px(&p, 190, 5));
    }

    #[test]
    fn a_base_intent_draws_full_size_under_the_pulses() {
        let id = Uuid::new_v4();
        let mut e = EffectIntents::new();
        // A dim base across the whole effect, and a bright pulse in part of it, as Chase does.
        e.add_intent_for_element(id, Intent::static_lighting(Color::RED, 0.25, ms(1000)), TimeSpan::ZERO);
        e.add_intent_for_element(id, Intent::static_lighting(Color::BLUE, 1.0, ms(100)), ms(200));
        let ids = [id];
        let p = rasterize(&request(&e, &ids, 1000, 100, 20));
        assert!(px(&p, 5, 10)[0] > 0, "base visible outside the pulse: {:?}", px(&p, 5, 10));
        assert!(px(&p, 25, 10)[2] > 200, "pulse on top: {:?}", px(&p, 25, 10));
    }

    #[test]
    fn only_the_visible_part_is_drawn() {
        let id = Uuid::new_v4();
        let mut e = EffectIntents::new();
        e.add_intent_for_element(id, Intent::static_lighting(Color::RED, 1.0, ms(100)), ms(0));
        e.add_intent_for_element(id, Intent::static_lighting(Color::BLUE, 1.0, ms(100)), ms(100));
        let ids = [id];
        let r = ThumbnailRequest {
            intents: &e,
            elements: &ids,
            effect_span: ms(200),
            visible_start: ms(100),
            visible_end: ms(200),
            overall_width: 200.0,
            width: 100,
            height: 4,
        };
        let p = rasterize(&r);
        assert_eq!(px(&p, 50, 2), [0, 0, 255, 255]);
    }

    #[test]
    fn empty_requests_are_safe() {
        let e = EffectIntents::new();
        assert_eq!(rasterize(&request(&e, &[], 100, 10, 10)).rgba.iter().sum::<u8>(), 0);
        assert!(rasterize(&request(&e, &[Uuid::new_v4()], 100, 0, 0)).rgba.is_empty());
    }

    fn static_array(c: RgbValue, span: TimeSpan) -> Intent {
        Intent::new(IntentData::ArrayRgb(FrameArray::from_vec(vec![c, c], span)), span)
    }

    /// Upstream `EffectRasterizerDrawingTests.DrawCanvasContent_DisplaysVisibleTargetsInDenseThumbnail` (VIX-4032):
    /// sparse targets with overlapping intents show in their target bands.
    #[test]
    fn draw_canvas_content_displays_visible_targets_in_dense_thumbnail() {
        let targets: Vec<Uuid> = (0..40).map(|_| Uuid::new_v4()).collect();
        let (visible_start, visible_end) = (ms(100), ms(300));
        let visible_duration = visible_end - visible_start;
        let mut e = EffectIntents::new();
        e.add_intent_for_element(targets[0], static_array(RgbValue { r: 0, g: 0, b: 255 }, visible_start), TimeSpan::ZERO);
        let dense = static_array(RgbValue { r: 255, g: 0, b: 0 }, visible_duration);
        for _ in 0..215 {
            e.add_intent_for_element(targets[2], dense.clone(), visible_start);
        }
        e.add_intent_for_element(targets[39], static_array(RgbValue { r: 0, g: 255, b: 0 }, visible_duration), visible_start);
        let r = ThumbnailRequest {
            intents: &e,
            elements: &targets,
            effect_span: ms(400),
            visible_start,
            visible_end,
            overall_width: 200.0,
            width: 100,
            height: 19,
        };
        let p = rasterize(&r);
        assert_eq!(px(&p, 20, 0), [255, 0, 0, 255]);
        assert_eq!(px(&p, 20, 9)[3], 0);
        assert_eq!(px(&p, 20, 18), [0, 255, 0, 255]);
    }

    /// Upstream `IntentRasterizerDrawingTests.Rasterize_CapturedButterflyBounds_ContinuesDrawing` (VIX-4032).
    #[test]
    fn captured_butterfly_bounds_continue_drawing() {
        let mut p = Pixels::transparent(320, 24);
        let captured = (297.225464, 7.57701015, 4.71786451, 8.1524071e-5);
        draw_gradient(&mut p, Color::RED, Color::BLUE, captured);
        draw_gradient(&mut p, Color::from_argb_u32(0x5B0505FE), Color::from_argb_u32(0x580505FF), captured);
        draw_gradient(&mut p, Color::RED, Color::BLUE, (10.0, 10.0, 100.0, 8.0));
        assert_ne!(px(&p, 30, 13)[3], 0);
        assert_ne!(px(&p, 20, 13), px(&p, 90, 13));
    }

    /// Upstream `IntentRasterizerDrawingTests.Rasterize_TinyAndInvalidBounds_ContinuesDrawing` (VIX-4032).
    #[test]
    fn tiny_and_invalid_bounds_continue_drawing() {
        let mut p = Pixels::transparent(40, 20);
        draw_gradient(&mut p, Color::RED, Color::BLUE, (0.0, 0.0, 1e-8, 4.0));
        draw_gradient(&mut p, Color::RED, Color::BLUE, (f32::NAN, 0.0, 10.0, 4.0));
        draw_gradient(&mut p, Color::RED, Color::BLUE, (0.0, 0.0, -1.0, 4.0));
        draw_gradient(&mut p, Color::LIME, Color::LIME, (5.0, 8.0, 20.0, 5.0));
        assert_eq!(px(&p, 30, 1)[3], 0);
        assert_eq!(px(&p, 10, 10), [0, 255, 0, 255]);
    }

    #[test]
    fn a_non_finite_rectangle_draws_nothing() {
        let mut p = Pixels::transparent(10, 10);
        draw_gradient(&mut p, Color::RED, Color::BLUE, (f32::NAN, 0.0, 10.0, 4.0));
        draw_gradient(&mut p, Color::RED, Color::BLUE, (0.0, 0.0, f32::INFINITY, 4.0));
        assert!(p.rgba.iter().all(|&b| b == 0));
    }
}
