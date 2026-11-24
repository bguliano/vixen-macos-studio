//! render_analyze: automated taste checks over a rendered span, from the schedule's exact colours rather than a decoded
//! video. It finds the things a viewer rejects a sequence for: dark stretches, a prop (or the whole show) changing too
//! many times a second ("spazzing"), visual changes that land on no mark, and sections whose colours stray from the
//! palette or lean blue/purple. Every finding carries a sequence time an agent can jump to.
use super::{activity, activity::Thresholds, Renderer};
use serde_json::{json, Value};

/// What counts as a light changing from one frame to the next, and as a group changing.
#[derive(Clone, Copy)]
pub struct Change {
    /// A level jump (0 to 1) that is a change on its own.
    pub level: f32,
    /// A colour jump between two lit frames (distance between full-brightness colours, 0 to about 1.7).
    pub color: f32,
    /// The share of a group's lights (0 to 1) that must change in one frame for the group to change.
    pub share: f32,
}

impl Change {
    pub const DEFAULT: Change = Change { level: 0.25, color: 0.35, share: 0.3 };
}

/// A section the colour checks report on.
pub struct Section {
    pub start_ms: f64,
    pub end_ms: f64,
    pub name: String,
}

/// Everything the command layer resolved: thresholds, marks, sections and palette.
pub struct Input {
    pub dark: Thresholds,
    pub change: Change,
    /// More changes than this per second, over `window_ms`, is spazzing.
    pub max_per_sec: f64,
    pub window_ms: f64,
    /// Sorted mark times to check changes against, with the collections' names; `None` skips the check.
    pub marks: Option<(Vec<f64>, Vec<String>)>,
    pub tolerance_ms: f64,
    /// The share of off-mark changes above which the check is flagged.
    pub off_mark_max: f64,
    pub sections: Vec<Section>,
    /// The sequence palette as `#RRGGBB` (empty skips the palette check).
    pub palette: Vec<String>,
    pub palette_distance: f32,
    pub off_palette_max: f64,
    pub blue_purple_max: f64,
}

pub const DESCRIPTION: &str = "Automated taste checks on a rendered span, from the schedule's exact colours (no video): what a viewer rejects a sequence for, each with times to jump to. Read-only: it changes nothing, so there is nothing to undo. Reports, for the preview's linked lights (or scope's lights in it): dark, every stretch of at least minMs where fewer than minLit of the lights are at level or brighter (as sequence_activity); changes, frames where at least changeShare of the lights visibly change (a level jump of changeLevel, or a colour jump of changeColor while lit), with perSec mean/peak and spazz, the stretches with more than maxChangesPerSec changes in some windowMs; marks, how many change onsets (lights coming on or changing colour; going dark isn't one) land on no mark (within toleranceMs) of the marks collections (default every collection with marks; [] skips), flagged above offMarkMax; sections, every sectionMs (or one per mark of the sections collection, named by mark text) with litMean, the top 3 dominant hues weighted by brightness (red, orange, yellow, green, cyan, blue, purple, magenta, white, each with share and average hex), bluePurpleShare (flagged at bluePurpleMax) and, with palette, offPaletteShare (lit colour further than paletteDistance from every palette colour, flagged above offPaletteMax) and offPaletteColors. elements (element or group ids) adds dark, changes and marks per prop. Top level: ok (nothing flagged), flags (counts), and jumpTo, every finding sorted by time (timeMs, m:ss.cc time, kind dark|spazz|offMark|bluePurple|offPalette, detail, element). Defaults: whole sequence, 20 fps, level 0.1, minLit 0.05, minMs 1000, changeLevel 0.25, changeColor 0.35, changeShare 0.3, maxChangesPerSec 6, windowMs 1000, toleranceMs 50, offMarkMax 0.25, sectionMs 10000, paletteDistance 0.35, offPaletteMax 0.15, bluePurpleMax 0.5. Change counts are per sampled frame, so keep fps at 20 or more for spazz checks. Example: {\"sequenceId\":\"...\",\"previewId\":\"...\",\"scope\":\"<megatree id>\",\"marks\":[\"Music - beats\"],\"sections\":\"Sections\",\"palette\":[\"#ff0000\",\"#ffffff\",\"#00ff00\"]}. Without palette it checks against the sequence's own palette (sequence_palette) and its distance; paletteFrom says which was used (argument, sequence or none).";

