//! Commissioning tests for a first power-up: a moving pixel, R/G/B/W color bars or every light on, at a chosen brightness,
//! on one element, group or controller, without a sequence. They play through a live context, so they need Live Output on
//! (execution open and not muted) and stop at once on demand. The Display pane's Test submenus (the user, `Origin::Local`)
//! and the MCP `output_test_pattern` / `output_test_stop` tools (`Origin::Agent`, also gated by the local permission
//! switch in `studio_live`) run this same code. One test runs at a time; starting another replaces it.
use crate::app::AppCore;
use crate::setup::tree::Nid;
use crate::setup::SetupSession;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;
use vixen_core::{Color, EffectIntents, FrameArray, Intent, IntentData, LightingValue, TimeSpan};

/// The live context the user's tests play in.
const LOCAL_CONTEXT: &str = "Studio Commissioning";
/// Agents' tests share `studio_live`'s context, so revoking the permission switch stops them with everything else.
pub(crate) const AGENT_CONTEXT: &str = "studio-agent-test";
pub const DEFAULT_BRIGHTNESS: u32 = 25;
/// The longest a test runs before stopping by itself, and the default for the user's tests, which run until stopped.
pub const MAX_DURATION_MS: u64 = 600_000;
/// An agent that forgets to stop leaves lights on for a minute, not ten.
const AGENT_DEFAULT_DURATION_MS: u64 = 60_000;
const MOVING_PIXEL_STEP_MS: u64 = 100;
const COLOR_BARS_STEP_MS: u64 = 1_500;
pub const STEP_RANGE_MS: (u64, u64) = (20, 10_000);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Local,
    Agent,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Pattern {
    /// One light at a time, in wire order, so a skipped, swapped or reversed light shows.
    MovingPixel,
    /// Every light red, green, blue, then white, so a wrong color order or a dead channel shows.
    ColorBars,
    /// Every light white, for a power and boundary check.
    AllOn,
}

