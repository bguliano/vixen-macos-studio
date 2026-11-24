//! music_rhythm and sequence_section_marks: a song's bars, 16th-slot onset table, energy, estimated sections and
//! repeated bars, and named section marks an agent can write and read back.
use super::{analysis_audio, filename, find, mark_effects::sequence_audio, onsets, record, string, track, uuid};
use crate::{app::AppCore, sequence::SequenceSession, undo::UndoAction};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use vixen_analysis::fft::FftReal;
use vixen_core::{marks::Mark, Color, MarkCollection, TimeSpan};

pub(super) const SECTIONS: &str = "Sections";
/// Bars each side of a boundary that the section novelty compares.
const KERNEL_BARS: usize = 4;
const MIN_SECTION_BARS: usize = 4;
/// Window, in bars, that two passages are compared over to call one a repeat of the other.
const REPEAT_BARS: usize = 4;

pub(super) struct Bar {
    pub start: f64,
    pub length: f64,
    pub beats: Vec<f64>,
    pub tempo: f64,
    pub db: f64,
}

pub(super) struct Section {
    /// intro, verse, chorus, bridge, drop or outro.
    pub kind: &'static str,
    /// The kind numbered by occurrence when it occurs more than once ("Chorus 2").
    pub label: String,
    /// Letter shared by sections that sound alike (A, B, ...).
    pub group: char,
    /// First bar and one past the last, 0-based.
    pub bars: (usize, usize),
    pub db: f64,
}

pub(super) struct Analysis {
    pub bars: Vec<Bar>,
    pub grid: Vec<[[f64; onsets::SLOTS_PER_BAR]; 3]>,
    /// For each bar, the earlier bar it best repeats and the similarity, when it is a repeat.
    pub repeat_of: Vec<Option<(usize, f64)>>,
    /// Runs of at least REPEAT_BARS consecutive repeated bars: (first bar, bar count, first bar repeated, similarity).
    pub repeats: Vec<(usize, usize, usize, f64)>,
    pub sections: Vec<Section>,
    /// Tempo segments: (first bar, bpm).
    pub tempo_map: Vec<(usize, f64)>,
}

/// Bars from the tracker's downbeats (the last bar as long as the median), each with its beats, tempo and level.
fn bars(samples: &[f32], sr: f64, bar_starts: &[f64], beats: &[f64]) -> Vec<Bar> {
    let mut lengths: Vec<f64> = bar_starts.windows(2).map(|w| w[1] - w[0]).filter(|d| *d > 0.0).collect();
    lengths.sort_by(f64::total_cmp);
    let Some(typical) = lengths.get(lengths.len() / 2).copied() else { return Vec::new() };
    bar_starts
        .iter()
        .enumerate()
        .map(|(i, start)| {
            // The last bar is as long as a typical one even where the song ends inside it, so its tempo stays real.
            let length = bar_starts.get(i + 1).map_or(typical, |next| next - start);
            let in_bar: Vec<f64> = beats.iter().copied().filter(|b| *b >= start - 1.0 && *b < start + length - 1.0).collect();
            // A short bar where a breakdown left beats over counts what it has; the song's last bar, cut off, counts 4.
            let count = if (2..=6).contains(&in_bar.len()) && i + 1 < bar_starts.len() { in_bar.len() } else { 4 };
            let (from, to) = ((start * sr / 1000.0) as usize, (((start + length) * sr / 1000.0) as usize).min(samples.len()));
            let power = samples.get(from..to).filter(|s| !s.is_empty()).map_or(0.0, |s| {
                s.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / s.len() as f64
            });
            Bar { start: *start, length, beats: in_bar, tempo: count as f64 * 60_000.0 / length, db: db(power) }
        })
        .collect()
}

fn db(power: f64) -> f64 {
    (10.0 * power.max(1e-9).log10()).max(-90.0)
}

/// Per-bar chroma (12 pitch classes, 100..2100 Hz) and spectral shape (8 log-spaced bands, 60..12000 Hz), from a
/// ~186 ms STFT with a quarter-window hop.
fn bar_spectra(samples: &[f32], sr: f64, bars: &[Bar]) -> (Vec<[f64; 12]>, Vec<[f64; 8]>) {
    let mut chroma = vec![[0.0; 12]; bars.len()];
    let mut shape = vec![[0.0; 8]; bars.len()];
    let n = ((sr * 0.186).round() as usize).max(256).next_power_of_two();
    let hop = n / 4;
    let Some(mut fft) = FftReal::new(n) else { return (chroma, shape) };
    if samples.len() < n || bars.is_empty() {
        return (chroma, shape);
    }
    let window: Vec<f64> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()).collect();
    let bin_hz = sr / n as f64;
    let pitch_class: Vec<Option<usize>> = (0..=n / 2)
        .map(|k| {
            let f = k as f64 * bin_hz;
            (100.0..2100.0).contains(&f).then(|| ((12.0 * (f / 440.0).log2()).round() as i64).rem_euclid(12) as usize)
        })
        .collect();
    let band: Vec<Option<usize>> = (0..=n / 2)
        .map(|k| {
            let f = k as f64 * bin_hz;
            (60.0..12_000.0).contains(&f).then(|| ((f / 60.0).log2() / (200f64).log2() * 8.0) as usize).map(|b| b.min(7))
        })
        .collect();
    let (mut input, mut re, mut im) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    let starts: Vec<f64> = bars.iter().map(|b| b.start).collect();
    let end = bars.last().map_or(0.0, |b| b.start + b.length);
    for frame in 0..=(samples.len() - n) / hop {
        let time = (frame * hop + n / 2) as f64 * 1000.0 / sr;
        if time < starts[0] || time >= end {
            continue;
        }
        let bar = starts.partition_point(|s| *s <= time) - 1;
        for (i, x) in input.iter_mut().enumerate() {
            *x = f64::from(samples[frame * hop + i]) * window[i];
        }
        fft.forward(&input, &mut re, &mut im);
        for k in 1..=n / 2 {
            let power = re[k] * re[k] + im[k] * im[k];
            if let Some(pc) = pitch_class[k] {
                chroma[bar][pc] += power.sqrt();
            }
            if let Some(b) = band[k] {
                shape[bar][b] += power;
            }
        }
    }
    for s in &mut shape {
        let logs = s.map(|p| (p + 1e-9).ln());
        let mean = logs.iter().sum::<f64>() / 8.0;
        *s = logs.map(|l| l - mean);
    }
    (chroma, shape)
}

fn cosine(a: &[f64], b: &[f64]) -> f64 {
    let (dot, na, nb) = a.iter().zip(b).fold((0.0, 0.0, 0.0), |(d, x, y), (p, q)| (d + p * q, x + p * p, y + q * q));
    if na <= 1e-12 && nb <= 1e-12 {
        1.0
    } else if na <= 1e-12 || nb <= 1e-12 {
        0.0
    } else {
        dot / (na * nb).sqrt()
    }
}

/// Bar-to-bar similarity 0..1: harmony (chroma), sound (spectral shape) and rhythm (16th-slot onsets), scaled down by
/// the level difference.
fn similarity(bars: &[Bar], chroma: &[[f64; 12]], shape: &[[f64; 8]], grid: &[[[f64; 16]; 3]]) -> Vec<Vec<f64>> {
    let rhythm: Vec<Vec<f64>> = grid.iter().map(|g| g.iter().flatten().copied().collect()).collect();
    let n = bars.len();
    let mut s = vec![vec![1.0; n]; n];
    for i in 0..n {
        for j in 0..i {
            let harmony = cosine(&chroma[i], &chroma[j]);
            let sound = (cosine(&shape[i], &shape[j]) + 1.0) / 2.0;
            let beat = cosine(&rhythm[i], &rhythm[j]);
            let level = (-(bars[i].db - bars[j].db).abs() / 8.0).exp();
            let v = (0.45 * harmony + 0.25 * sound + 0.3 * beat) * level;
            s[i][j] = v;
            s[j][i] = v;
        }
    }
    s
}