pub fn schema() -> Value {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let ms = json!({"type":"number","minimum":0,"maximum":3600000});
    let share = |text: &str, example: f64| json!({"type":"number","minimum":0,"maximum":1,"description":text,"examples":[example]});
    let id_list = json!({"type":"array","items":string,"maxItems":64,"description":"Element or group ids to check one by one","examples":[["<prop id>"]]});
    json!({
        "sequenceId": string,
        "previewId": string,
        "startMs": ms,
        "endMs": ms,
        "fps": {"type":"number","minimum":5,"maximum":60,"description":"Frames sampled per second","examples":[20]},
        "scope": {"type":"string","minLength":1,"maxLength":256,"description":"Check only this element's (or group's) lights instead of the whole preview","examples":["<megatree id>"]},
        "elements": id_list,
        "level": share("A light's level at which it counts as lit", 0.1),
        "minLit": share("The share of lights that must be lit for a frame not to be dark", 0.05),
        "minMs": {"type":"number","minimum":0,"maximum":600000,"description":"Shortest dark stretch reported","examples":[1000]},
        "changeLevel": {"type":"number","minimum":0.01,"maximum":1,"description":"A light's level jump between frames that counts as a change","examples":[0.25]},
        "changeColor": {"type":"number","minimum":0.01,"maximum":2,"description":"A lit light's colour jump that counts as a change (red to white is 1.41)","examples":[0.35]},
        "changeShare": {"type":"number","minimum":0.01,"maximum":1,"description":"The share of lights that must change in one frame for the whole to change","examples":[0.3]},
        "maxChangesPerSec": {"type":"number","minimum":0.5,"maximum":60,"description":"More whole changes per second than this is spazzing","examples":[6, 4]},
        "windowMs": {"type":"number","minimum":200,"maximum":10000,"description":"The window the change rate is counted over","examples":[1000]},
        "marks": {"type":"array","items":string,"maxItems":64,"description":"Mark collections (names or ids) a change should land on; [] skips the check","examples":[["Music - beats"], []]},
        "toleranceMs": {"type":"number","minimum":0,"maximum":1000,"description":"How far from a mark a change still counts as on it","examples":[50]},
        "offMarkMax": share("The share of off-mark change onsets above which marks is flagged", 0.25),
        "sections": {"type":"string","minLength":1,"maxLength":256,"description":"A mark collection whose marks start sections (mark text names them)","examples":["Sections"]},
        "sectionMs": {"type":"number","minimum":1000,"maximum":600000,"description":"Section length when no sections collection is given","examples":[10000]},
        "palette": {"type":"array","items":{"type":"string","pattern":"^#[0-9A-Fa-f]{6}$"},"maxItems":32,"description":"The sequence's colours; lit colours far from all of them are off-palette. Omit it to use the palette sequence_palette set","examples":[["#ff0000","#ffffff","#00ff00"]]},
        "paletteDistance": {"type":"number","minimum":0.01,"maximum":2,"description":"How far (full-brightness RGB, 0 to 1 per channel) a colour may be from a palette colour","examples":[0.35]},
        "offPaletteMax": share("The off-palette share of a section's light above which it is flagged", 0.15),
        "bluePurpleMax": share("The blue and purple share of a section's light at which it is flagged", 0.5),
    })
}

const BINS: [&str; 9] = ["red", "orange", "yellow", "green", "cyan", "blue", "purple", "magenta", "white"];
const BLUE: usize = 5;
const PURPLE: usize = 6;
const MAX_LISTED: usize = 50;
const MAX_JUMPS: usize = 100;

fn level(c: u32) -> f32 {
    f32::from(c.to_le_bytes()[3]) / 255.0
}

/// A colour scaled so its brightest channel is 1: what it looks like regardless of level.
fn hue_of(rgb: [f32; 3]) -> [f32; 3] {
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    if max <= 0.0 {
        [0.0; 3]
    } else {
        rgb.map(|v| v / max)
    }
}