impl Pattern {
    pub const NAMES: [&'static str; 3] = ["movingPixel", "colorBars", "allOn"];
    fn parse(name: &str) -> Option<Pattern> {
        match name {
            "movingPixel" => Some(Pattern::MovingPixel),
            "colorBars" => Some(Pattern::ColorBars),
            "allOn" => Some(Pattern::AllOn),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }
    fn title(self) -> &'static str {
        ["Moving Pixel", "Color Bars", "All On"][self as usize]
    }
    fn default_step_ms(self) -> u64 {
        match self {
            Pattern::MovingPixel => MOVING_PIXEL_STEP_MS,
            Pattern::ColorBars => COLOR_BARS_STEP_MS,
            Pattern::AllOn => 0,
        }
    }
}

/// The test running now, for the pane's notice and `status`.
pub(crate) struct Running {
    origin: Origin,
    summary: Value,
    started: Instant,
    duration: Duration,
}

/// The lights a test reaches, as leaf element IDs in the order it walks them, with the target's name and kind.
#[derive(Debug)]
pub struct Lights {
    pub name: String,
    pub kind: &'static str,
    pub elements: Vec<Uuid>,
}

/// An element or group's lights in wire order: those patched by their first output, then any unpatched ones in tree order
/// (they still show in previews). A controller's lights: what's patched to it, in output order.
pub fn lights(s: &SetupSession, target: Option<Uuid>, controller: Option<Uuid>) -> Result<Lights, String> {
    let element_of = |n: Nid| s.tree.node(n).element_id;
    match (target, controller) {
        (Some(id), None) => {
            let node = s.tree.find(&id).ok_or("Element does not exist")?;
            let leaves = s.tree.distinct_leaves([node]);
            let mine: std::collections::HashSet<Nid> = leaves.iter().copied().collect();
            let mut order: Vec<Nid> =
                crate::studio_patch::lights_in_output_order(s, None).into_iter().filter(|n| mine.contains(n)).collect();
            let patched: std::collections::HashSet<Nid> = order.iter().copied().collect();
            order.extend(leaves.into_iter().filter(|n| !patched.contains(n)));
            let name = s.tree.node(node).name.clone();
            let elements: Vec<Uuid> = order.into_iter().filter_map(element_of).collect();
            if elements.is_empty() {
                return Err(format!("{name} has no lights to test"));
            }
            Ok(Lights { name, kind: "element", elements })
        }
        (None, Some(id)) => {
            let slot = s.slot_of(id).filter(|&slot| s.controllers[slot as usize].alive).ok_or("Controller not found")?;
            let name = s.controllers[slot as usize].name.clone();
            let elements: Vec<Uuid> =
                crate::studio_patch::lights_in_output_order(s, Some(slot)).into_iter().filter_map(element_of).collect();
            if elements.is_empty() {
                return Err(format!("Nothing is patched to {name}"));
            }
            Ok(Lights { name, kind: "controller", elements })
        }
        _ => Err("Give exactly one of targetId and controllerId".into()),
    }
}

/// The pattern as intents for `duration`, every light at `brightness` (0..1). A moving pixel lights each light for one
/// step in turn and starts again; color bars give every light one shared per-step color array, so a long test on
/// thousands of lights stays small.
pub fn intents(elements: &[Uuid], pattern: Pattern, brightness: f64, step: TimeSpan, duration: TimeSpan) -> EffectIntents {
    let mut intents = EffectIntents::with_capacity(elements.len());
    match pattern {
        Pattern::AllOn => {
            for &id in elements {
                intents.add_intent_for_element(id, Intent::static_lighting(Color::WHITE, brightness, duration), TimeSpan::ZERO);
            }
        }
        Pattern::ColorBars => {
            let colors = [Color::RED, Color::rgb(0, 255, 0), Color::BLUE, Color::WHITE];
            let steps = (duration.ticks() + step.ticks() - 1) / step.ticks();
            let values: Arc<[LightingValue]> =
                (0..steps).map(|i| LightingValue::new(colors[i as usize % colors.len()], brightness)).collect();
            for &id in elements {
                let array = FrameArray::new(values.clone(), 0, values.len(), step);
                intents.add_intent_for_element(id, Intent::new(IntentData::ArrayLighting(array), duration), TimeSpan::ZERO);
            }
        }
        Pattern::MovingPixel => {
            let cycle = step.ticks() * elements.len() as i64;
            for (i, &id) in elements.iter().enumerate() {
                let mut start = step.ticks() * i as i64;
                while start < duration.ticks() {
                    let span = TimeSpan(step.ticks().min(duration.ticks() - start));
                    intents.add_intent_for_element(id, Intent::static_lighting(Color::WHITE, brightness, span), TimeSpan(start));
                    start += cycle;
                }
            }
        }
    }
    intents
}

/// Why the user can't run a test now, or None. Tests send to the controllers, so they need Live Output on.
pub fn local_block(core: &AppCore) -> Option<&'static str> {
    if !core.playback.hardware_allowed() || std::env::var("VIXEN_MUTE").is_ok_and(|v| v == "1") {
        Some("Tests can't run while the app is muted")
    } else if !core.execution().is_open() {
        Some("Turn on Live Output to run a test")
    } else {
        None
    }
}

fn uuid_arg(a: &Value, key: &str) -> Result<Option<Uuid>, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_str().and_then(|s| Uuid::parse_str(s).ok()).map(Some).ok_or(format!("{key} must be a UUID")),
    }
}

fn number_arg(a: &Value, key: &str, default: u64, min: u64, max: u64) -> Result<u64, String> {
    match a.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_u64().filter(|n| (min..=max).contains(n)).ok_or(format!("{key} must be {min}..{max}")),
    }
}