fn percentile(mut values: Vec<f64>, p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    values[((values.len() - 1) as f64 * p).round() as usize]
}

/// Mean similarity of the `len` bars from i against the `len` bars from j.
fn window(s: &[Vec<f64>], i: usize, j: usize, len: usize) -> f64 {
    let n = s.len();
    let k = len.min(n - i).min(n - j);
    (0..k).map(|d| s[i + d][j + d]).sum::<f64>() / k.max(1) as f64
}

/// Foote novelty at each bar start (a checkerboard kernel over the similarity matrix) plus the level jump across it.
fn novelty(s: &[Vec<f64>], bars: &[Bar]) -> Vec<f64> {
    let n = s.len();
    let l = KERNEL_BARS as isize;
    let mut raw = vec![0.0; n];
    for (b, value) in raw.iter_mut().enumerate().skip(1) {
        let (mut sum, mut weight) = (0.0, 0.0);
        for x in -l..l {
            for y in -l..l {
                let (i, j) = (b as isize + x, b as isize + y);
                if i < 0 || j < 0 || i >= n as isize || j >= n as isize {
                    continue;
                }
                let sign = if (x < 0) == (y < 0) { 1.0 } else { -1.0 };
                let taper = (-((x as f64 + 0.5).powi(2) + (y as f64 + 0.5).powi(2)) / (2.0 * (l as f64 / 2.0).powi(2))).exp();
                sum += sign * taper * s[i as usize][j as usize];
                weight += taper;
            }
        }
        *value = (sum / weight.max(1e-9)).max(0.0);
    }
    let top = raw.iter().copied().fold(0.0, f64::max).max(1e-9);
    let mean_db = |r: std::ops::Range<usize>| r.clone().map(|i| bars[i].db).sum::<f64>() / r.len().max(1) as f64;
    (0..n)
        .map(|b| {
            let jump = if b == 0 { 0.0 } else { (mean_db(b..(b + KERNEL_BARS).min(n)) - mean_db(b.saturating_sub(KERNEL_BARS)..b)).abs() };
            raw[b] / top + (jump / 12.0).min(1.0) * 0.6
        })
        .collect()
}

/// Section starts: novelty peaks at least MIN_SECTION_BARS apart, moved to the song's 4-bar phrase grid when a
/// boundary there is nearly as strong.
fn boundaries(nov: &[f64]) -> Vec<usize> {
    let n = nov.len();
    if n < 2 * MIN_SECTION_BARS {
        return vec![0];
    }
    let phase = (0..4)
        .max_by(|a, b| {
            let score = |p: usize| (p..n).step_by(4).map(|i| nov[i]).sum::<f64>();
            score(*a).total_cmp(&score(*b))
        })
        .unwrap_or(0);
    let inner = &nov[2..n - 2];
    let mean = inner.iter().sum::<f64>() / inner.len() as f64;
    let std = (inner.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / inner.len() as f64).sqrt();
    let mut candidates: Vec<usize> = (2..n - 2)
        .filter(|&b| nov[b] >= mean + 0.5 * std && (b.saturating_sub(2)..=(b + 2).min(n - 1)).all(|o| nov[o] <= nov[b]))
        .map(|b| {
            let aligned = [b.saturating_sub(1), b, b + 1].into_iter().find(|o| *o < n && *o % 4 == phase);
            match aligned {
                Some(a) if nov[a] >= 0.75 * nov[b] => a,
                _ => b,
            }
        })
        .collect();
    candidates.sort_by(|a, b| nov[*b].total_cmp(&nov[*a]));
    let mut accepted = vec![0usize];
    for b in candidates {
        if b >= 2 && n - b >= MIN_SECTION_BARS && accepted.iter().all(|a| a.abs_diff(b) >= MIN_SECTION_BARS || (*a == 0 && b >= 2)) {
            accepted.push(b);
        }
    }
    accepted.sort_unstable();
    accepted.dedup();
    accepted
}

/// Groups alike sections and names them: an unrepeated (or short, quiet) first and last section is the intro and outro, the
/// loudest repeated group the chorus (a drop when it bursts in after a much quieter section with heavy low hits),
/// other repeated groups verses, and the rest bridges (or a drop, by the same test).
fn sections(s: &[Vec<f64>], bars: &[Bar], grid: &[[[f64; 16]; 3]], starts: &[usize], threshold: f64) -> Vec<Section> {
    let n = bars.len();
    let spans: Vec<(usize, usize)> = starts.iter().enumerate().map(|(i, b)| (*b, starts.get(i + 1).copied().unwrap_or(n))).collect();
    let level = |(a, b): (usize, usize)| bars[a..b].iter().map(|x| x.db).sum::<f64>() / (b - a).max(1) as f64;
    let lows = |(a, b): (usize, usize)| grid[a..b].iter().map(|g| g[0].iter().filter(|v| **v > 0.0).count()).sum::<usize>() as f64 / (b - a).max(1) as f64;
    let mut group = vec![usize::MAX; spans.len()];
    let mut leaders: Vec<usize> = Vec::new();
    for (i, span) in spans.iter().enumerate() {
        let len = |x: (usize, usize)| x.1 - x.0;
        let found = leaders.iter().position(|&l| {
            let other = spans[l];
            len(*span).min(len(other)) * 2 >= len(*span).max(len(other)) && window(s, span.0, other.0, len(*span).min(len(other))) >= threshold
        });
        group[i] = found.unwrap_or_else(|| {
            leaders.push(i);
            leaders.len() - 1
        });
    }
    let count = |g: usize| group.iter().filter(|x| **x == g).count();
    let mut bar_levels: Vec<f64> = bars.iter().map(|b| b.db).collect();
    bar_levels.sort_by(f64::total_cmp);
    let reference = bar_levels[bar_levels.len() / 2];
    let median_lows = percentile(spans.iter().map(|x| lows(*x)).collect(), 0.5);
    let last = spans.len() - 1;
    let mut kinds: Vec<&'static str> = vec![""; spans.len()];
    // A quiet end that recurs later at full length is a verse, not an intro; a short quiet one is still an intro.
    let end_part = |i: usize| count(group[i]) == 1 || (level(spans[i]) < reference - 3.0 && spans[i].1 - spans[i].0 <= MIN_SECTION_BARS);
    if spans.len() > 2 {
        if end_part(0) {
            kinds[0] = "intro";
        }
        if end_part(last) {
            kinds[last] = "outro";
        }
    }
    let repeated: Vec<usize> = (0..leaders.len()).filter(|g| (0..spans.len()).filter(|i| group[*i] == *g && kinds[*i].is_empty()).count() >= 2).collect();
    let group_level = |g: usize| {
        let members: Vec<f64> = (0..spans.len()).filter(|i| group[*i] == g).map(|i| level(spans[i])).collect();
        members.iter().sum::<f64>() / members.len().max(1) as f64
    };
    let chorus = repeated.iter().copied().max_by(|a, b| group_level(*a).total_cmp(&group_level(*b)));
    let loudest = spans.iter().map(|x| level(*x)).fold(f64::MIN, f64::max);
    let bursts = |i: usize| {
        i > 0 && level(spans[i]) - level(spans[i - 1]) >= 6.0 && level(spans[i]) >= loudest - 2.0 && lows(spans[i]) >= 1.3 * median_lows.max(0.5)
    };
    for i in 0..spans.len() {
        if !kinds[i].is_empty() {
            continue;
        }
        kinds[i] = if Some(group[i]) == chorus {
            if bursts(i) { "drop" } else { "chorus" }
        } else if repeated.contains(&group[i]) {
            "verse"
        } else if bursts(i) {
            "drop"
        } else {
            "bridge"
        };
    }
    let mut seen: Vec<(&str, usize)> = Vec::new();
    spans
        .iter()
        .enumerate()
        .map(|(i, span)| {
            let total = kinds.iter().filter(|k| **k == kinds[i]).count();
            let nth = match seen.iter_mut().find(|(k, _)| *k == kinds[i]) {
                Some(entry) => {
                    entry.1 += 1;
                    entry.1
                }
                None => {
                    seen.push((kinds[i], 1));
                    1
                }
            };
            let mut label = kinds[i][..1].to_uppercase() + &kinds[i][1..];
            if total > 1 {
                label = format!("{label} {nth}");
            }
            Section { kind: kinds[i], label, group: (b'A' + (group[i] % 26) as u8) as char, bars: *span, db: level(*span) }
        })
        .collect()
}