fn rgb(c: u32) -> [f32; 3] {
    let [r, g, b, _] = c.to_le_bytes();
    hue_of([r, g, b].map(|v| f32::from(v) / 255.0))
}

pub fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Whether a light visibly changed between two frames: a big level jump, or a new colour while lit both times.
pub fn changed(a: u32, b: u32, change: Change, lit: f32) -> bool {
    let (la, lb) = (level(a), level(b));
    (la - lb).abs() >= change.level || (la >= lit && lb >= lit && distance(rgb(a), rgb(b)) >= change.color)
}

/// Whether a light came on (or brighter) or changed colour between two frames: a change that starts something, as
/// opposed to one going dark.
pub fn attack(a: u32, b: u32, change: Change, lit: f32) -> bool {
    let (la, lb) = (level(a), level(b));
    lb - la >= change.level || (la >= lit && lb >= lit && distance(rgb(a), rgb(b)) >= change.color)
}

/// The named hue a full-brightness colour falls in; washed-out colours are white.
pub fn bin(c: [f32; 3]) -> usize {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    if max <= 0.0 || (max - min) / max < 0.25 {
        return 8;
    }
    let d = max - min;
    let hue = if max == c[0] {
        60.0 * ((c[1] - c[2]) / d).rem_euclid(6.0)
    } else if max == c[1] {
        60.0 * ((c[2] - c[0]) / d + 2.0)
    } else {
        60.0 * ((c[0] - c[1]) / d + 4.0)
    };
    match hue {
        h if !(15.0..345.0).contains(&h) => 0,
        h if h < 40.0 => 1,
        h if h < 70.0 => 2,
        h if h < 160.0 => 3,
        h if h < 195.0 => 4,
        h if h < 255.0 => 5,
        h if h < 290.0 => 6,
        _ => 7,
    }
}

/// `#RRGGBB` to a full-brightness colour.
pub fn parse_hex(text: &str) -> Option<[f32; 3]> {
    let hex = text.strip_prefix('#').filter(|h| h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()))?;
    let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|v| f32::from(v) / 255.0);
    let color = [channel(0)?, channel(2)?, channel(4)?];
    (color.iter().any(|&v| v > 0.0)).then(|| hue_of(color))
}