/// Starts a test from {targetId | controllerId, pattern, brightnessPct?, stepMs?, durationMs?}. The caller has already
/// checked the gate (`local_block`, or `studio_live`'s for agents). Returns what's running.
pub fn start(core: &AppCore, origin: Origin, a: &Value) -> Result<Value, String> {
    let pattern = a["pattern"]
        .as_str()
        .and_then(Pattern::parse)
        .ok_or_else(|| format!("pattern must be one of {}", Pattern::NAMES.join(", ")))?;
    let brightness = number_arg(a, "brightnessPct", DEFAULT_BRIGHTNESS as u64, 1, 100)?;
    let step_ms = number_arg(a, "stepMs", pattern.default_step_ms(), STEP_RANGE_MS.0, STEP_RANGE_MS.1)?;
    let default_duration = if origin == Origin::Local { MAX_DURATION_MS } else { AGENT_DEFAULT_DURATION_MS };
    let duration_ms = number_arg(a, "durationMs", default_duration, 25, MAX_DURATION_MS)?;
    let target = uuid_arg(a, "targetId")?;
    let controller = uuid_arg(a, "controllerId")?;
    let setup = SetupSession::from_system(core.system.read().clone(), &Default::default(), false);
    let lights = lights(&setup, target, controller)?;
    if let Some(id) = controller {
        if !core.playback.running_controllers().is_some_and(|running| running.contains(&id)) {
            return Err(format!("{} isn't running; start it first", lights.name));
        }
    }
    let step = TimeSpan::from_ms(step_ms.max(1) as f64);
    let duration = TimeSpan::from_ms(duration_ms as f64);
    let intents = intents(&lights.elements, pattern, brightness as f64 / 100.0, step, duration);
    stop(core);
    let context = if origin == Origin::Local { LOCAL_CONTEXT } else { AGENT_CONTEXT };
    core.live.insert(core, context, target.or(controller).unwrap_or_default(), intents, duration);
    let mut summary = json!({"pattern":pattern.name(),"title":pattern.title(),"name":lights.name,"kind":lights.kind,
        "lights":lights.elements.len(),"brightnessPct":brightness,"durationMs":duration_ms,
        "agent":origin == Origin::Agent});
    if let Some(id) = target.or(controller) {
        summary[if target.is_some() { "targetId" } else { "controllerId" }] = json!(id.to_string());
    }
    if pattern != Pattern::AllOn {
        summary["stepMs"] = json!(step_ms);
    }
    *core.studio_display.commission.lock() =
        Some(Running { origin, summary: summary.clone(), started: Instant::now(), duration: Duration::from_millis(duration_ms) });
    Ok(summary)
}

/// Stops whatever test is running, whoever started it. Stopping is always allowed.
pub fn stop(core: &AppCore) -> bool {
    core.live.release(LOCAL_CONTEXT);
    core.live.release(AGENT_CONTEXT);
    core.studio_display.commission.lock().take().is_some()
}

/// Forgets an agent's test once its context has been released (the permission switch turned off).
pub(crate) fn agent_revoked(core: &AppCore) {
    let mut running = core.studio_display.commission.lock();
    if running.as_ref().is_some_and(|r| r.origin == Origin::Agent) {
        *running = None;
    }
}

/// The running test with `remainingMs`, or None once it has ended, been stopped or Live Output went off.
pub fn status(core: &AppCore) -> Option<Value> {
    let mut running = core.studio_display.commission.lock();
    let live = running.as_ref().is_some_and(|r| r.started.elapsed() < r.duration) && local_block(core).is_none();
    if !live {
        *running = None;
        return None;
    }
    let r = running.as_ref()?;
    let mut summary = r.summary.clone();
    summary["remainingMs"] = json!(r.duration.saturating_sub(r.started.elapsed()).as_millis() as u64);
    Some(summary)
}