/// Tempo segments: a new segment starts where the bar tempo (median of three) moves more than 4% from the current
/// segment's and stays there for two bars.
fn tempo_map(bars: &[Bar]) -> Vec<(usize, f64)> {
    let smooth: Vec<f64> = (0..bars.len())
        .map(|i| {
            let mut w: Vec<f64> = bars[i.saturating_sub(1)..(i + 2).min(bars.len())].iter().map(|b| b.tempo).collect();
            w.sort_by(f64::total_cmp);
            w[w.len() / 2]
        })
        .collect();
    let mut map: Vec<(usize, f64)> = Vec::new();
    let mut from = 0;
    for i in 0..smooth.len() {
        let current = smooth[from..i.max(from + 1)].iter().sum::<f64>() / (i.max(from + 1) - from) as f64;
        let away = |t: f64| (t - current).abs() / current > 0.04;
        if i > from && away(smooth[i]) && smooth.get(i + 1).is_none_or(|t| away(*t)) {
            map.push((from, current));
            from = i;
        }
    }
    if !smooth.is_empty() {
        map.push((from, smooth[from..].iter().sum::<f64>() / (smooth.len() - from) as f64));
    }
    map
}

/// Level in dB of the samples between two times.
fn level(samples: &[f32], sr: f64, from: f64, to: f64) -> f64 {
    let (a, b) = (((from.max(0.0) * sr / 1000.0) as usize).min(samples.len()), ((to.max(0.0) * sr / 1000.0) as usize).min(samples.len()));
    if b <= a {
        return -90.0;
    }
    db(samples[a..b].iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / (b - a) as f64)
}

/// The song's loudness over time for level envelopes (effects_edit levelFromEnergy, B94): the RMS level every
/// LOUDNESS_STEP_MS, smoothed over a centred window, as 0..1 from the song's quietest 5% to its loudest 5% of windows
/// with music in them (within 30 dB of the loud level, so silence before and after the song doesn't set the floor).
pub(super) struct Loudness {
    values: Vec<f64>,
}

const LOUDNESS_STEP_MS: f64 = 50.0;

impl Loudness {
    pub(super) fn new(samples: &[f32], sample_rate: u32, smooth_ms: f64) -> Self {
        let per = ((LOUDNESS_STEP_MS / 1000.0 * f64::from(sample_rate)).round() as usize).max(1);
        let raw: Vec<f64> = samples.chunks(per).map(|c| db(c.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / c.len() as f64)).collect();
        let half = (smooth_ms / LOUDNESS_STEP_MS / 2.0).round() as usize;
        let smoothed: Vec<f64> = (0..raw.len())
            .map(|i| {
                let w = &raw[i.saturating_sub(half)..(i + half + 1).min(raw.len())];
                w.iter().sum::<f64>() / w.len() as f64
            })
            .collect();
        let top = percentile(raw.clone(), 0.95);
        let music: Vec<f64> = smoothed.iter().copied().filter(|d| *d >= top - 30.0).collect();
        let (quiet, loud) = (percentile(music.clone(), 0.05), percentile(music, 0.95));
        let values = smoothed.iter().map(|d| ((d - quiet) / (loud - quiet).max(1.0)).clamp(0.0, 1.0)).collect();
        Self { values }
    }

    /// Loudness 0..1 at a time in ms, between the two nearest windows (0 past the end of the song).
    pub(super) fn at(&self, ms: f64) -> f64 {
        let x = (ms / LOUDNESS_STEP_MS - 0.5).max(0.0);
        let i = x.floor() as usize;
        match (self.values.get(i), self.values.get(i + 1)) {
            (Some(a), Some(b)) => a + (b - a) * x.fract(),
            (Some(a), None) => *a,
            _ => 0.0,
        }
    }
}

/// Where the music starts and ends: the first and last 50 ms within 30 dB of its loud level (95th percentile).
fn music_span(samples: &[f32], sr: f64) -> Option<(f64, f64)> {
    let block = ((sr * 0.05) as usize).max(1);
    let power: Vec<f64> = samples.chunks(block).map(|c| c.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / c.len() as f64).collect();
    let loud = percentile(power.clone(), 0.95);
    let first = power.iter().position(|p| *p >= loud * 1e-3)?;
    let last = power.iter().rposition(|p| *p >= loud * 1e-3)?;
    Some((first as f64 * 50.0, (last + 1) as f64 * 50.0))
}

/// The strongest low/mid onset within 40 ms of a time, per band.
fn hit(detected: &[onsets::Onset], band: usize, time: f64) -> f64 {
    let from = detected.partition_point(|o| o.time_ms < time - 40.0);
    detected[from..].iter().take_while(|o| o.time_ms <= time + 40.0).filter(|o| o.band == band).map(|o| o.strength).fold(0.0, f64::max)
}

/// How well a beat grid explains the low and mid onsets between two times: the F-measure of beats with a hit within
/// 40 ms and hits within 40 ms of a beat.
fn fit(grid: &[f64], detected: &[onsets::Onset], from: f64, to: f64) -> f64 {
    let hits: Vec<f64> = detected.iter().filter(|o| o.band < 2 && o.strength >= 0.2 && o.time_ms >= from && o.time_ms < to).map(|o| o.time_ms).collect();
    let beats: Vec<f64> = grid.iter().copied().filter(|t| *t >= from && *t < to).collect();
    if hits.is_empty() || beats.is_empty() {
        return 0.0;
    }
    let near = |t: f64, list: &[f64]| list.iter().any(|x| (x - t).abs() <= 40.0);
    let precision = beats.iter().filter(|b| near(**b, &hits)).count() as f64 / beats.len() as f64;
    let recall = hits.iter().filter(|h| near(**h, &beats)).count() as f64 / hits.len() as f64;
    if precision + recall == 0.0 { 0.0 } else { 2.0 * precision * recall / (precision + recall) }
}

