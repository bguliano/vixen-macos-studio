//! Tests for Spiral's Step on Marks (B51) and its Step Every interval (B55); the code is in the `spiral-step-*` blocks of `spiral.rs`.

#[cfg(test)]
mod tests {
    use crate::spiral::DESCRIPTOR;
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

    /// Renders a 30 × 30 grid for 3 s with the given marks, returning every element's colour every 50 ms.
    fn frames(marks: &[MarkCollection], edit: impl FnOnce(&mut Params)) -> Vec<Vec<RgbValue>> {
        let (strings, pixels, ms) = (30, 30, 3000);
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
        let mut res = vec![vec![RgbValue::BLACK; elements.len()]; (ms / 50) as usize];
        for (i, e) in elements.iter().enumerate() {
            if let Some(nodes) = out.nodes_for(e) {
                if let IntentData::ArrayRgb(a) = &nodes[0].intent.data {
                    for (f, frame) in res.iter_mut().enumerate() {
                        frame[i] = a.at(TimeSpan::from_ms_i(f as i64 * 50));
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
    }

    #[test]
    fn the_spiral_holds_still_between_marks_and_turns_one_stripe_per_mark() {
        let marks = beats(&[500, 1000, 1500, 2000]);
        let f = frames(std::slice::from_ref(&marks), |p| stepping(p, &marks, "Forward"));
        // Three colours, 3 steps per cycle (the default): each mark turns the spiral one stripe.
        for i in 1..10 {
            assert_eq!(f[i], f[0], "frame {i} moved before the first mark");
        }
        assert_ne!(f[10], f[0], "the first mark turns the spiral");
        for i in 11..20 {
            assert_eq!(f[i], f[10], "frame {i} moved between marks");
        }
        assert_ne!(f[20], f[10]);
        assert_eq!(f[30], f[0], "three marks are one full color pattern");
        assert_eq!(f[40], f[10]);
        assert!(f[0].iter().any(|c| *c != RgbValue::BLACK), "the spiral is lit");
    }

    #[test]
    fn the_direction_decides_which_way_each_step_goes() {
        let marks = beats(&[500, 1000]);
        let forward = frames(std::slice::from_ref(&marks), |p| stepping(p, &marks, "Forward"));
        let backward = frames(std::slice::from_ref(&marks), |p| stepping(p, &marks, "Backwards"));
        // One stripe forward is two stripes back in a three-stripe pattern.
        assert_eq!(forward[10], backward[20]);
        assert_ne!(forward[10], backward[10]);
        let still = frames(std::slice::from_ref(&marks), |p| stepping(p, &marks, "None"));
        assert!(still.iter().all(|frame| *frame == still[0]));
    }

    #[test]
    fn a_step_is_a_share_of_the_repeated_pattern() {
        let marks = beats(&[500, 1000, 1500]);
        let f = frames(std::slice::from_ref(&marks), |p| {
            stepping(p, &marks, "Forward");
            p.set("Repeat", ParamValue::Int(2));
        });
        assert_ne!(f[10], f[0]);
        assert_eq!(f[30], f[0], "with Repeat 2, three marks turn the spiral one of its two patterns");
    }

    #[test]
    fn without_marks_the_spiral_stays_put() {
        let marks = beats(&[500, 1000]);
        let f = frames(&[], |p| stepping(p, &marks, "Forward"));
        assert!(f.iter().all(|frame| *frame == f[0]));
    }

    #[test]
    fn step_every_turns_the_spiral_on_a_fixed_interval_without_marks() {
        let marks = beats(&[500, 1000, 1500, 2000]);
        let on_marks = frames(std::slice::from_ref(&marks), |p| stepping(p, &marks, "Forward"));
        let every = frames(&[], |p| {
            stepping(p, &marks, "Forward");
            p.set("MarkCollectionId", ParamValue::Guid(Uuid::nil()));
            p.set("StepEveryMs", ParamValue::Int(500));
        });
        assert_eq!(every[..41], on_marks[..41]);
        assert_ne!(every[10], every[0]);
        assert_eq!(every[30], every[0], "three steps are one full color pattern");
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
        p.set("MovementType", ParamValue::Enum("Speed".to_string()));
        p.set("StepToMarks", ParamValue::Bool(true));
        let shown = testing::visible_names(&DESCRIPTOR, &p, &t);
        assert!(shown.contains(&"MarkCollectionId") && shown.contains(&"StepsPerCycle") && shown.contains(&"Direction"));
        assert!(shown.contains(&"StepEveryMs"));
        assert!(!shown.contains(&"Speed") && !shown.contains(&"SpeedCurve") && !shown.contains(&"MovementType"));
        p.set("StepEveryMs", ParamValue::Int(500));
        assert!(!testing::visible_names(&DESCRIPTOR, &p, &t).contains(&"MarkCollectionId"), "the interval replaces the collection");
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