/// What the Test submenu needs for one element row or controller: the target, how many lights it has, why tests can't
/// run now, and the running test.
pub fn menu(core: &AppCore, s: &SetupSession, target: Option<Uuid>, controller: Option<Uuid>) -> Value {
    let found = lights(s, target, controller);
    json!({
        "name": found.as_ref().map(|l| l.name.clone()).ok(),
        "lights": found.as_ref().map_or(0, |l| l.elements.len()),
        "problem": found.err(),
        "block": local_block(core),
        "running": status(core),
        "defaultBrightnessPct": DEFAULT_BRIGHTNESS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::test_support::*;
    use parking_lot::Mutex;
    use vixen_core::{CombinedState, ElementTree, StateValue};
    use vixen_engine::FrameSink;

    fn lit(intents: &EffectIntents, id: &Uuid, ms: f64) -> Option<(Color, f64)> {
        let t = TimeSpan::from_ms(ms);
        let node = intents.nodes_for(id)?.iter().find(|n| n.contains(t))?;
        match node.intent.state_at(TimeSpan(t.ticks() - node.start.ticks())) {
            StateValue::Lighting(v) => Some((v.color, v.intensity)),
            _ => None,
        }
    }

    #[test]
    fn moving_pixel_walks_each_light_in_turn_and_repeats() {
        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let step = TimeSpan::from_ms(100.0);
        let i = intents(&ids, Pattern::MovingPixel, 0.25, step, TimeSpan::from_ms(650.0));
        let on = |ms| ids.iter().position(|id| lit(&i, id, ms).is_some());
        assert_eq!([on(50.0), on(150.0), on(250.0), on(350.0), on(620.0)], [Some(0), Some(1), Some(2), Some(0), Some(0)]);
        assert_eq!(lit(&i, &ids[1], 150.0), Some((Color::WHITE, 0.25)));
        // Only one light at a time.
        assert_eq!(ids.iter().filter(|id| lit(&i, id, 150.0).is_some()).count(), 1);
        // The last step is cut at the end, not carried past it.
        assert!(i.nodes_for(&ids[0]).unwrap().iter().all(|n| n.start.ticks() + n.intent.span.ticks() <= TimeSpan::from_ms(650.0).ticks()));
    }

    #[test]
    fn color_bars_cycle_red_green_blue_white_on_every_light() {
        let ids: Vec<Uuid> = (0..2).map(|_| Uuid::new_v4()).collect();
        let i = intents(&ids, Pattern::ColorBars, 0.5, TimeSpan::from_ms(1000.0), TimeSpan::from_ms(5500.0));
        for id in &ids {
            let colors: Vec<Color> = [500.0, 1500.0, 2500.0, 3500.0, 4500.0].iter().map(|&ms| lit(&i, id, ms).unwrap().0).collect();
            assert_eq!(colors, [Color::RED, Color::rgb(0, 255, 0), Color::BLUE, Color::WHITE, Color::RED]);
            assert_eq!(lit(&i, id, 500.0).unwrap().1, 0.5);
        }
        // One shared color array, however many lights.
        let block = |id: &Uuid| match &i.nodes_for(id).unwrap()[0].intent.data {
            IntentData::ArrayLighting(a) => a.block.clone(),
            _ => panic!("not an array"),
        };
        assert!(Arc::ptr_eq(&block(&ids[0]), &block(&ids[1])));
    }

    #[test]
    fn all_on_holds_every_light_at_the_brightness() {
        let ids: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
        let i = intents(&ids, Pattern::AllOn, 0.1, TimeSpan::ZERO, TimeSpan::from_ms(2000.0));
        assert!(ids.iter().all(|id| lit(&i, id, 1999.0) == Some((Color::WHITE, 0.1))));
    }

    /// Lights A..F, A B C grouped as "Left" and D E F as "Right", patched A..F to the first outputs of one controller.
    fn rig() -> SetupSession {
        let mut s = session(&["A", "B", "C", "D", "E", "F"], 12);
        let rows = |s: &SetupSession, names: &[&str]| -> Vec<String> {
            names.iter().map(|n| s.row_path_of(s.tree.live_nodes().find(|(_, x)| x.name == *n).unwrap().0)).collect()
        };
        let left = rows(&s, &["A", "B", "C"]);
        s.create_group(&left, "Left");
        let right = rows(&s, &["D", "E", "F"]);
        s.create_group(&right, "Right");
        let both = rows(&s, &["Left", "Right"]);
        let outputs = s.all_outputs_selection();
        assert_eq!(s.patch_elements(&both, &outputs, &Default::default(), false), 6);
        s
    }
    fn node_id(s: &SetupSession, name: &str) -> Uuid {
        s.tree.node(s.tree.live_nodes().find(|(_, n)| n.name == name).unwrap().0).id
    }
    fn names(s: &SetupSession, l: &Lights) -> Vec<String> {
        l.elements.iter().map(|e| s.tree.node(s.tree.find_by_element(e).unwrap()).name.clone()).collect()
    }

    #[test]
    fn lights_follow_the_wiring_for_groups_and_controllers() {
        let mut s = rig();
        let right = node_id(&s, "Right");
        crate::studio_patch::reverse(&mut s, &json!({"id": right.to_string()})).unwrap();
        let group = lights(&s, Some(right), None).unwrap();
        assert_eq!((group.name.as_str(), group.kind), ("Right", "element"));
        assert_eq!(names(&s, &group), ["F", "E", "D"]);
        let controller = s.controllers[0].id;
        let all = lights(&s, None, Some(controller)).unwrap();
        assert_eq!(all.kind, "controller");
        assert_eq!(names(&s, &all), ["A", "B", "C", "F", "E", "D"]);
        assert!(lights(&s, None, None).is_err());
        assert!(lights(&s, Some(right), Some(controller)).is_err());
        assert!(lights(&s, Some(Uuid::new_v4()), None).unwrap_err().contains("does not exist"));
    }

    #[test]
    fn unpatched_lights_come_after_patched_ones_and_an_empty_controller_is_refused() {
        let mut s = session(&["A", "B", "C"], 6);
        let rows: Vec<String> = ["C"].iter().map(|n| s.row_path_of(s.tree.live_nodes().find(|(_, x)| x.name == *n).unwrap().0)).collect();
        let outputs = s.all_outputs_selection();
        s.patch_elements(&rows, &outputs, &Default::default(), false);
        let all: Vec<String> = ["A", "B", "C"].iter().map(|n| s.row_path_of(s.tree.live_nodes().find(|(_, x)| x.name == *n).unwrap().0)).collect();
        s.create_group(&all, "All");
        let l = lights(&s, Some(node_id(&s, "All")), None).unwrap();
        assert_eq!(names(&s, &l), ["C", "A", "B"]);
        let empty = session(&["A"], 3);
        let controller = empty.controllers[0].id;
        assert!(lights(&empty, None, Some(controller)).unwrap_err().starts_with("Nothing is patched"));
    }

    struct Recorder(Arc<Mutex<Vec<CombinedState>>>);
    impl FrameSink for Recorder {
        fn frame(&mut self, _: TimeSpan, states: &[CombinedState], _: bool) {
            *self.0.lock() = states.to_vec();
        }
    }
    fn wait(mut done: impl FnMut() -> bool) -> bool {
        (0..400).any(|_| done() || {
            std::thread::sleep(Duration::from_millis(5));
            false
        })
    }

    /// A muted core sends nothing to hardware, but its frames still reach sinks, so the test watches them.
    #[test]
    fn a_test_lights_its_target_reports_status_and_stops_on_demand() {
        let mut system = vixen_io::SystemData::default();
        let mut tree = ElementTree::new();
        let leaf = tree.add_element("Leaf");
        let node = tree.node(leaf).id;
        system.tree = tree;
        system.default_update_interval = 2;
        let core = AppCore::assemble(
            vixen_io::DataPaths::new(std::env::temp_dir().join(format!("vixen-commission-{}", Uuid::new_v4()))),
            system,
            Default::default(),
            None,
            crate::boot::LaunchOptions { muted: true, ..Default::default() },
            None,
        );
        let states = Arc::new(Mutex::new(Vec::new()));
        core.playback.add_sink(Box::new(Recorder(states.clone())));
        let color = || states.lock().first().and_then(|s| s.lighting.map(|v| v.full_color()));
        // Muted: the user's gate refuses before anything plays.
        assert_eq!(local_block(&core), Some("Tests can't run while the app is muted"));
        let started = start(&core, Origin::Local, &json!({"targetId": node.to_string(), "pattern": "allOn", "brightnessPct": 50})).unwrap();
        assert_eq!((started["lights"].as_u64(), started["brightnessPct"].as_u64()), (Some(1), Some(50)));
        assert_eq!(started["durationMs"].as_u64(), Some(MAX_DURATION_MS));
        assert!(wait(|| color().is_some_and(|c| c.r > 100 && c.r < 160 && c.r == c.g && c.g == c.b)), "{:?}", color());
        assert!(stop(&core));
        assert!(wait(|| color().is_none()));
        assert!(!stop(&core));
        let bad = start(&core, Origin::Agent, &json!({"targetId": node.to_string(), "pattern": "rainbow"}));
        assert!(bad.unwrap_err().contains("pattern must be one of"));
        let agent = start(&core, Origin::Agent, &json!({"targetId": node.to_string(), "pattern": "movingPixel"})).unwrap();
        assert_eq!((agent["durationMs"].as_u64(), agent["stepMs"].as_u64()), (Some(AGENT_DEFAULT_DURATION_MS), Some(100)));
        agent_revoked(&core);
        assert!(core.studio_display.commission.lock().is_none());
        stop(&core);
    }
}