/// Repairs the tracker's beats, then numbers bars from the beat that sounds like beat 1. Beats in the silence before or
/// after the music go; a stretch at either end tracked off the steady tempo (a sparse intro or outro) is replaced by the
/// steady beat carried out to the music's edge, unless the stretch holds a steady, kick-driven tempo of its own that fits
/// its onsets better.
/// The downbeat is the beat phase (of 4) where the level jumps (sections enter on 1), the low hits fall on 1 and 3
/// and the backbeat (mid) on 2 and 4, with the tracker's own bar phase breaking ties. Returns bar starts and beats.
fn regrid(samples: &[f32], sr: f64, bar_starts: &[f64], beats: &[f64], detected: &[onsets::Onset]) -> (Vec<f64>, Vec<f64>) {
    let Some((start, end)) = music_span(samples, sr) else { return (bar_starts.to_vec(), beats.to_vec()) };
    let mut periods: Vec<f64> = beats.windows(2).map(|w| w[1] - w[0]).collect();
    let dominant = percentile(periods.clone(), 0.5);
    let mut beats: Vec<f64> = beats.iter().copied().filter(|t| *t >= start - 0.25 * dominant && *t < end).collect();
    if beats.len() < 16 || dominant <= 0.0 {
        return (bar_starts.to_vec(), beats);
    }
    periods = beats.windows(2).map(|w| w[1] - w[0]).collect();
    let steady = |i: usize| periods[i..i + 8].iter().all(|p| (p / dominant - 1.0).abs() <= 0.06);
    // A real tempo of its own keeps 80% of its beats within 6% of its median and has a low hit (a kick) on at least half
    // of them; a tracker that locked onto syncopated stabs (Sandstorm's intro) or wanders through a drumless intro doesn't.
    let own_tempo = |p: &[f64], b: &[f64]| {
        let m = percentile(p.to_vec(), 0.5);
        let steady = p.len() >= 8 && p.iter().filter(|x| (*x / m - 1.0).abs() <= 0.06).count() * 5 >= p.len() * 4;
        steady && b.iter().filter(|t| hit(detected, 0, **t) >= 0.3).count() * 2 >= b.len()
    };
    let runs: Vec<usize> = (0..periods.len().saturating_sub(7)).filter(|i| steady(*i)).collect();
    let mut cuts: Vec<f64> = Vec::new();
    if let (Some(&first), Some(&last)) = (runs.first(), runs.last()) {
        // Repairs run from the end so earlier indices stay valid.
        let last = last + 8;
        if last + 1 < beats.len() {
            let step = (beats[last] - beats[last - 8]) / 8.0;
            let carried: Vec<f64> = (1..).map(|k| beats[last] + k as f64 * step).take_while(|t| *t < end).collect();
            if !own_tempo(&periods[last..], &beats[last + 1..]) || fit(&carried, detected, beats[last] + 1.0, end) >= fit(&beats[last + 1..], detected, beats[last] + 1.0, end) {
                beats.truncate(last + 1);
                beats.extend(carried);
            }
        }
        // A wandering stretch between two steady passages at the same tempo (a breakdown the tracker lost) gets evenly
        // spaced beats at that tempo, so the bar count through it stays right.
        let mut covered = vec![false; periods.len() + 1];
        for &r in &runs {
            covered[r..=r + 8].iter_mut().for_each(|c| *c = true);
        }
        let mut gaps = Vec::new();
        let mut j = first;
        while j < last {
            if covered[j] && !covered[j + 1] {
                let next = (j + 1..=last).find(|k| covered[*k]).unwrap_or(last);
                gaps.push((j, next));
                j = next;
            } else {
                j += 1;
            }
        }
        for &(a, b) in gaps.iter().rev() {
            let side = |p: &[f64]| percentile(p.to_vec(), 0.5);
            let (before, after) = (side(&periods[a.saturating_sub(8)..a]), side(&periods[b..(b + 8).min(periods.len())]));
            let local = (before + after) / 2.0;
            let count = ((beats[b] - beats[a]) / local).round().max(1.0) as usize;
            let step = (beats[b] - beats[a]) / count as f64;
            let even: Vec<f64> = (1..count).map(|k| beats[a] + k as f64 * step).collect();
            let keep = own_tempo(&periods[a..b], &beats[a + 1..b]) && fit(&beats[a + 1..b], detected, beats[a] + 1.0, beats[b]) > fit(&even, detected, beats[a] + 1.0, beats[b]);
            if (before / after - 1.0).abs() <= 0.06 && !keep {
                beats.splice(a + 1..b, even);
                cuts.push(beats[a] + 1.0);
            }
        }
        if first > 0 {
            let step = periods[first..first + 8].iter().sum::<f64>() / 8.0;
            let mut carried: Vec<f64> = (1..).map(|k| beats[first] - k as f64 * step).take_while(|t| *t >= start - 0.25 * step).collect();
            carried.reverse();
            if !own_tempo(&periods[..first], &beats[..first]) || fit(&carried, detected, start, beats[first] - 1.0) >= fit(&beats[..first], detected, start, beats[first] - 1.0) {
                beats.splice(..first, carried);
            }
        }
    }
    // Beat phase, chosen on each side of a repaired breakdown (whose length need not be whole bars): level jumps of 3 dB
    // or more (capped at 12) a beat apart, skipping the music's first entrance, and the low/mid balance on each beat.
    cuts.sort_by(f64::total_cmp);
    let tracked = bar_starts.iter().find_map(|b| beats.iter().position(|t| (t - b).abs() < 0.25 * dominant)).map(|i| i % 4);
    let mut starts = Vec::new();
    let mut from = 0;
    let mut prior = tracked;
    for segment in 0..=cuts.len() {
        let to = cuts.get(segment).map_or(beats.len(), |c| beats.partition_point(|t| *t < *c));
        if to <= from {
            continue;
        }
        let (mut entrance, mut backbeat, mut hits) = ([0.0; 4], [0.0; 4], 0.0);
        for i in from..to {
            if i > 0 && i + 1 < beats.len() && beats[i] > start + 2.0 * dominant {
                let rise = level(samples, sr, beats[i], beats[i + 1]) - level(samples, sr, beats[i - 1], beats[i]);
                entrance[i % 4] += (rise - 3.0).clamp(0.0, 9.0);
            }
            let (low, mid) = (hit(detected, 0, beats[i]), hit(detected, 1, beats[i]));
            backbeat[i % 4] += low - mid;
            backbeat[(i + 2) % 4] += low - mid;
            hits += low + mid;
        }
        // Scaled so that one capped entrance, or a backbeat a quarter of all hits strong, counts 1; a passage with neither
        // (a kick on every beat, no breaks) keeps the tracker's phase, or the previous passage's.
        let entrance = entrance.map(|e| e / 9.0);
        let backbeat = backbeat.map(|b| (b / (0.25 * hits).max(1.0)).clamp(-1.0, 1.0));
        let phase = (0..4)
            .max_by(|a, b| {
                let score = |p: usize| entrance[p] + 0.5 * backbeat[p] + if prior == Some(p) { 0.3 } else { 0.0 };
                score(*a).total_cmp(&score(*b))
            })
            .unwrap_or(0);
        starts.extend((from..to).filter(|i| i % 4 == phase).map(|i| beats[i]));
        prior = Some(phase);
        from = to;
    }
    (starts, beats)
}

