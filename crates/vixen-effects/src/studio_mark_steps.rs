//! Beat-quantized movement: effects that step once per mark of a chosen collection instead of moving continuously
//! (Bars' Step on Marks, #67), or once every fixed number of milliseconds for sequences without beat marks (B55).

use vixen_core::{MarkCollection, TimeSpan};

/// When an effect steps: the times of the marks of a collection that fall inside it, as ms from the effect's start,
/// sorted (marks at the same time, which beat tracks often repeat, are one step), or a fixed interval.
pub struct MarkSteps {
    times: Vec<f64>,
    /// Step Every: `(interval, effect length)` in ms. Counted, not listed, so a tiny interval costs nothing.
    every: Option<(f64, f64)>,
}

impl MarkSteps {
    /// The step source of an effect's Step on Marks: every `every_ms` ms when that is above zero (the Step Every
    /// member), else the marks of `collection`.
    pub fn for_effect(collection: Option<&MarkCollection>, every_ms: i32, start: TimeSpan, span: TimeSpan) -> MarkSteps {
        if every_ms > 0 {
            MarkSteps::every(every_ms as f64, span)
        } else {
            MarkSteps::new(collection, start, span)
        }
    }

    /// A step every `interval_ms` ms from the effect's start, the first one interval in (so it starts at step zero),
    /// up to the effect's end.
    pub fn every(interval_ms: f64, span: TimeSpan) -> MarkSteps {
        MarkSteps { times: Vec::new(), every: Some((interval_ms.max(1.0), span.total_ms())) }
    }

    /// The marks of `collection` from `start` to the effect's end; marks before the effect never count, so it
    /// starts at step zero. No collection means no steps.
    pub fn new(collection: Option<&MarkCollection>, start: TimeSpan, span: TimeSpan) -> MarkSteps {
        let end = start + span;
        let mut times: Vec<f64> = collection
            .map(|c| c.marks.iter().filter(|m| m.start_time >= start && m.start_time <= end).map(|m| (m.start_time - start).total_ms()).collect())
            .unwrap_or_default();
        times.sort_by(f64::total_cmp);
        times.dedup();
        MarkSteps { times, every: None }
    }

    /// How many steps have passed `ms` into the effect; a step exactly at `ms` counts.
    pub fn passed(&self, ms: f64) -> usize {
        match self.every {
            Some((interval, end)) if ms >= 0.0 => (ms.min(end) / interval).floor() as usize,
            Some(_) => 0,
            None => self.times.partition_point(|t| *t <= ms),
        }
    }

    /// The position in one cycle, 0..1, after the marks passed at `ms`, each mark moving `1 / steps_per_cycle`.
    pub fn cycle_position(&self, ms: f64, steps_per_cycle: i32) -> f64 {
        let n = steps_per_cycle.max(1) as usize;
        (self.passed(ms) % n) as f64 / n as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vixen_core::Mark;

    fn collection(ms: &[i64]) -> MarkCollection {
        MarkCollection { marks: ms.iter().map(|m| Mark::new(TimeSpan::from_ms_i(*m))).collect(), ..MarkCollection::default() }
    }

    #[test]
    fn counts_only_the_marks_inside_the_effect() {
        let c = collection(&[500, 1000, 1500, 2500, 4000]);
        let s = MarkSteps::new(Some(&c), TimeSpan::from_ms_i(1000), TimeSpan::from_ms_i(2000));
        assert_eq!(s.passed(-1.0), 0);
        assert_eq!(s.passed(0.0), 1);
        assert_eq!(s.passed(499.0), 1);
        assert_eq!(s.passed(500.0), 2);
        assert_eq!(s.passed(10_000.0), 3);
    }

    #[test]
    fn marks_at_the_same_time_are_one_step() {
        let c = collection(&[100, 100, 200, 200, 200, 300]);
        let s = MarkSteps::new(Some(&c), TimeSpan::ZERO, TimeSpan::from_ms_i(1000));
        assert_eq!(s.passed(150.0), 1);
        assert_eq!(s.passed(250.0), 2);
        assert_eq!(s.passed(1000.0), 3);
    }

    #[test]
    fn cycle_position_wraps_after_a_full_cycle() {
        let c = collection(&[100, 200, 300, 400, 500]);
        let s = MarkSteps::new(Some(&c), TimeSpan::ZERO, TimeSpan::from_ms_i(1000));
        assert_eq!(s.cycle_position(50.0, 4), 0.0);
        assert_eq!(s.cycle_position(100.0, 4), 0.25);
        assert_eq!(s.cycle_position(399.0, 4), 0.75);
        assert_eq!(s.cycle_position(400.0, 4), 0.0);
        assert_eq!(s.cycle_position(500.0, 4), 0.25);
        assert_eq!(MarkSteps::new(None, TimeSpan::ZERO, TimeSpan::from_ms_i(1000)).cycle_position(500.0, 4), 0.0);
    }

    #[test]
    fn a_fixed_interval_steps_from_the_effect_start_to_its_end() {
        let s = MarkSteps::every(250.0, TimeSpan::from_ms_i(1000));
        assert_eq!(s.passed(-1.0), 0);
        assert_eq!(s.passed(0.0), 0);
        assert_eq!(s.passed(249.0), 0);
        assert_eq!(s.passed(250.0), 1);
        assert_eq!(s.passed(999.0), 3);
        assert_eq!(s.passed(1000.0), 4);
        assert_eq!(s.passed(5000.0), 4, "no steps past the effect's end");
        assert_eq!(s.cycle_position(500.0, 4), 0.5);
    }

    #[test]
    fn a_step_interval_replaces_the_collection_only_when_set() {
        let c = collection(&[100]);
        let span = TimeSpan::from_ms_i(1000);
        assert_eq!(MarkSteps::for_effect(Some(&c), 0, TimeSpan::ZERO, span).passed(500.0), 1);
        assert_eq!(MarkSteps::for_effect(Some(&c), 200, TimeSpan::ZERO, span).passed(500.0), 2);
        assert_eq!(MarkSteps::for_effect(None, 200, TimeSpan::ZERO, span).passed(500.0), 2);
        assert_eq!(MarkSteps::every(0.0, span).passed(10.0), 10, "intervals under 1 ms step every ms");
    }
}
