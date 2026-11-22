//! Automatic updates (Sparkle 2, B49): what the app's updater last reported, the MCP tools that read it and ask for a
//! check, and whether a show is running, which the updater consults so it never interrupts one.
//!
//! The updater itself lives in the app (Sparkle is a Swift framework). The app publishes its state here after every
//! change and registers one hook that starts a check; agents read the state and ask for checks, but installing and the
//! automatic-install switch stay the user's, so no tool touches them.
use crate::app::AppCore;
use crate::execution::ExecutionState;
use serde_json::{json, Value};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use vixen_engine::Transport;

/// Starts a check for update information in the app. It returns at once; the result arrives through [`publish`].
pub type CheckHook = Box<dyn Fn() -> Result<(), String> + Send + Sync>;

/// How long `app_update_check` waits for the app's answer. Sparkle's own feed request times out sooner.
const CHECK_WAIT: Duration = Duration::from_secs(45);

#[derive(Default)]
struct State {
    /// The app's last report, as it gave it; `None` until the app publishes (a headless bridge never does).
    status: Option<Value>,
    /// Checks the app has finished, so a waiting tool call knows its own check is done.
    finished: u64,
}

struct Store {
    state: Mutex<State>,
    changed: Condvar,
    hook: Mutex<Option<CheckHook>>,
}

fn store() -> &'static Store {
    static STORE: OnceLock<Store> = OnceLock::new();
    STORE.get_or_init(|| Store { state: Mutex::new(State::default()), changed: Condvar::new(), hook: Mutex::new(None) })
}

/// The app's updater state as a JSON object. `finished_check` marks the report that ends a check.
pub fn publish(status: Value, finished_check: bool) -> Result<(), String> {
    if !status.is_object() {
        return Err("status must be an object".into());
    }
    let s = store();
    let mut state = s.state.lock().unwrap_or_else(|p| p.into_inner());
    state.status = Some(status);
    if finished_check {
        state.finished += 1;
    }
    s.changed.notify_all();
    Ok(())
}

/// The app's way to start a check; replaces any earlier one.
pub fn register_check(hook: CheckHook) {
    *store().hook.lock().unwrap_or_else(|p| p.into_inner()) = Some(hook);
}

/// Whether a show is running, for the updater: a sequence or a scheduled show playing (or paused mid-show), or Live
/// Output on. While either is true the updater keeps its prompt and relaunch for later.
pub fn show_activity(core: &AppCore) -> Value {
    let playing = core.playback.context_states().iter().any(|c| c.transport != Transport::Stopped);
    let live_output = core.playback.execution_state() != ExecutionState::Closed;
    json!({"playing": playing, "liveOutput": live_output, "busy": playing || live_output})
}

fn current() -> Result<Value, String> {
    let state = store().state.lock().unwrap_or_else(|p| p.into_inner());
    state.status.clone().ok_or_else(|| {
        "No updater state: automatic updates live in the running app, so attach to the app rather than a headless bridge.".to_string()
    })
}

fn no_arguments(args: &Value) -> Result<(), String> {
    match args.as_object() {
        Some(obj) if obj.is_empty() => Ok(()),
        Some(_) => Err("Unknown argument".into()),
        None => Err("arguments must be an object".into()),
    }
}

pub fn tools() -> Vec<Value> {
    let status = json!({"type":"object","properties":{},"additionalProperties":false});
    vec![
        json!({"name":"app_update_status","description":"Vixen Studio's automatic updates, as Settings ▸ General ▸ Updates shows them: enabled (false in builds without an update feed), currentVersion and currentBuild, automaticallyChecks and checkIntervalHours, automaticallyDownloads (install updates without asking), lastCheck, checking, available (the newer version the last check found: version, build, title, notesUrl; null when up to date), deferred (a scheduled update kept as a reminder because a sequence plays or Live Output is on), showRunning, waitingForShowToStop (an accepted install waiting to relaunch), readyToInstallOnQuit and lastError. Needs the running app. Read-only.","inputSchema":status,"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":true}}),
        json!({"name":"app_update_check","description":"Ask Vixen Studio's updater to check its feed now and wait for the answer (up to 45 s); returns app_update_status afterwards. It only checks: nothing is downloaded or installed, and no window opens. Installing, and the automatic-install switch, stay the user's. An update already showing or installing is reported in lastError; refused in builds without an update feed. Needs the running app.","inputSchema":status,"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":true}}),
    ]
}