pub(super) fn analyze(samples: &[f32], sample_rate: u32, bar_starts: &[f64], beats: &[f64], detected: &[onsets::Onset]) -> Analysis {
    let sr = f64::from(sample_rate);
    let (bar_starts, beats) = regrid(samples, sr, bar_starts, beats, detected);
    let bars = bars(samples, sr, &bar_starts, &beats);
    let starts: Vec<f64> = bars.iter().map(|b| b.start).collect();
    let grid = onsets::slot_grid(&starts, detected);
    let (chroma, shape) = bar_spectra(samples, sr, &bars);
    let s = similarity(&bars, &chroma, &shape, &grid);
    let n = bars.len();
    // Repeats are judged against the song's own spread: well above its typical bar pair, and never below 0.75.
    let windows: Vec<f64> = (0..n).flat_map(|i| (0..(i + 1).saturating_sub(REPEAT_BARS)).map(move |j| (i, j))).map(|(i, j)| window(&s, i, j, REPEAT_BARS)).collect();
    let threshold = percentile(windows, 0.85).max(0.75);
    // A bar repeats an earlier bar at least REPEAT_BARS back (so a steady passage doesn't just repeat itself). Among
    // near-equal matches it continues the previous bar's run, else takes the first occurrence.
    let mut repeat_of: Vec<Option<(usize, f64)>> = Vec::with_capacity(n);
    for i in 0..n {
        let scores: Vec<(usize, f64)> = (0..(i + 1).saturating_sub(REPEAT_BARS)).map(|j| (j, window(&s, i, j, REPEAT_BARS))).collect();
        let best = scores.iter().map(|x| x.1).fold(f64::MIN, f64::max);
        let near = |j: usize| scores.get(j).is_some_and(|x| x.1 >= best - 0.02);
        let previous = i.checked_sub(1).and_then(|p| repeat_of[p]).map(|(k, _)| k + 1);
        let pick = previous.filter(|k| near(*k)).or_else(|| scores.iter().position(|x| x.1 >= best - 0.02));
        repeat_of.push(pick.map(|j| scores[j]).filter(|(_, v)| *v >= threshold));
    }
    let mut repeats = Vec::new();
    let mut i = 0;
    while i < n {
        let Some((j, _)) = repeat_of[i] else {
            i += 1;
            continue;
        };
        let mut len = 1;
        while i + len < n && repeat_of[i + len].is_some_and(|(k, _)| k == j + len) {
            len += 1;
        }
        if len >= REPEAT_BARS {
            repeats.push((i, len, j, window(&s, i, j, len)));
        }
        i += len;
    }
    // Whole sections are alike at a slightly looser bar: onset strengths shift with what comes before (a quiet verse
    // after a loud chorus), which a bar-for-bar repeat would not forgive.
    let group_threshold = (threshold - 0.08).max(0.72);
    let sections = if n == 0 { Vec::new() } else { sections(&s, &bars, &grid, &boundaries(&novelty(&s, &bars)), group_threshold) };
    let tempo_map = tempo_map(&bars);
    Analysis { bars, grid, repeat_of, repeats, sections, tempo_map }
}

/// A band's 16 slots as text, a beat per group: a digit 1-9 for the strongest onset's strength in tenths (rounded up),
/// "." for none. "9... 3... 9.3. 3..." is a kick on every beat with offbeat fills.
pub(super) fn slot_text(slots: &[f64; onsets::SLOTS_PER_BAR]) -> String {
    let mut out = String::with_capacity(19);
    for (i, v) in slots.iter().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push(' ');
        }
        out.push(if *v <= 0.0 { '.' } else { char::from(b'0' + (v * 9.0).ceil().clamp(1.0, 9.0) as u8) });
    }
    out
}

fn round(v: f64, places: i32) -> f64 {
    (v * 10f64.powi(places)).round() / 10f64.powi(places)
}

/// The media to analyse and the sequence it came from, if any.
fn source(core: &AppCore, a: &Value) -> Result<(String, Option<Arc<Mutex<SequenceSession>>>), String> {
    let shared = if a.get("sequenceId").is_some() { Some(find(core, uuid(a, "sequenceId")?)?) } else { None };
    let media = match (a.get("media"), &shared) {
        (Some(_), _) => filename(a, "media")?,
        (None, Some(s)) => sequence_audio(&s.lock()).ok_or("The sequence has no audio; pass media, a profile Media filename")?,
        (None, None) => return Err("Pass media (a profile Media filename) or sequenceId".into()),
    };
    Ok((media, shared))
}

struct Run {
    media: String,
    audio: vixen_audio::AudioClip,
    duration: f64,
    detected: Vec<onsets::Onset>,
    analysis: Analysis,
}

fn run(core: &AppCore, media: &str) -> Result<Run, String> {
    let audio = analysis_audio(core, media)?;
    let duration = audio.duration().total_ms();
    let (_, bar_starts, beats) = track(&audio)?;
    let detected = onsets::detect(audio.mono(), audio.sample_rate());
    let analysis = analyze(audio.mono(), audio.sample_rate(), &bar_starts, &beats, &detected);
    if analysis.bars.is_empty() {
        return Err("No bars were detected in this media (too short, or no steady beat)".into());
    }
    Ok(Run { media: media.into(), audio, duration, detected, analysis })
}

/// 1-based number of the bar containing a time (0 before the first downbeat).
fn bar_at(bars: &[Bar], time: f64) -> usize {
    bars.partition_point(|b| b.start <= time + 0.5)
}

fn sections_from_marks(collection: &MarkCollection, bars: &[Bar], length: f64) -> Vec<Value> {
    let mut marks: Vec<&Mark> = collection.marks.iter().collect();
    marks.sort_by(|a, b| a.start_time.total_ms().total_cmp(&b.start_time.total_ms()));
    marks
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let start = m.start_time.total_ms();
            let end = marks.get(i + 1).map_or(length, |n| n.start_time.total_ms());
            json!({"name":m.text,"startMs":start,"endMs":end,"startBar":bar_at(bars, start),"source":"marks"})
        })
        .collect()
}

fn estimated_sections(analysis: &Analysis, duration: f64) -> Vec<Value> {
    let bars = &analysis.bars;
    analysis
        .sections
        .iter()
        .enumerate()
        .map(|(i, s)| {
            // The first section starts at 0 so a pickup before the first downbeat belongs to it.
            let start = if i == 0 { 0.0 } else { bars[s.bars.0].start };
            let end = bars.get(s.bars.1).map_or(duration, |b| b.start);
            json!({"name":s.label,"kind":s.kind,"group":s.group.to_string(),"startMs":round(start, 0),"endMs":round(end, 0),"startBar":s.bars.0 + 1,"endBar":s.bars.1,"levelDb":round(s.db, 1),"source":"estimate"})
        })
        .collect()
}

