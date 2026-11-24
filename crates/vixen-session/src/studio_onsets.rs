//! Per-band spectral-flux onset detection for music_analyze, and placement of onsets on the bar/16th grid.
use vixen_analysis::fft::FftReal;

pub const BANDS: [&str; 3] = ["low", "mid", "high"];
/// Band edges in Hz: low < 200, mid 200..2000, high >= 2000.
const EDGES: [f64; 2] = [200.0, 2000.0];
pub const SLOTS_PER_BAR: usize = 16;

#[derive(Clone, Debug, PartialEq)]
pub struct Onset {
    pub time_ms: f64,
    pub band: usize,
    /// Peak flux relative to the band's strong onsets, 0..1.
    pub strength: f64,
}

/// Onsets in each band, sorted by time. Detection: STFT with a ~46 ms Hann window and a hop of an eighth of it (~6 ms),
/// log magnitude scaled to each band's own level (so another band's leakage stays small), half-wave rectified flux
/// summed per band, adaptive median + MAD threshold over about a second, local-maximum peak picking. Each peak is then
/// timed on a 1 ms energy envelope of the band-filtered signal, where the attack is sharpest.
pub fn detect(samples: &[f32], sample_rate: u32) -> Vec<Onset> {
    let sr = f64::from(sample_rate);
    if sr <= 0.0 {
        return Vec::new();
    }
    let n = ((sr * 0.0464).round() as usize).max(64).next_power_of_two();
    let hop = n / 8;
    if samples.len() < n {
        return Vec::new();
    }
    let Some(mut fft) = FftReal::new(n) else { return Vec::new() };
    let window: Vec<f64> =
        (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()).collect();
    let bin_hz = sr / n as f64;
    let band_of = |k: usize| EDGES.iter().filter(|e| k as f64 * bin_hz >= **e).count();
    let frames = (samples.len() - n) / hop + 1;
    let (mut input, mut re, mut im) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    let mut spectrum = |frame: usize, magnitude: &mut [f64]| {
        let start = frame * hop;
        for (i, x) in input.iter_mut().enumerate() {
            *x = f64::from(samples[start + i]) * window[i];
        }
        fft.forward(&input, &mut re, &mut im);
        for k in 1..=n / 2 {
            magnitude[k] = (re[k] * re[k] + im[k] * im[k]).sqrt();
        }
    };
    // Pass 1: each band's mean bin magnitude sets its log scale.
    let mut magnitude = vec![0.0; n / 2 + 1];
    let (mut sums, mut counts) = ([0.0; 3], [0usize; 3]);
    for frame in 0..frames {
        spectrum(frame, &mut magnitude);
        for (k, m) in magnitude.iter().enumerate().skip(1) {
            sums[band_of(k)] += m;
            counts[band_of(k)] += 1;
        }
    }
    let overall = sums.iter().sum::<f64>() / counts.iter().sum::<usize>().max(1) as f64;
    if overall <= 0.0 {
        return Vec::new();
    }
    let scale: Vec<f64> =
        (0..3).map(|b| 0.01 * (sums[b] / counts[b].max(1) as f64).max(0.01 * overall)).collect();
    // Pass 2: flux.
    let mut previous = vec![0.0; n / 2 + 1];
    let mut current = vec![0.0; n / 2 + 1];
    let mut flux = vec![vec![0.0; frames]; BANDS.len()];
    for frame in 0..frames {
        spectrum(frame, &mut magnitude);
        for k in 1..=n / 2 {
            let band = band_of(k);
            current[k] = (1.0 + magnitude[k] / scale[band]).ln();
            if frame > 0 {
                flux[band][frame] += (current[k] - previous[k]).max(0.0);
            }
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let frame_ms = |f: usize| (f * hop + n / 2) as f64 * 1000.0 / sr;
    let hop_ms = hop as f64 * 1000.0 / sr;
    let half = (500.0 / hop_ms).round() as usize;
    let near = (30.0 / hop_ms).round().max(1.0) as usize;
    let envelopes = band_envelopes(samples, sr);
    let loud: Vec<f64> = envelopes.iter().map(|e| loud_level(e)).collect();
    let mut onsets = Vec::new();
    for (band, values) in flux.iter().enumerate() {
        let mean = values.iter().sum::<f64>() / values.len().max(1) as f64;
        let mut peaks: Vec<(f64, f64)> = Vec::new();
        let mut scratch = Vec::with_capacity(2 * half + 1);
        for t in 1..values.len().saturating_sub(1) {
            let v = values[t];
            let (lo, hi) = (t.saturating_sub(near), (t + near + 1).min(values.len()));
            if v <= 0.0 || values[lo..hi].iter().any(|w| *w > v) || values[t - 1] == v {
                continue;
            }
            scratch.clear();
            scratch.extend_from_slice(&values[t.saturating_sub(half)..(t + half + 1).min(values.len())]);
            let middle = median(&mut scratch);
            scratch.iter_mut().for_each(|x| *x = (*x - middle).abs());
            let mad = median(&mut scratch);
            if v <= middle + 3.0 * mad + 0.1 * mean {
                continue;
            }
            // Spectral smear from another band's attack moves the flux but not this band's filtered energy.
            let Some(time) = refine(&envelopes[band], loud[band], frame_ms(t)) else { continue };
            // Two peaks timed onto the same attack: keep the stronger.
            match peaks.last_mut() {
                Some(last) if time - last.0 < MIN_GAP_MS => {
                    if v > last.1 {
                        *last = (time, v);
                    }
                }
                _ => peaks.push((time, v)),
            }
        }
        let mut heights: Vec<f64> = peaks.iter().map(|p| p.1).collect();
        heights.sort_by(f64::total_cmp);
        let reference = heights.get(heights.len() * 95 / 100).copied().unwrap_or(1.0).max(f64::MIN_POSITIVE);
        onsets.extend(peaks.into_iter().map(|(time_ms, v)| Onset { time_ms, band, strength: (v / reference).min(1.0) }));
    }
    onsets.sort_by(|a, b| a.time_ms.total_cmp(&b.time_ms));
    onsets
}

const MIN_GAP_MS: f64 = 50.0;

/// Energy per millisecond of each band: 4th-order Butterworth low-pass at 200 Hz, 200-2000 Hz band-pass, high-pass at
/// 2000 Hz. Each sample goes to the millisecond it falls in (a whole-sample block would drift: 44 samples at 44.1 kHz
/// put a minute in at 60.14 s).
fn band_envelopes(samples: &[f32], sr: f64) -> Vec<Vec<f64>> {
    let ms = |i: usize| (i as f64 * 1000.0 / sr) as usize;
    let filters = [
        vec![Biquad::new(sr, EDGES[0], false), Biquad::new(sr, EDGES[0], false)],
        vec![Biquad::new(sr, EDGES[0], true), Biquad::new(sr, EDGES[1], false)],
        vec![Biquad::new(sr, EDGES[1], true), Biquad::new(sr, EDGES[1], true)],
    ];
    filters
        .into_iter()
        .map(|mut chain| {
            let mut energy = vec![0.0; ms(samples.len().saturating_sub(1)) + 1];
            for (i, s) in samples.iter().enumerate() {
                let y = chain.iter_mut().fold(f64::from(*s), |x, f| f.process(x));
                energy[ms(i)] += y * y;
            }
            energy
        })
        .collect()
}

/// Mean energy of the loudest 1% of a band's milliseconds.
fn loud_level(energy: &[f64]) -> f64 {
    let mut sorted = energy.to_vec();
    sorted.sort_by(|a, b| b.total_cmp(a));
    let top = &sorted[..(sorted.len() / 100).max(1).min(sorted.len())];
    top.iter().sum::<f64>() / top.len().max(1) as f64
}

/// The millisecond near a flux peak where the band's energy rises most (4 ms after against 4 ms before), or None when it
/// rises by less than half or stays 30 dB below the band's loud level. The flux frame is ~46 ms long, so the attack lies within about half a window of its
/// reference time.
fn refine(energy: &[f64], loud: f64, time_ms: f64) -> Option<f64> {
    const SPAN: usize = 4;
    let floor = 1e-3 * energy.iter().sum::<f64>() / energy.len().max(1) as f64 + 1e-12;
    let centre = time_ms.round() as isize;
    let (from, to) = ((centre - 30).max(SPAN as isize) as usize, (centre + 40).max(0) as usize);
    let to = to.min(energy.len().saturating_sub(SPAN));
    (from..to)
        .map(|b| {
            let after: f64 = energy[b..b + SPAN].iter().sum();
            let before: f64 = energy[b - SPAN..b].iter().sum();
            (b, after, (after + floor).ln() - (before + floor).ln())
        })
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .filter(|(_, after, rise)| *rise >= 1.5f64.ln() && *after >= 1e-3 * loud * SPAN as f64)
        .map(|(b, _, _)| b as f64)
}

/// RBJ second-order Butterworth low- or high-pass.
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn new(sr: f64, cutoff: f64, high: bool) -> Self {
        let w = 2.0 * std::f64::consts::PI * (cutoff / sr).min(0.49);
        let (cos, alpha) = (w.cos(), w.sin() / std::f64::consts::SQRT_2);
        let a0 = 1.0 + alpha;
        let b = if high { [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0] } else { [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0] };
        Biquad { b: b.map(|v| v / a0), a: [-2.0 * cos / a0, (1.0 - alpha) / a0], x: [0.0; 2], y: [0.0; 2] }
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mid = values.len() / 2;
    *values.select_nth_unstable_by(mid, f64::total_cmp).1
}

/// Sorted, with marks closer than `tolerance_ms` to the previous one dropped.
pub fn dedupe(mut times: Vec<f64>, tolerance_ms: f64) -> Vec<f64> {
    times.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::with_capacity(times.len());
    for t in times {
        if out.last().is_none_or(|last| t - last >= tolerance_ms) {
            out.push(t);
        }
    }
    out
}

/// Bar index, 16th slot and offset from that slot (ms) for a time, given sorted bar starts. The last bar is assumed as
/// long as the median bar. A time nearer the next bar's first slot belongs to that bar. None before the first bar.
pub fn slot(bars: &[f64], time_ms: f64) -> Option<(usize, usize, f64)> {
    let mut lengths: Vec<f64> = bars.windows(2).map(|w| w[1] - w[0]).filter(|d| *d > 0.0).collect();
    lengths.sort_by(f64::total_cmp);
    let typical = *lengths.get(lengths.len() / 2)?;
    slot_with(bars, time_ms, typical)
}

fn slot_with(bars: &[f64], time_ms: f64, typical: f64) -> Option<(usize, usize, f64)> {
    let first = *bars.first()?;
    let step0 = bars.get(1).map_or(typical, |b| b - first) / SLOTS_PER_BAR as f64;
    if time_ms < first - step0 / 2.0 {
        return None;
    }
    let bar = bars.partition_point(|b| *b <= time_ms).saturating_sub(1);
    let start = bars[bar];
    let length = bars.get(bar + 1).map_or(typical, |b| b - start);
    let step = length / SLOTS_PER_BAR as f64;
    let index = ((time_ms - start) / step).round();
    if index < 0.0 {
        return Some((0, 0, time_ms - first));
    }
    let index = index as usize;
    if index >= SLOTS_PER_BAR {
        if bar + 1 < bars.len() {
            return Some((bar + 1, 0, time_ms - bars[bar + 1]));
        }
        if time_ms > start + length + step / 2.0 {
            return None;
        }
        return Some((bar, SLOTS_PER_BAR - 1, time_ms - (start + step * (SLOTS_PER_BAR - 1) as f64)));
    }
    Some((bar, index, time_ms - (start + step * index as f64)))
}

/// For each bar, the strongest onset in each band's 16 slots.
pub fn slot_grid(bars: &[f64], onsets: &[Onset]) -> Vec<[[f64; SLOTS_PER_BAR]; 3]> {
    let mut grid = vec![[[0.0f64; SLOTS_PER_BAR]; 3]; bars.len()];
    for onset in onsets {
        if let Some((bar, slot, _)) = slot(bars, onset.time_ms) {
            let cell = &mut grid[bar][onset.band][slot];
            *cell = cell.max(onset.strength);
        }
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decaying noise bursts (broadband) and decaying sine bursts at the given frequency.
    fn bursts(sr: u32, seconds: f64, times_ms: &[f64], freq: Option<f64>) -> Vec<f32> {
        let mut out = vec![0.0f32; (f64::from(sr) * seconds) as usize];
        let mut seed = 12345u32;
        for &t in times_ms {
            let start = (t / 1000.0 * f64::from(sr)) as usize;
            for i in 0..(f64::from(sr) * 0.08) as usize {
                let Some(sample) = out.get_mut(start + i) else { break };
                let env = (-(i as f64) / (f64::from(sr) * 0.015)).exp();
                let value = match freq {
                    Some(f) => (2.0 * std::f64::consts::PI * f * i as f64 / f64::from(sr)).sin(),
                    None => {
                        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                        f64::from(seed >> 8) / f64::from(1u32 << 24) * 2.0 - 1.0
                    }
                };
                *sample += (0.5 * env * value) as f32;
            }
        }
        out
    }

    #[test]
    fn onsets_are_found_within_ten_milliseconds() {
        // 148 BPM 16ths (~101 ms apart) with an irregular start, so 100 ms quantization could not pass.
        let times: Vec<f64> = (0..40).map(|i| 337.0 + i as f64 * 60_000.0 / 148.0 / 4.0).collect();
        for sr in [44_100, 48_000, 22_050] {
            let found = detect(&bursts(sr, 6.0, &times, None), sr);
            for t in &times {
                let nearest = found.iter().map(|o| (o.time_ms - t).abs()).fold(f64::INFINITY, f64::min);
                assert!(nearest <= 10.0, "{sr} Hz: no onset within 10 ms of {t} (nearest {nearest})");
            }
        }
    }

    #[test]
    fn late_onsets_stay_on_time() {
        // A steady 8th-note line for a minute: timing must not drift with the position in the track.
        let times: Vec<f64> = (0..240).map(|i| 250.0 + i as f64 * 250.0).collect();
        for sr in [44_100, 22_050] {
            let found = detect(&bursts(sr, 61.0, &times, None), sr);
            for t in &times {
                let nearest = found.iter().map(|o| (o.time_ms - t).abs()).fold(f64::INFINITY, f64::min);
                assert!(nearest <= 10.0, "{sr} Hz: no onset within 10 ms of {t} (nearest {nearest})");
            }
        }
    }

    #[test]
    fn bands_separate_bass_and_treble() {
        let sr = 44_100;
        let kicks = [500.0, 1500.0, 2500.0];
        let hats = [1000.0, 2000.0, 3000.0];
        let mut audio = bursts(sr, 4.0, &kicks, Some(60.0));
        for (a, b) in audio.iter_mut().zip(bursts(sr, 4.0, &hats, Some(6000.0))) {
            *a += b;
        }
        let found = detect(&audio, sr);
        let near = |band: usize, t: f64| found.iter().any(|o| o.band == band && (o.time_ms - t).abs() <= 12.0);
        assert!(kicks.iter().all(|t| near(0, *t)), "{found:?}");
        assert!(hats.iter().all(|t| near(2, *t)), "{found:?}");
        assert!(!hats.iter().any(|t| near(0, *t)), "{found:?}");
        assert!(found.iter().all(|o| o.strength > 0.0 && o.strength <= 1.0));
    }

    #[test]
    fn silence_has_no_onsets() {
        assert!(detect(&vec![0.0; 44_100 * 2], 44_100).is_empty());
        assert!(detect(&[0.1; 100], 44_100).is_empty());
    }

    #[test]
    fn duplicate_beats_are_dropped() {
        assert_eq!(dedupe(vec![81.0, 81.0, 313.0, 81.4, 545.0], 1.0), vec![81.0, 313.0, 545.0]);
    }

    #[test]
    fn times_land_in_bar_slots() {
        let bars = [1000.0, 2600.0, 4200.0];
        assert_eq!(slot(&bars, 900.0), None);
        assert_eq!(slot(&bars, 1000.0), Some((0, 0, 0.0)));
        assert_eq!(slot(&bars, 1608.0), Some((0, 6, 8.0)));
        assert_eq!(slot(&bars, 2560.0), Some((1, 0, -40.0)));
        assert_eq!(slot(&bars, 5700.0), Some((2, 15, 0.0)));
        assert_eq!(slot(&bars, 9000.0), None);
        let grid = slot_grid(&bars, &[Onset { time_ms: 1608.0, band: 2, strength: 0.5 }]);
        assert_eq!(grid[0][2][6], 0.5);
    }
}