pub fn is_tool(name: &str) -> bool {
    matches!(name, "app_update_status" | "app_update_check")
}

pub fn execute(name: &str, args: &Value) -> Result<Value, String> {
    no_arguments(args)?;
    match name {
        "app_update_status" => current(),
        "app_update_check" => check(CHECK_WAIT),
        _ => Err("Unknown tool".into()),
    }
}

fn check(wait: Duration) -> Result<Value, String> {
    let status = current()?;
    if status["enabled"] != json!(true) {
        return Err("Automatic updates are off in this build (it has no update feed), so there is nothing to check.".into());
    }
    let s = store();
    let before = s.state.lock().unwrap_or_else(|p| p.into_inner()).finished;
    {
        let hook = s.hook.lock().unwrap_or_else(|p| p.into_inner());
        let hook = hook.as_ref().ok_or("The app hasn't started its updater yet.")?;
        hook()?;
    }
    let deadline = Instant::now() + wait;
    let mut state = s.state.lock().unwrap_or_else(|p| p.into_inner());
    while state.finished == before {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("The update check didn't finish in time; app_update_status shows it when it does.".into());
        }
        state = s.changed.wait_timeout(state, left).unwrap_or_else(|p| p.into_inner()).0;
    }
    Ok(state.status.clone().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    // One process-wide store: the tests take turns.
    static SERIAL: StdMutex<()> = StdMutex::new(());

    fn reset() {
        let s = store();
        *s.state.lock().unwrap() = State::default();
        *s.hook.lock().unwrap() = None;
    }

    #[test]
    fn status_needs_the_app() {
        let _turn = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        let err = execute("app_update_status", &json!({})).unwrap_err();
        assert!(err.contains("running app"), "{err}");
        publish(json!({"enabled":true,"currentBuild":"12"}), false).unwrap();
        assert_eq!(execute("app_update_status", &json!({})).unwrap()["currentBuild"], "12");
        assert_eq!(execute("app_update_status", &json!({"x":1})).unwrap_err(), "Unknown argument");
        assert!(publish(json!([1]), false).is_err());
    }

    #[test]
    fn check_waits_for_the_apps_answer() {
        let _turn = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        publish(json!({"enabled":true,"checking":false}), false).unwrap();
        register_check(Box::new(|| {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(50));
                publish(json!({"enabled":true,"checking":false,"available":{"version":"3.13.4 alpha 6","build":"4000"}}), true).unwrap();
            });
            Ok(())
        }));
        let result = check(Duration::from_secs(5)).unwrap();
        assert_eq!(result["available"]["build"], "4000");
    }

    #[test]
    fn check_refuses_without_a_feed_or_when_the_app_says_no() {
        let _turn = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        publish(json!({"enabled":false}), false).unwrap();
        assert!(check(Duration::from_millis(10)).unwrap_err().contains("no update feed"));
        publish(json!({"enabled":true}), false).unwrap();
        assert!(check(Duration::from_millis(10)).unwrap_err().contains("hasn't started"));
        register_check(Box::new(|| Err("An update is already in progress.".into())));
        assert_eq!(check(Duration::from_millis(10)).unwrap_err(), "An update is already in progress.");
        register_check(Box::new(|| Ok(())));
        assert!(check(Duration::from_millis(20)).unwrap_err().contains("didn't finish in time"));
    }
}