pub(super) fn music_rhythm(core: &AppCore, a: &Value) -> Result<Value, String> {
    let (media, shared) = source(core, a)?;
    let Run { audio, duration, detected, analysis, .. } = run(core, &media)?;
    let include = |part: &str| a.get("include").and_then(Value::as_array).is_none_or(|l| l.iter().any(|v| v == part));
    let (from, to) = match a.get("rangeMs").and_then(Value::as_array) {
        Some(r) => (r[0].as_f64().unwrap_or(0.0), r[1].as_f64().unwrap_or(f64::MAX)),
        None => (0.0, f64::MAX),
    };
    if from >= to {
        return Err("rangeMs must be [startMs, endMs] with startMs < endMs".into());
    }
    let bars = &analysis.bars;
    let collection_name = if a.get("sectionsCollection").is_some() { string(a, "sectionsCollection")? } else { SECTIONS };
    let marked = shared.as_ref().and_then(|s| {
        let s = s.lock();
        let length = s.length().total_ms();
        s.marks().iter().find(|c| c.name == collection_name && !c.marks.is_empty()).map(|c| sections_from_marks(c, bars, length))
    });
    let sections = marked.unwrap_or_else(|| estimated_sections(&analysis, duration));
    let section_of = |bar: usize| {
        let start = bars[bar].start;
        sections.iter().rev().find(|s| s["startMs"].as_f64().is_some_and(|t| t <= start + 0.5)).map(|s| s["name"].clone()).unwrap_or(Value::Null)
    };
    let mut levels: Vec<f64> = bars.iter().map(|b| b.db).collect();
    levels.sort_by(f64::total_cmp);
    let (quiet, loud) = (levels[levels.len() * 5 / 100], levels[levels.len() * 95 / 100]);
    let loudness = |d: f64| round(((d - quiet) / (loud - quiet).max(1.0)).clamp(0.0, 1.0), 2);
    let in_range = |t: f64| t >= from && t < to;
    let mut out = json!({
        "media": media,
        "durationMs": round(duration, 0),
        "assumedBeatsPerBar": 4,
        "barCount": bars.len(),
        "firstDownbeatMs": round(bars[0].start, 1),
        "tempoMap": analysis.tempo_map.iter().map(|(bar, bpm)| json!({"bar":bar + 1,"timeMs":round(bars[*bar].start, 0),"bpm":round(*bpm, 1)})).collect::<Vec<_>>(),
    });
    if include("bars") {
        out["bars"] = json!(bars
            .iter()
            .enumerate()
            .filter(|(_, b)| in_range(b.start) || in_range(b.start + b.length - 1.0))
            .map(|(i, b)| {
                let g = &analysis.grid[i];
                let mut row = json!({
                    "bar": i + 1,
                    "downbeatMs": round(b.start, 1),
                    "lengthMs": round(b.length, 1),
                    "tempoBpm": round(b.tempo, 1),
                    "beatsMs": b.beats.iter().map(|t| round(*t, 1)).collect::<Vec<_>>(),
                    "low": slot_text(&g[0]), "mid": slot_text(&g[1]), "high": slot_text(&g[2]),
                    "levelDb": round(b.db, 1),
                    "loudness": loudness(b.db),
                    "section": section_of(i),
                });
                if let Some((j, v)) = analysis.repeat_of[i] {
                    row["repeatOf"] = json!(j + 1);
                    row["repeatSimilarity"] = json!(round(v, 2));
                }
                row
            })
            .collect::<Vec<_>>());
        out["slotFormat"] = json!("low/mid/high: the bar's 16 sixteenth slots in 4 beat groups; digit 1-9 = strongest onset in that slot (strength 0..1, in ninths rounded up), . = none. loudness 0..1 spans the song's quietest 5% to loudest 5% of bars.");
    }
    if include("onsets") {
        let min = a.get("minStrength").and_then(Value::as_f64).unwrap_or(0.0);
        let mut bands = serde_json::Map::new();
        for (band, name) in onsets::BANDS.iter().enumerate() {
            let list: Vec<Value> = detected
                .iter()
                .filter(|o| o.band == band && o.strength >= min && in_range(o.time_ms))
                .map(|o| json!([round(o.time_ms, 0), round(o.strength, 2)]))
                .collect();
            bands.insert((*name).into(), json!(list));
        }
        out["onsets"] = Value::Object(bands);
        out["onsetFormat"] = json!("Per band, [timeMs, strength 0..1]; low <200 Hz, mid 200-2000 Hz, high >2000 Hz. Timed on a 1 ms band envelope, typically within 10 ms of the attack.");
    }
    if include("energy") {
        let step = a.get("energyStepMs").and_then(Value::as_f64).unwrap_or(250.0);
        let samples = audio.mono();
        let per = ((step / 1000.0 * f64::from(audio.sample_rate())).round() as usize).max(1);
        let first = (from.min(duration) / step) as usize;
        let values: Vec<f64> = samples
            .chunks(per)
            .enumerate()
            .skip(first)
            .take_while(|(i, _)| (*i as f64 * step) < to)
            .map(|(_, c)| round(db(c.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / c.len() as f64), 1))
            .collect();
        out["energy"] = json!({"startMs": first as f64 * step, "stepMs": step, "db": values});
    }
    if include("sections") {
        out["sections"] = json!(sections);
        out["sectionMethod"] = json!(if sections.first().is_some_and(|s| s["source"] == "marks") {
            format!("Read from the sequence's \"{collection_name}\" mark collection (each mark's text is a section name).")
        } else {
            "Estimate: boundaries where harmony, sound, rhythm or level change (novelty over bar similarity, aligned to 4-bar phrases); alike sections share a group letter; names are heuristic (unrepeated or short quiet ends = intro/outro, loudest repeated group = chorus, other repeated = verse, rest = bridge; drop = a loud, low-heavy burst after a much quieter section). Check them by ear; write corrected ones with sequence_section_marks.".into()
        });
    }
    if include("repeats") {
        out["repeats"] = json!(analysis
            .repeats
            .iter()
            .filter(|(i, len, _, _)| in_range(bars[*i].start) || in_range(bars[i + len - 1].start))
            .map(|(i, len, j, v)| json!({"bars":[i + 1, i + len],"sameAs":[j + 1, j + len],"similarity":round(*v, 2)}))
            .collect::<Vec<_>>());
    }
    Ok(out)
}

/// A song's music grid for porting old sequences (B79): every bar split into `per_bar` equal slots (16 for the 16ths of
/// music_rhythm's slot table, 8, 4 for beats, 1 for downbeats), from the same repaired downbeats, extended back to 0 at
/// the first bar's spacing. Sorted.
pub(super) fn grid_points(core: &AppCore, media: &str, per_bar: usize) -> Result<Vec<f64>, String> {
    Ok(bar_grid(&run(core, media)?.analysis.bars, per_bar))
}

fn bar_grid(bars: &[Bar], per_bar: usize) -> Vec<f64> {
    let Some(first) = bars.first() else { return Vec::new() };
    let step0 = first.length / per_bar as f64;
    let mut points: Vec<f64> = (1..).map(|k| first.start - step0 * k as f64).take_while(|t| *t >= -0.5).collect();
    points.reverse();
    for bar in bars {
        points.extend((0..per_bar).map(|k| bar.start + bar.length * k as f64 / per_bar as f64));
    }
    if let Some(last) = bars.last() {
        points.push(last.start + last.length);
    }
    points.sort_by(f64::total_cmp);
    points
}

pub(super) fn section_marks(core: &AppCore, shared: &Arc<Mutex<SequenceSession>>, a: &Value, agent: bool) -> Result<Value, String> {
    let name = if a.get("collection").is_some() { string(a, "collection")? } else { SECTIONS };
    let given = a.get("sections").and_then(Value::as_array);
    let needs_audio = given.is_none_or(|list| list.iter().any(|s| s.get("bar").is_some()));
    let analysed = if needs_audio {
        let media = match a.get("media") {
            Some(_) => filename(a, "media")?,
            None => sequence_audio(&shared.lock()).ok_or("The sequence has no audio; pass media, or give every section a startMs")?,
        };
        Some(run(core, &media)?)
    } else {
        None
    };
    let mut wanted: Vec<(f64, String)> = match given {
        Some(list) => list
            .iter()
            .map(|s| {
                let label = string(s, "name")?.to_string();
                let start = match (s.get("startMs").and_then(Value::as_f64), s.get("bar").and_then(Value::as_u64)) {
                    (Some(t), None) => t,
                    (None, Some(bar)) => {
                        let bars = &analysed.as_ref().unwrap().analysis.bars;
                        bars.get((bar as usize).wrapping_sub(1)).ok_or(format!("bar {bar} is outside 1..{}", bars.len()))?.start.round()
                    }
                    _ => return Err(format!("section \"{label}\": give exactly one of startMs or bar")),
                };
                Ok((start, label))
            })
            .collect::<Result<_, String>>()?,
        None => {
            let run = analysed.as_ref().unwrap();
            estimated_sections(&run.analysis, run.duration)
                .iter()
                .map(|s| (s["startMs"].as_f64().unwrap(), s["name"].as_str().unwrap().to_string()))
                .collect()
        }
    };
    wanted.sort_by(|x, y| x.0.total_cmp(&y.0));
    if wanted.windows(2).any(|w| w[1].0 - w[0].0 < 1.0) {
        return Err("Two sections start at the same time".into());
    }
    let mut s = shared.lock();
    let length = s.length().total_ms();
    if given.is_none() {
        // An estimate covers the whole song; a sequence shorter than its media gets the sections that start in it.
        wanted.retain(|(t, _)| *t < length);
    }
    if let Some((start, label)) = wanted.iter().find(|(t, _)| *t >= length) {
        return Err(format!("section \"{label}\" starts at {start} ms, at or after the sequence end ({length} ms)"));
    }
    let marks: Vec<Mark> = wanted
        .iter()
        .enumerate()
        .map(|(i, (start, label))| {
            let end = wanted.get(i + 1).map_or(length, |n| n.0);
            let mut mark = Mark::new(TimeSpan::from_ms(*start));
            mark.duration = TimeSpan::from_ms((end - start).max(1.0));
            mark.text = label.clone();
            mark
        })
        .collect();
    let mut old = s.marks().to_vec();
    let mut collections = old.clone();
    let id = match collections.iter_mut().find(|c| c.name == name) {
        Some(existing) => {
            if a["replace"] == false {
                return Err(format!("A mark collection named \"{name}\" exists; pass replace:true or another collection name"));
            }
            existing.marks = marks;
            existing.sort_marks();
            existing.id
        }
        None => {
            let mut collection = MarkCollection::default();
            collection.name = name.to_string();
            collection.decorator.color = Color::rgb(240, 190, 60);
            collection.marks = marks;
            collection.sort_marks();
            // Undo restores marks into collections it knows and never removes a collection, so it gets the new one empty.
            old.push(MarkCollection { marks: Vec::new(), ..collection.clone() });
            let id = collection.id;
            collections.push(collection);
            id
        }
    };
    s.set_marks(collections);
    record(&mut s, "Section marks", UndoAction::MarksChanged { label: "Section marks".into(), marks: old }, agent);
    let bars = analysed.as_ref().map(|r| &r.analysis.bars);
    Ok(json!({
        "collectionId": id.to_string(),
        "name": name,
        "source": if given.is_some() { "given" } else { "estimate" },
        "media": analysed.as_ref().map(|r| r.media.clone()),
        "sections": wanted.iter().enumerate().map(|(i, (start, label))| {
            let mut item = json!({"name":label,"startMs":start,"endMs":wanted.get(i + 1).map_or(length, |n| n.0)});
            if let Some(bars) = bars {
                item["startBar"] = json!(bar_at(bars, *start));
            }
            item
        }).collect::<Vec<_>>(),
    }))
}

