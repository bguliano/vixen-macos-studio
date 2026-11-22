//! Tests for Bars' Step on Marks (#67); the code is in the `bars-step-*` blocks of `bars.rs`.

#[cfg(test)]
mod tests {
    use crate::bars::DESCRIPTOR;
    use std::sync::atomic::AtomicBool;
    use vixen_effects::prelude::*;
    use vixen_effects::testing;

    fn beats(ms: &[i64]) -> MarkCollection {
        MarkCollection {
            name: "Beats".to_string(),
            marks: ms.iter().map(|m| Mark::new(TimeSpan::from_ms_i(*m))).collect(),
            ..MarkCollection::default()
        }
    }

    /// Renders 4 strings of 30 pixels for 2 s with the given marks, returning `[frame][string][pixel]` every 50 ms.
    fn frames(marks: &[MarkCollection], edit: impl FnOnce(&mut Params)) -> Vec<Vec<Vec<RgbValue>>> {
        let (strings, pixels, ms) = (4, 30, 2000);
        let mut tree = ElementTree::new();
        let g = testing::grid(&mut tree, "G", strings, pixels);
        let targets = [g];
        let cancel = AtomicBool::new(false);
        let ctx = RenderCtx {
            video_cache: None,
            tree: &tree,
            targets: &targets,
            start_time: TimeSpan::ZERO,
            time_span: TimeSpan::from_ms_i(ms),
            frame_ms: 50,
            marks,
            audio: None,
            module_data_root: None,
            cancel: &cancel,
        };
        let mut p = DESCRIPTOR.new_params();
        edit(&mut p);
        DESCRIPTOR.set_targets(&mut p, &ctx.target_info());
        let out = (DESCRIPTOR.render)(&ctx, &p);
        let elements = tree.distinct_elements(g);
        let mut res = vec![vec![vec![RgbValue::BLACK; pixels]; strings]; (ms / 50) as usize];
        for (i, e) in elements.iter().enumerate() {
            if let Some(nodes) = out.nodes_for(e) {
                if let IntentData::ArrayRgb(a) = &nodes[0].intent.data {
                    for (f, frame) in res.iter_mut().enumerate() {
                        frame[i / pixels][i % pixels] = a.at(TimeSpan::from_ms_i(f as i64 * 50));
                    }
                }
            }
        }
        res
    }

    fn stepping(p: &mut Params, marks: &MarkCollection, direction: &str) {
        p.set("Direction", ParamValue::Enum(direction.to_string()));
        p.set("StepToMarks", ParamValue::Bool(true));
        p.set("MarkCollectionId", ParamValue::Guid(marks.id));
        p.set("StepsPerCycle", ParamValue::Int(3));
    }

    /// The shift `k` that makes `later[i] == earlier[i + k]` along a string, if one does.
    fn shift(earlier: &[RgbValue], later: &[RgbValue]) -> Option<i32> {
        let n = earlier.len() as i32;
        (-15..=15).filter(|k| *k != 0).find(|k| {
            (0..n).filter(|i| (0..n).contains(&(i + k))).all(|i| later[i as usize] == earlier[(i + k) as usize])
        })
    }

    #[test]
    fn bars_hold_still_between_marks_and_move_one_band_per_mark() {
        let marks = beats(&[500, 1000, 1500]);
        let f = frames(std::slice::from_ref(&marks), |p| stepping(p, &marks, "Up"));
        // Three colours, repeat 1, 30 pixels: 11-pixel bands, and 3 steps per cycle moves one band a mark.
        for i in 1..10 {
            assert_eq!(f[i], f[0], "frame {i} moved before the first mark");
        }
        assert_ne!(f[10], f[0], "the first mark moves the bars");
        for i in 11..20 {
            assert_eq!(f[i], f[10], "frame {i} moved between marks");
        }
        assert_ne!(f[20], f[10]);
        assert_eq!(f[30], f[0], "three marks are one full cycle");
        assert_eq!(shift(&f[0][0], &f[10][0]).map(i32::abs), Some(11));
    }

    #[test]
    fn the_direction_decides_which_way_each_step_goes() {
        let marks = beats(&[500]);
        let up = frames(std::slice::from_ref(&marks), |p| stepping(p, &marks, "Up"));
        let down = frames(std::slice::from_ref(&marks), |p| stepping(p, &marks, "Down"));
        let up_shift = shift(&up[0][0], &up[10][0]).expect("Up steps");
        let down_shift = shift(&down[0][0], &down[10][0]).expect("Down steps");
        assert_eq!(up_shift, -down_shift);
    }

    #[test]
    fn without_marks_the_bars_stay_put() {
        let marks = beats(&[500, 1000]);
        let f = frames(&[], |p| stepping(p, &marks, "Up"));
        assert!(f.iter().all(|frame| *frame == f[0]));
    }

    #[test]
    fn step_settings_replace_the_movement_settings_while_on() {
        let mut tree = ElementTree::new();
        let g = testing::grid(&mut tree, "G", 2, 10);
        let targets = [g];
        let t = testing::target_info(&tree, &targets);
        let mut p = DESCRIPTOR.new_params();
        let shown = testing::visible_names(&DESCRIPTOR, &p, &t);
        assert!(shown.contains(&"StepToMarks") && shown.contains(&"Speed"));
        assert!(!shown.contains(&"MarkCollectionId") && !shown.contains(&"StepsPerCycle"));
        p.set("StepToMarks", ParamValue::Bool(true));
        let shown = testing::visible_names(&DESCRIPTOR, &p, &t);
        assert!(shown.contains(&"MarkCollectionId") && shown.contains(&"StepsPerCycle"));
        assert!(!shown.contains(&"Speed") && !shown.contains(&"SpeedCurve") && !shown.contains(&"MovementType"));
        p.set("BarType", ParamValue::Enum("ZigZag".to_string()));
        assert!(!testing::visible_names(&DESCRIPTOR, &p, &t).contains(&"StepToMarks"));
    }

    #[test]
    fn turning_step_on_marks_on_picks_a_collection() {
        let mut tree = ElementTree::new();
        let g = testing::grid(&mut tree, "G", 2, 10);
        let targets = [g];
        let marks = [beats(&[500])];
        let t = TargetInfo { marks: &marks, ..testing::target_info(&tree, &targets) };
        let mut p = DESCRIPTOR.new_params();
        DESCRIPTOR.set_param(&mut p, "StepToMarks", ParamValue::Bool(true), &t);
        assert_eq!(p.guid("MarkCollectionId"), marks[0].id);
    }
}
