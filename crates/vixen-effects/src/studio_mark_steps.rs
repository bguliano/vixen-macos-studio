//! Beat-quantized movement: effects that step once per mark of a chosen collection instead of moving continuously
//! (Bars' Step on Marks, #67).

use vixen_core::{MarkCollection, TimeSpan};

/// The times of the marks of a collection that fall inside an effect, as ms from the effect's start, sorted. Marks at the
/// same time (beat tracks often repeat one) are one step.
pub struct MarkSteps {
    times: Vec<f64>,
}

impl MarkSteps {
    /// The marks of `collection` from `start` to the effect's end; marks before the effect never count, so it
    /// starts at step zero. No collection means no steps.
    pub fn new(collection: Option<&MarkCollection>, start: TimeSpan, span: TimeSpan) -> MarkSteps {
        let end = start + span;
        let mut times: Vec<f64> = collection
            .map(|c| c.marks.iter().filter(|m| m.start_time >= start && m.start_time <= end).map(|m| (m.start_time - start).total_ms()).collect())
            .unwrap_or_default();
        times.sort_by(f64::total_cmp);
        times.dedup();
        MarkSteps { times }
    }

    /// How many marks have passed `ms` into the effect; a mark exactly at `ms` counts.
    pub fn passed(&self, ms: f64) -> usize {
        self.times.partition_point(|t| *t <= ms)
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
}