pub(super) const RHYTHM_DESCRIPTION: &str = "Rhythm analysis of profile media (media) or a sequence's audio (sequenceId), for planning a show by bar. Returns: tempoMap (tempo segments; a change is a move of more than 4% that lasts), firstDownbeatMs, and bars: one row per bar (bar numbers from 1 at the first downbeat; 4/4 assumed) with downbeatMs, lengthMs, tempoBpm, beatsMs, the 16th-slot onset table as text per band (low/mid/high, e.g. \"9... 3... 9.3. 3...\": a digit 1-9 for the strongest hit in that slot, . for none), levelDb and loudness 0..1, its section, and repeatOf (the earlier bar it best repeats, with repeatSimilarity) when it repeats one; onsets: per band [timeMs, strength] at 1 ms precision (typically within 10 ms); energy: an RMS level curve in dB every energyStepMs (default 250); sections: named sections with start/end ms and bars, and a group letter shared by alike ones (estimated, or read from the sequence's \"Sections\" mark collection when it has one, so corrections written with sequence_section_marks come back); repeats: runs of 4+ bars that repeat earlier bars. include picks parts (default all); rangeMs [startMs,endMs] limits bars, onsets, energy and repeats to a time range (the analysis still covers the whole song); minStrength drops weaker onsets. Downbeats, sections and repeats are estimates: check them before relying on them. Reads only; no playback.";

pub(super) const SECTION_MARKS_DESCRIPTION: &str = "Write named section marks (intro, verse, chorus, bridge, drop, outro or any name) to a sequence as one mark collection, in one undo step: each mark starts a section, lasts until the next, and its text is the section's name. sections [{name, startMs|bar}] gives them (bar is a 1-based bar number from music_rhythm, placed on that bar's downbeat); without sections the estimate from music_rhythm is written. The collection is \"Sections\" unless collection names another; an existing collection of that name is replaced (replace:false refuses instead). music_rhythm reads the collection back, so hand-moved or retyped marks are what later analysis reports. Returns the collection id and each section's start, end and bar.";

pub(super) fn rhythm_schema() -> Value {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    json!({
        "media": {"type":"string","minLength":1,"maxLength":256,"examples":["Darude_Sandstorm.mp3"]},
        "sequenceId": string,
        "include": {"type":"array","items":{"type":"string","enum":["bars","onsets","energy","sections","repeats"]},"minItems":1,"maxItems":5,"examples":[["bars","sections"],["onsets"]]},
        "rangeMs": {"type":"array","items":{"type":"number","minimum":0,"maximum":3600000},"minItems":2,"maxItems":2,"examples":[[30000,60000]]},
        "energyStepMs": {"type":"number","minimum":20,"maximum":5000,"examples":[250]},
        "minStrength": {"type":"number","minimum":0,"maximum":1,"examples":[0.3]},
        "sectionsCollection": {"type":"string","minLength":1,"maxLength":256,"examples":["Sections"]},
    })
}