fn hex(c: [f64; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", (c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8)
}

fn round(v: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (v * scale).round() / scale
}

/// A run of changes faster than allowed.
#[derive(Debug, PartialEq)]
pub struct Burst {
    pub start_ms: f64,
    pub end_ms: f64,
    pub changes: usize,
    pub peak_per_sec: f64,
}

/// The stretches where more than `max_per_sec` changes land in some `window_ms`, merged where they overlap.
pub fn bursts(changes: &[f64], max_per_sec: f64, window_ms: f64) -> Vec<Burst> {
    let need = (max_per_sec * window_ms / 1000.0).floor() as usize + 1;
    let mut found: Vec<Burst> = Vec::new();
    let mut k = 0;
    for (j, &start) in changes.iter().enumerate() {
        k = k.max(j);
        while k + 1 < changes.len() && changes[k + 1] < start + window_ms {
            k += 1;
        }
        let count = k - j + 1;
        if count < need {
            continue;
        }
        let rate = count as f64 * 1000.0 / window_ms;
        match found.last_mut() {
            Some(last) if start <= last.end_ms => {
                last.end_ms = last.end_ms.max(changes[k]);
                last.peak_per_sec = last.peak_per_sec.max(rate);
            }
            _ => found.push(Burst { start_ms: start, end_ms: changes[k], changes: 0, peak_per_sec: rate }),
        }
    }
    for burst in &mut found {
        burst.changes = changes.iter().filter(|&&t| (burst.start_ms..=burst.end_ms).contains(&t)).count();
    }
    found
}

/// Whether any of the sorted `marks` falls in `from..=to`.
pub fn mark_in(marks: &[f64], from: f64, to: f64) -> bool {
    let i = marks.partition_point(|&m| m < from);
    marks.get(i).is_some_and(|&m| m <= to)
}

/// A finding an agent can seek to.
struct Jump {
    time_ms: f64,
    kind: &'static str,
    detail: String,
    element: Option<String>,
}

/// One group's change and mark findings, and the jump points they produce.
fn group_findings(
    name: Option<&str>,
    times: &[f64],
    end_ms: f64,
    lit: &[f32],
    changed: &[bool],
    attacks: &[bool],
    input: &Input,
    jumps: &mut Vec<Jump>,
) -> (Value, usize, bool) {
    let (dark, dark_ms) = activity::dark_json(times, end_ms, lit, input.dark);
    for stretch in dark.as_array().unwrap() {
        jumps.push(Jump {
            time_ms: stretch["startMs"].as_f64().unwrap(),
            kind: "dark",
            detail: format!("dark for {:.1} s", stretch["durationMs"].as_f64().unwrap() / 1000.0),
            element: name.map(str::to_string),
        });
    }
    let change_times: Vec<f64> = times.iter().zip(changed).filter(|(_, &c)| c).map(|(&t, _)| t).collect();
    let span_s = ((end_ms - times.first().copied().unwrap_or(0.0)) / 1000.0).max(1e-3);
    let spazz = bursts(&change_times, input.max_per_sec, input.window_ms);
    let mut peak = 0.0f64;
    let mut k = 0;
    for (j, &start) in change_times.iter().enumerate() {
        k = k.max(j);
        while k + 1 < change_times.len() && change_times[k + 1] < start + input.window_ms {
            k += 1;
        }
        peak = peak.max((k - j + 1) as f64 * 1000.0 / input.window_ms);
    }
    for burst in &spazz {
        jumps.push(Jump {
            time_ms: burst.start_ms,
            kind: "spazz",
            detail: format!(
                "{} changes in {:.1} s, up to {:.0}/s",
                burst.changes,
                (burst.end_ms - burst.start_ms) / 1000.0,
                burst.peak_per_sec
            ),
            element: name.map(str::to_string),
        });
    }
    let mut out = json!({
        "dark": dark,
        "darkMs": dark_ms,
        "changes": {
            "total": change_times.len(),
            "perSec": {"mean": round(change_times.len() as f64 / span_s, 2), "peak": round(peak, 2)},
            "spazz": spazz.iter().map(|b| json!({
                "startMs": b.start_ms.round(),
                "endMs": b.end_ms.round(),
                "durationMs": (b.end_ms - b.start_ms).round(),
                "changes": b.changes,
                "peakPerSec": round(b.peak_per_sec, 2),
            })).collect::<Vec<_>>(),
        },
    });
    let mut off_flag = false;
    if let Some((marks, names)) = &input.marks {
        // A change first seen at frame f happened after frame f-1: it is on a mark if one falls in between, give or take
        // the tolerance. A change spanning several frames is one onset; lights going dark aren't onsets.
        let mut onsets = 0;
        let mut off = Vec::new();
        for f in 1..attacks.len() {
            if !attacks[f] || attacks[f - 1] {
                continue;
            }
            onsets += 1;
            if !mark_in(marks, times[f - 1] - input.tolerance_ms, times[f] + input.tolerance_ms) {
                off.push(times[f]);
            }
        }
        let share = if onsets == 0 { 0.0 } else { off.len() as f64 / onsets as f64 };
        off_flag = share > input.off_mark_max;
        if off_flag {
            for &t in off.iter().take(20) {
                jumps.push(Jump { time_ms: t, kind: "offMark", detail: "change on no mark".into(), element: name.map(str::to_string) });
            }
        }
        out["marks"] = json!({
            "collections": names,
            "onsets": onsets,
            "onMark": onsets - off.len(),
            "offMark": off.len(),
            "offMarkShare": round(share, 3),
            "flagged": off_flag,
            "offMarkMs": off.iter().take(MAX_LISTED).map(|t| t.round()).collect::<Vec<_>>(),
        });
    }
    let flagged = dark.as_array().unwrap().len() + spazz.len();
    (out, flagged, off_flag)
}

/// Colour weights for one section of the main group: every lit light in every frame, weighted by its level.
#[derive(Default)]
struct Tally {
    weight: f64,
    bins: [f64; 9],
    sums: [[f64; 3]; 9],
    off: f64,
    off_bins: [f64; 9],
    lit: f64,
    frames: usize,
}

fn top(bins: &[f64; 9], total: f64, sums: Option<&[[f64; 3]; 9]>) -> Vec<Value> {
    let mut order: Vec<usize> = (0..9).filter(|&b| bins[b] > 0.0 && bins[b] / total >= 0.05).collect();
    order.sort_by(|&a, &b| bins[b].total_cmp(&bins[a]));
    order
        .into_iter()
        .take(3)
        .map(|b| {
            let mut entry = json!({"name": BINS[b], "share": round(bins[b] / total, 3)});
            if let Some(sums) = sums {
                entry["hex"] = json!(hex(sums[b].map(|v| v / bins[b])));
            }
            entry
        })
        .collect()
}

/// Samples the schedule at `times` and checks groups[0] (the preview or the scope) and every other group.
pub fn analyze(renderer: &mut Renderer, times: &[f64], end_ms: f64, groups: &[activity::Group], input: &Input) -> Value {
    let palette: Vec<[f32; 3]> = input.palette.iter().filter_map(|h| parse_hex(h)).collect();
    let mut lit: Vec<Vec<f32>> = vec![Vec::with_capacity(times.len()); groups.len()];
    let mut changes: Vec<Vec<bool>> = vec![Vec::with_capacity(times.len()); groups.len()];
    let mut attacks: Vec<Vec<bool>> = vec![Vec::with_capacity(times.len()); groups.len()];
    let mut tallies: Vec<Tally> = input.sections.iter().map(|_| Tally::default()).collect();
    let (mut colors, mut previous) = (Vec::new(), Vec::new());
    for (f, &time) in times.iter().enumerate() {
        renderer.colors(time, &mut colors);
        for (g, group) in groups.iter().enumerate() {
            let lights = group.indices();
            let on = lights.iter().filter(|&&i| level(colors[i]) >= input.dark.level).count();
            let moved = if f == 0 { 0 } else { lights.iter().filter(|&&i| changed(previous[i], colors[i], input.change, input.dark.level)).count() };
            let started = if f == 0 { 0 } else { lights.iter().filter(|&&i| attack(previous[i], colors[i], input.change, input.dark.level)).count() };
            let total = lights.len().max(1) as f32;
            lit[g].push(on as f32 / total);
            changes[g].push(!lights.is_empty() && f > 0 && moved as f32 / total >= input.change.share);
            attacks[g].push(!lights.is_empty() && f > 0 && started as f32 / total >= input.change.share);
        }
        let section = input.sections.iter().position(|s| time >= s.start_ms && time < s.end_ms);
        if let Some(tally) = section.map(|s| &mut tallies[s]) {
            tally.frames += 1;
            tally.lit += f64::from(lit[0][f]);
            for &i in groups[0].indices() {
                let c = colors[i];
                let weight = f64::from(level(c));
                if level(c) < input.dark.level {
                    continue;
                }
                let color = rgb(c);
                let b = bin(color);
                tally.weight += weight;
                tally.bins[b] += weight;
                for (sum, v) in tally.sums[b].iter_mut().zip(color) {
                    *sum += f64::from(v) * weight;
                }
                if !palette.is_empty() && palette.iter().all(|p| distance(*p, color) > input.palette_distance) {
                    tally.off += weight;
                    tally.off_bins[b] += weight;
                }
            }
        }
        std::mem::swap(&mut previous, &mut colors);
    }

    let mut jumps = Vec::new();
    let (mut main, main_flags, main_off) = group_findings(None, times, end_ms, &lit[0], &changes[0], &attacks[0], input, &mut jumps);
    let dark_count = main["dark"].as_array().unwrap().len();
    let spazz_count = main["changes"]["spazz"].as_array().unwrap().len();
    let (mut off_palette, mut blue_purple) = (0, 0);
    let sections: Vec<Value> = input
        .sections
        .iter()
        .zip(&tallies)
        .map(|(section, tally)| {
            let mut flags = Vec::new();
            let mut out = json!({
                "startMs": section.start_ms.round(),
                "endMs": section.end_ms.round(),
                "time": super::timestamp(section.start_ms),
                "litMean": round(if tally.frames == 0 { 0.0 } else { tally.lit / tally.frames as f64 }, 3),
                "dominant": [],
            });
            if !section.name.is_empty() {
                out["name"] = json!(section.name);
            }
            if tally.weight > 0.0 {
                out["dominant"] = json!(top(&tally.bins, tally.weight, Some(&tally.sums)));
                let bp = (tally.bins[BLUE] + tally.bins[PURPLE]) / tally.weight;
                out["bluePurpleShare"] = json!(round(bp, 3));
                if bp >= input.blue_purple_max {
                    flags.push("bluePurple");
                    blue_purple += 1;
                    jumps.push(Jump { time_ms: section.start_ms, kind: "bluePurple", detail: format!("{:.0}% blue/purple", bp * 100.0), element: None });
                }
                if !palette.is_empty() {
                    let share = tally.off / tally.weight;
                    out["offPaletteShare"] = json!(round(share, 3));
                    if share > input.off_palette_max {
                        flags.push("offPalette");
                        off_palette += 1;
                        out["offPaletteColors"] = json!(top(&tally.off_bins, tally.off, None));
                        jumps.push(Jump { time_ms: section.start_ms, kind: "offPalette", detail: format!("{:.0}% outside the palette", share * 100.0), element: None });
                    }
                }
            }
            out["flags"] = json!(flags);
            out
        })
        .collect();
    let mut flagged = main_flags + off_palette + blue_purple + usize::from(main_off);
    let elements: Vec<Value> = groups[1..]
        .iter()
        .enumerate()
        .map(|(i, group)| {
            let (mut found, n, off) = group_findings(Some(&group.name), times, end_ms, &lit[i + 1], &changes[i + 1], &attacks[i + 1], input, &mut jumps);
            flagged += n + usize::from(off);
            found["id"] = json!(group.id.map(|id| id.to_string()));
            found["name"] = json!(group.name);
            found["lights"] = json!(group.lights());
            found
        })
        .collect();
    jumps.sort_by(|a, b| a.time_ms.total_cmp(&b.time_ms));
    let jump_count = jumps.len();
    main["lights"] = json!(groups[0].lights());
    if let Some(id) = groups[0].id {
        main["scope"] = json!({"id": id.to_string(), "name": groups[0].name});
    }
    main["ok"] = json!(flagged == 0);
    main["flags"] = json!({
        "dark": dark_count,
        "spazz": spazz_count,
        "offMark": main_off,
        "offPalette": off_palette,
        "bluePurple": blue_purple,
        "elements": elements.iter().filter(|e| !e["dark"].as_array().unwrap().is_empty() || !e["changes"]["spazz"].as_array().unwrap().is_empty() || e["marks"]["flagged"] == true).count(),
    });
    main["jumpTo"] = json!(jumps
        .iter()
        .take(MAX_JUMPS)
        .map(|j| {
            let mut entry = json!({"timeMs": j.time_ms.round(), "time": super::timestamp(j.time_ms), "kind": j.kind, "detail": j.detail});
            if let Some(element) = &j.element {
                entry["element"] = json!(element);
            }
            entry
        })
        .collect::<Vec<_>>());
    if jump_count > MAX_JUMPS {
        main["jumpToTruncated"] = json!(jump_count);
    }
    main["sections"] = json!(sections);
    if !elements.is_empty() {
        main["elements"] = json!(elements);
    }
    if input.marks.is_none() {
        main["marks"] = json!(null);
    }
    main["thresholds"] = json!({
        "dark": input.dark.to_json(),
        "change": {"level": input.change.level, "color": input.change.color, "share": input.change.share},
        "maxChangesPerSec": input.max_per_sec,
        "windowMs": input.window_ms,
        "toleranceMs": input.tolerance_ms,
        "offMarkMax": input.off_mark_max,
        "paletteDistance": input.palette_distance,
        "offPaletteMax": input.off_palette_max,
        "bluePurpleMax": input.blue_purple_max,
    });
    if !input.palette.is_empty() {
        main["palette"] = json!(input.palette);
    }
    main
}

#[cfg(test)]
mod tests {
    use super::*;

    fn color(r: u8, g: u8, b: u8, level: u8) -> u32 {
        u32::from_le_bytes([r, g, b, level])
    }

    #[test]
    fn a_light_changes_on_a_level_jump_or_a_new_colour_while_lit() {
        let lit = 0.1;
        assert!(changed(color(255, 0, 0, 255), 0, Change::DEFAULT, lit), "off");
        assert!(!changed(color(255, 0, 0, 255), color(255, 0, 0, 220), Change::DEFAULT, lit), "a slow fade step");
        assert!(changed(color(255, 0, 0, 255), color(0, 0, 255, 255), Change::DEFAULT, lit), "red to blue");
        assert!(!changed(color(255, 255, 255, 255), color(255, 240, 220, 255), Change::DEFAULT, lit), "white to warm white");
        assert!(!changed(color(255, 0, 0, 10), color(0, 0, 255, 10), Change::DEFAULT, lit), "colours of an unlit light");
        assert!(!attack(color(255, 0, 0, 255), 0, Change::DEFAULT, lit), "going dark starts nothing");
        assert!(attack(0, color(255, 0, 0, 255), Change::DEFAULT, lit), "coming on");
        assert!(attack(color(255, 0, 0, 255), color(0, 255, 0, 255), Change::DEFAULT, lit), "a new colour");
    }

    #[test]
    fn hues_fall_in_named_bins_and_washed_out_colours_are_white() {
        let name = |c: [f32; 3]| BINS[bin(c)];
        assert_eq!(name([1.0, 0.0, 0.0]), "red");
        assert_eq!(name([1.0, 0.5, 0.0]), "orange");
        assert_eq!(name([0.0, 1.0, 0.0]), "green");
        assert_eq!(name([0.0, 0.0, 1.0]), "blue");
        assert_eq!(name([0.6, 0.0, 1.0]), "purple");
        assert_eq!(name([1.0, 0.0, 0.6]), "magenta");
        assert_eq!(name([1.0, 0.9, 0.8]), "white");
        assert_eq!(parse_hex("#800000"), Some([1.0, 0.0, 0.0]));
        assert_eq!(parse_hex("#000000"), None);
        assert_eq!(parse_hex("red"), None);
    }

    #[test]
    fn bursts_are_more_changes_per_second_than_allowed_merged_where_they_overlap() {
        // Two changes a second, then ten a second from 10 s to 12 s, then two a second again.
        let mut times: Vec<f64> = (0..20).map(|i| i as f64 * 500.0).collect();
        times.extend((0..20).map(|i| 10_000.0 + i as f64 * 100.0));
        times.extend((0..10).map(|i| 12_500.0 + i as f64 * 500.0));
        let found = bursts(&times, 6.0, 1000.0);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].start_ms, 10_000.0);
        assert!(found[0].end_ms >= 11_900.0 && found[0].end_ms <= 12_500.0, "{found:?}");
        assert_eq!(found[0].peak_per_sec, 10.0);
        assert!(bursts(&times[..20], 6.0, 1000.0).is_empty(), "a steady beat is not spazzing");
        // Exactly the limit is allowed; one more is not.
        let six: Vec<f64> = (0..6).map(|i| i as f64 * 150.0).collect();
        assert!(bursts(&six, 6.0, 1000.0).is_empty());
        let seven: Vec<f64> = (0..7).map(|i| i as f64 * 140.0).collect();
        assert_eq!(bursts(&seven, 6.0, 1000.0).len(), 1);
    }

    #[test]
    fn marks_are_found_between_two_frames_with_tolerance() {
        let marks = [500.0, 1000.0];
        assert!(mark_in(&marks, 450.0, 550.0));
        assert!(!mark_in(&marks, 550.0, 950.0));
        assert!(mark_in(&marks, 950.0, 2000.0));
        assert!(!mark_in(&marks, 1001.0, 2000.0));
    }
}
