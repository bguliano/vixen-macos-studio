//! How much of a preview is lit across a sequence span, from the rendered schedule's exact intensities rather than a
//! decoded video, and the stretches where it goes dark ("dead air"), so an agent can check a show before anyone
//! watches a preview of it. A light's level is its brightest channel, as the preview draws it, so a red tree is lit.
use super::Renderer;
use serde_json::{json, Value};
use std::collections::HashSet;
use uuid::Uuid;

/// When a frame counts as dark: fewer than `min_lit` of the lights at `level` or brighter, for at least `min_ms`.
#[derive(Clone, Copy)]
pub struct Thresholds {
    /// A light's level (0 to 1) at which it counts as lit.
    pub level: f32,
    /// The share of the lights (0 to 1) that must be lit for a frame not to be dark.
    pub min_lit: f32,
    /// The shortest dark run reported.
    pub min_ms: f64,
}

impl Thresholds {
    pub const DEFAULT: Thresholds = Thresholds { level: 0.1, min_lit: 0.05, min_ms: 1000.0 };

    pub fn to_json(self) -> Value {
        json!({"level": self.level, "minLit": self.min_lit, "minMs": self.min_ms})
    }
}

/// One group of lights measured on its own: the whole preview, or one element's lights in it.
pub struct Group {
    pub id: Option<Uuid>,
    pub name: String,
    /// Indices into the renderer's linked lights.
    lights: Vec<usize>,
    /// The share of the group's lights lit in each frame.
    pub series: Vec<f32>,
}

impl Group {
    pub fn whole(renderer: &Renderer) -> Self {
        Group { id: None, name: String::new(), lights: (0..renderer.lights.len()).collect(), series: Vec::new() }
    }

    /// The preview's lights that belong to `elements` (an element and everything under it).
    pub fn of(renderer: &Renderer, id: Uuid, name: String, elements: &HashSet<Uuid>) -> Self {
        let lights = renderer.lights.iter().enumerate().filter(|(_, l)| elements.contains(&l.element)).map(|(i, _)| i).collect();
        Group { id: Some(id), name, lights, series: Vec::new() }
    }

    pub fn lights(&self) -> usize {
        self.lights.len()
    }
}

/// Evaluates the schedule at each of `times` and fills every group's series. A group with no lights is never lit.
pub fn measure(renderer: &mut Renderer, times: &[f64], level: f32, groups: &mut [Group]) {
    let mut levels = Vec::new();
    for group in groups.iter_mut() {
        group.series = Vec::with_capacity(times.len());
    }
    for &time in times {
        renderer.levels(time, &mut levels);
        for group in groups.iter_mut() {
            let lit = group.lights.iter().filter(|&&i| levels[i] >= level).count();
            group.series.push(if group.lights.is_empty() { 0.0 } else { lit as f32 / group.lights.len() as f32 });
        }
    }
}

/// A run of dark frames: from the first dark frame's time to the next lit frame's (or the span's end).
#[derive(Debug, PartialEq)]
pub struct Stretch {
    pub start_ms: f64,
    pub end_ms: f64,
    /// The mean and highest lit share inside the run.
    pub mean: f32,
    pub peak: f32,
}

/// The dark runs in `series`, sampled at `times`, at least `thresholds.min_ms` long. `end_ms` closes a run that lasts
/// to the last frame.
pub fn stretches(times: &[f64], end_ms: f64, series: &[f32], thresholds: Thresholds) -> Vec<Stretch> {
    let mut found = Vec::new();
    let mut run: Option<usize> = None;
    for i in 0..=series.len() {
        let dark = series.get(i).is_some_and(|&lit| lit < thresholds.min_lit);
        match (run, dark) {
            (None, true) => run = Some(i),
            (Some(first), false) => {
                let end = times.get(i).copied().unwrap_or(end_ms);
                if end - times[first] >= thresholds.min_ms {
                    let part = &series[first..i];
                    found.push(Stretch {
                        start_ms: times[first],
                        end_ms: end,
                        mean: part.iter().sum::<f32>() / part.len() as f32,
                        peak: part.iter().copied().fold(0.0, f32::max),
                    });
                }
                run = None;
            }
            _ => {}
        }
    }
    found
}

fn share(v: f32) -> f64 {
    (f64::from(v) * 1000.0).round() / 1000.0
}

/// The dark runs as JSON, with their total length.
pub fn dark_json(times: &[f64], end_ms: f64, series: &[f32], thresholds: Thresholds) -> (Value, f64) {
    let found = stretches(times, end_ms, series, thresholds);
    let total = found.iter().map(|s| s.end_ms - s.start_ms).sum::<f64>();
    let list = found
        .iter()
        .map(|s| {
            json!({
                "startMs": s.start_ms.round(),
                "endMs": s.end_ms.round(),
                "durationMs": (s.end_ms - s.start_ms).round(),
                "meanLit": share(s.mean),
                "peakLit": share(s.peak),
            })
        })
        .collect::<Vec<_>>();
    (json!(list), total.round())
}

/// A group's lowest, mean and highest lit share, its dark runs and, with `series`, the lit share of every frame.
pub fn group_json(group: &Group, times: &[f64], end_ms: f64, thresholds: Thresholds, series: bool) -> Value {
    let values = &group.series;
    let (dark, dark_ms) = dark_json(times, end_ms, values, thresholds);
    let mut out = json!({
        "lights": group.lights.len(),
        "lit": {
            "min": share(values.iter().copied().fold(f32::INFINITY, f32::min).min(1.0)),
            "mean": share(if values.is_empty() { 0.0 } else { values.iter().sum::<f32>() / values.len() as f32 }),
            "max": share(values.iter().copied().fold(0.0, f32::max)),
        },
        "dark": dark,
        "darkMs": dark_ms,
    });
    if let Some(id) = group.id {
        out["id"] = json!(id.to_string());
        out["name"] = json!(group.name);
    }
    if series {
        out["series"] = json!(values.iter().map(|&v| share(v)).collect::<Vec<_>>());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Thresholds = Thresholds { level: 0.1, min_lit: 0.05, min_ms: 1000.0 };

    fn times(n: usize) -> Vec<f64> {
        (0..n).map(|i| i as f64 * 500.0).collect()
    }

    #[test]
    fn dark_runs_long_enough_are_reported_with_their_span() {
        // 500 ms frames: dark for 1.5 s from 1 s, a single dark frame at 4 s, dark again from 5 s to the end (6.2 s).
        let series = [0.5, 0.4, 0.0, 0.02, 0.0, 0.6, 0.6, 0.6, 0.01, 0.7, 0.0, 0.0, 0.0];
        let found = stretches(&times(series.len()), 6200.0, &series, T);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!((found[0].start_ms, found[0].end_ms), (1000.0, 2500.0));
        assert!((found[0].peak - 0.02).abs() < 1e-6);
        assert!((found[0].mean - 0.02 / 3.0).abs() < 1e-6);
        assert_eq!((found[1].start_ms, found[1].end_ms), (5000.0, 6200.0));
    }

    #[test]
    fn a_lit_or_short_span_has_no_dark_runs() {
        assert!(stretches(&times(4), 2000.0, &[0.2, 0.05, 0.3, 0.9], T).is_empty());
        assert!(stretches(&times(2), 1000.0, &[0.0, 0.5], T).is_empty());
        assert_eq!(stretches(&times(2), 1000.0, &[0.0, 0.0], T).len(), 1);
    }
}