pub(super) fn section_marks_schema() -> Value {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    json!({
        "sequenceId": string,
        "sections": {"type":"array","minItems":1,"maxItems":200,"items":{"type":"object","properties":{
            "name":{"type":"string","minLength":1,"maxLength":64},
            "startMs":{"type":"number","minimum":0,"maximum":3600000},
            "bar":{"type":"integer","minimum":1,"maximum":10000}},"required":["name"],"additionalProperties":false},
            "examples":[[{"name":"Intro","startMs":0},{"name":"Verse 1","bar":5},{"name":"Chorus 1","bar":21},{"name":"Drop","startMs":61250}]]},
        "media": {"type":"string","minLength":1,"maxLength":256},
        "collection": {"type":"string","minLength":1,"maxLength":256,"examples":["Sections"]},
        "replace": {"type":"boolean"},
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 22_050;

    /// A synthetic song in 4/4: each part is (bars, ms per bar, loud?). Quiet parts: a kick on every beat and an A pad.
    /// Loud parts: kick, snare on 2 and 4, 8th-note hats and a C major pad, 12 dB louder.
    fn song(parts: &[(usize, f64, bool)]) -> (Vec<f32>, Vec<f64>, Vec<f64>) {
        let total: f64 = parts.iter().map(|(n, len, _)| *n as f64 * len).sum::<f64>() + 500.0;
        let mut out = vec![0.0f32; (total / 1000.0 * f64::from(SR)) as usize];
        let (mut bars, mut beats) = (Vec::new(), Vec::new());
        let mut t = 200.0;
        let mut seed = 99u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            f64::from(seed >> 8) / f64::from(1u32 << 24) * 2.0 - 1.0
        };
        let at = |ms: f64| (ms / 1000.0 * f64::from(SR)) as usize;
        for &(count, len, loud) in parts {
            let gain = if loud { 1.0 } else { 0.25 };
            let pad: &[f64] = if loud { &[261.63, 329.63, 392.0] } else { &[220.0, 277.18] };
            for _ in 0..count {
                bars.push(t);
                for b in 0..4 {
                    let beat = t + b as f64 * len / 4.0;
                    beats.push(beat);
                    for i in 0..at(120.0) {
                        let x = i as f64 / f64::from(SR);
                        if let Some(s) = out.get_mut(at(beat) + i) {
                            *s += (gain * 0.6 * (-x / 0.04).exp() * (2.0 * std::f64::consts::PI * 55.0 * x).sin()) as f32;
                        }
                    }
                    if loud && b % 2 == 1 {
                        for i in 0..at(100.0) {
                            if let Some(s) = out.get_mut(at(beat) + i) {
                                *s += (0.3 * (-(i as f64) / f64::from(SR) / 0.03).exp() * noise()) as f32;
                            }
                        }
                    }
                    if loud {
                        for half in 0..2 {
                            let hat = at(beat + half as f64 * len / 8.0);
                            // Differenced noise: a hat's energy sits above 2 kHz.
                            let mut last = 0.0;
                            for i in 0..at(60.0) {
                                let v = noise();
                                if let Some(s) = out.get_mut(hat + i) {
                                    *s += (0.5 * (-(i as f64) / f64::from(SR) / 0.02).exp() * (v - last)) as f32;
                                }
                                last = v;
                            }
                        }
                    }
                }
                for i in at(t)..at(t + len).min(out.len()) {
                    let x = i as f64 / f64::from(SR);
                    out[i] += (gain * 0.08 * pad.iter().map(|f| (2.0 * std::f64::consts::PI * f * x).sin()).sum::<f64>()) as f32;
                }
                t += len;
            }
        }
        (out, bars, beats)
    }

    fn run_song(parts: &[(usize, f64, bool)]) -> Analysis {
        let (samples, bars, beats) = song(parts);
        let detected = onsets::detect(&samples, SR);
        analyze(&samples, SR, &bars, &beats, &detected)
    }

    #[test]
    fn verse_chorus_song_has_sections_repeats_and_slots() {
        let a = run_song(&[(8, 2000.0, false), (8, 2000.0, true), (8, 2000.0, false), (8, 2000.0, true)]);
        assert_eq!(a.bars.len(), 32);
        let starts: Vec<usize> = a.sections.iter().map(|s| s.bars.0).collect();
        assert_eq!(starts, vec![0, 8, 16, 24], "{:?}", a.sections.iter().map(|s| &s.label).collect::<Vec<_>>());
        let kinds: Vec<&str> = a.sections.iter().map(|s| s.kind).collect();
        assert_eq!(kinds, vec!["verse", "chorus", "verse", "chorus"]);
        assert_eq!(a.sections[3].label, "Chorus 2");
        assert_eq!(a.sections[0].group, a.sections[2].group);
        assert_ne!(a.sections[0].group, a.sections[1].group);
        // The second chorus repeats the first, bar for bar.
        assert!(a.repeats.iter().any(|(i, len, j, _)| *i <= 24 && i + len >= 28 && i - j == 16), "{:?}", a.repeats);
        for bar in 24..28 {
            assert_eq!(a.repeat_of[bar].map(|r| r.0), Some(bar - 16), "{:?}", a.repeats);
        }
        // A chorus bar: kick on every beat (low; the snare's and the kick tail's faint low bleed stays below 0.3).
        let row = &a.grid[12];
        let strong: Vec<usize> = (0..16).filter(|s| row[0][*s] >= 0.5).collect();
        assert_eq!(strong, vec![0, 4, 8, 12], "{}", slot_text(&row[0]));
        assert!((0..16).filter(|s| s % 4 != 0).all(|s| row[0][s] < 0.3), "{}", slot_text(&row[0]));
        // High: every 8th-note hat of the chorus, in both choruses (late bars too), and nothing in a verse bar.
        for bar in [8, 12, 15, 24, 31] {
            let high = &a.grid[bar][2];
            assert!((0..16).all(|s| (high[s] >= 0.5) == (s % 2 == 0)), "bar {bar}: {}", slot_text(high));
        }
        assert!(a.grid[4][2].iter().all(|v| *v == 0.0), "{}", slot_text(&a.grid[4][2]));
        assert!(a.bars[12].db > a.bars[4].db + 6.0);
        assert!((a.bars[3].tempo - 120.0).abs() < 0.5);
    }

    #[test]
    fn tempo_changes_split_the_tempo_map() {
        let a = run_song(&[(8, 2000.0, false), (8, 1500.0, false)]);
        assert_eq!(a.tempo_map.len(), 2, "{:?}", a.tempo_map);
        assert_eq!(a.tempo_map[1].0, 8);
        assert!((a.tempo_map[0].1 - 120.0).abs() < 1.0 && (a.tempo_map[1].1 - 160.0).abs() < 2.0, "{:?}", a.tempo_map);
    }

    #[test]
    fn bar_grid_splits_bars_and_reaches_back_to_zero() {
        let bar = |start: f64| Bar { start, length: 2000.0, beats: Vec::new(), tempo: 120.0, db: 0.0 };
        let points = bar_grid(&[bar(1000.0), bar(3000.0)], 4);
        assert_eq!(points, [0.0, 500.0, 1000.0, 1500.0, 2000.0, 2500.0, 3000.0, 3500.0, 4000.0, 4500.0, 5000.0]);
        assert_eq!(bar_grid(&[bar(1000.0)], 16)[..3], [0.0, 125.0, 250.0]);
        assert!(bar_grid(&[], 16).is_empty());
    }

    #[test]
    fn slot_text_shows_strength_per_beat() {
        let mut slots = [0.0; 16];
        slots[0] = 1.0;
        slots[6] = 0.3;
        slots[15] = 0.01;
        assert_eq!(slot_text(&slots), "9... ..3. .... ...1");
    }

    #[test]
    fn section_marks_write_replace_and_undo() {
        use super::super::execute;
        let root = std::env::temp_dir().join(format!("studio-section-marks-{}", uuid::Uuid::new_v4()));
        let core = AppCore::open(root.clone(), true).unwrap();
        let run = |name: &str, args: Value| execute(&core, name, &args, true).unwrap();
        let fail = |name: &str, args: Value| execute(&core, name, &args, true).unwrap_err();
        let id = run("sequence_create", json!({"lengthMs":60000}))["sequenceId"].clone();
        let sections = |v: &Value| -> Vec<(f64, f64, String)> {
            v.as_array().unwrap().iter().filter(|c| c["name"] == SECTIONS).flat_map(|c| c["marks"].as_array().unwrap().iter())
                .map(|m| (m["timeMs"].as_f64().unwrap(), m["durationMs"].as_f64().unwrap(), m["text"].as_str().unwrap().to_string())).collect()
        };
        let marks = || run("sequence_marks", json!({"sequenceId":id}));
        let written = run("sequence_section_marks", json!({"sequenceId":id,"sections":[{"name":"Chorus","startMs":30000},{"name":"Intro","startMs":0}]}));
        assert_eq!(written["source"], "given");
        assert_eq!(sections(&marks()), vec![(0.0, 30000.0, "Intro".into()), (30000.0, 30000.0, "Chorus".into())]);
        // Replacing keeps one collection; undo brings the first set back, a second undo empties it.
        run("sequence_section_marks", json!({"sequenceId":id,"sections":[{"name":"Drop","startMs":45000}]}));
        assert_eq!(sections(&marks()), vec![(45000.0, 15000.0, "Drop".into())]);
        assert_eq!(marks().as_array().unwrap().iter().filter(|c| c["name"] == SECTIONS).count(), 1);
        assert!(fail("sequence_section_marks", json!({"sequenceId":id,"replace":false,"sections":[{"name":"X","startMs":0}]})).contains("replace"));
        assert!(fail("sequence_section_marks", json!({"sequenceId":id,"sections":[{"name":"Late","startMs":60000}]})).contains("sequence end"));
        assert!(fail("sequence_section_marks", json!({"sequenceId":id,"sections":[{"name":"A","startMs":10},{"name":"B","startMs":10}]})).contains("same time"));
        assert!(fail("sequence_section_marks", json!({"sequenceId":id,"sections":[{"name":"V","bar":2}]})).contains("no audio"));
        assert!(fail("music_rhythm", json!({})).contains("media"));
        run("sequence_undo", json!({"sequenceId":id}));
        assert_eq!(sections(&marks()).len(), 2);
        run("sequence_undo", json!({"sequenceId":id}));
        assert!(sections(&marks()).is_empty());
        core.studio_display.stop();
        drop(core);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_bars_no_sections() {
        let a = analyze(&vec![0.0; 22_050], SR, &[], &[], &[]);
        assert!(a.bars.is_empty() && a.sections.is_empty() && a.tempo_map.is_empty());
    }
}
