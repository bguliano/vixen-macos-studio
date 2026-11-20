//! The scheduler's executor: the check timer, starting and ending shows, the status line and the log (F54-F62, F100, F101).
//!
//! Nothing here blocks. Actions report their end through a queue that `poll` drains, so a completion never runs inside the call that
//! started the action, and a show whose items all end at once can't recurse as upstream's does.

use super::actions::{ActionRunner, Done, RunningAction};
use super::{millis_between, plus_seconds, ScheduleItem, SchedulerData};
use crate::logging;
use crate::shows::{Show, ShowItem, ShowItemType};
use parking_lot::{Condvar, Mutex};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;
use vixen_io::datetime::LocalDateTime;

pub const LOGGER: &str = "Scheduler";
pub const LOG_LIMIT: usize = 250;
/// The status line before the first check.
pub const INITIAL_STATUS: &str = "Waiting to run next show...";

pub trait Clock: Send + Sync {
    fn now(&self) -> LocalDateTime;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> LocalDateTime {
        LocalDateTime::now()
    }
}

/// `StateType`, less `Startup` and `Paused`, which nothing upstream ever sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Waiting,
    Running,
    Shutdown,
}

/// The status line (F56). `show_name` gives the name of a waiting item's show, if it has one.
pub fn status_text(
    manually_disabled: bool,
    items: &[(&ScheduleItem, State, Option<&str>)],
    show_name: &dyn Fn(Uuid) -> Option<String>,
    now: &LocalDateTime,
) -> String {
    if manually_disabled {
        return "All Show Disabled".to_string();
    }
    if let Some((_, state, name)) = items.iter().find(|(_, state, _)| *state != State::Waiting) {
        let label = if *state == State::Running { "Running" } else { "Shutdown" };
        return format!("{label}: {}", name.unwrap_or(""));
    }
    let next = items
        .iter()
        .filter(|(item, _, _)| item.enabled)
        .filter_map(|(item, _, _)| item.next_start(now).filter(|at| at > now).map(|at| (at, item.show_id)))
        .min_by_key(|(at, _)| *at);
    match next {
        Some((at, show)) => {
            let name = show_name(show).unwrap_or_else(|| "Show not selected".to_string());
            format!("Up Next: {name} on {}", at.settings_text())
        }
        None => "Nothing scheduled.... ".to_string(),
    }
}

/// What the Status window shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub text: String,
    /// The executor is on, which is when the window is on screen.
    pub enabled: bool,
    pub can_start: bool,
    pub can_stop: bool,
}

/// What the executor needs from outside.
pub struct Deps {
    pub clock: Arc<dyn Clock>,
    pub runner: Arc<dyn ActionRunner>,
    /// A show by id, as it is now. Called with the scheduler's lock held: it mustn't call back in.
    pub shows: Arc<dyn Fn(Uuid) -> Option<Show> + Send + Sync>,
    /// Runs a long job off the caller's thread.
    pub spawn: Arc<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>,
}

impl Deps {
    pub fn threaded(runner: Arc<dyn ActionRunner>, shows: Arc<dyn Fn(Uuid) -> Option<Show> + Send + Sync>) -> Deps {
        Deps {
            clock: Arc::new(SystemClock),
            runner,
            shows,
            spawn: Arc::new(|job| {
                let _ = std::thread::Builder::new().name("show-prepare".into()).spawn(job);
            }),
        }
    }
}

enum Event {
    Completed { run: u64, token: u64 },
    PreProcessed { run: u64 },
}

#[derive(Default)]
struct Events {
    queue: Mutex<Vec<Event>>,
    wake: Condvar,
}

impl Events {
    fn push(&self, event: Event) {
        self.queue.lock().push(event);
        self.wake.notify_all();
    }
}

struct Shared {
    deps: Deps,
    events: Events,
    lines: Mutex<VecDeque<String>>,
    revision: AtomicU64,
    next_id: AtomicU64,
}

impl Shared {
    fn id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// `AddSchedulerLogEntry`: the window's list, newest first, and the `Scheduler` log file. FIX: the list stops at 250 lines.
    fn log(&self, show: &str, text: &str) {
        let mut lines = self.lines.lock();
        lines.push_front(format!("{} ( {show} ) {text}", self.deps.clock.now().settings_text()));
        lines.truncate(LOG_LIMIT);
        drop(lines);
        self.revision.fetch_add(1, Ordering::SeqCst);
        self.file(show, text);
    }

    fn file(&self, show: &str, text: &str) {
        logging::info(LOGGER, &format!("({show}) {text}"));
    }

    fn execute(self: &Arc<Self>, run: &ShowRun, state: State, index: usize, now: LocalDateTime) -> Running {
        let item = &run.items[index];
        let name = item.name_text();
        self.file(&run.show_name, &format!("ExecuteAction: {name} with state of {state:?}"));
        if run.runner.needs_pre_process(item) {
            self.log(&run.show_name, &format!("Pre-processing action: {name}"));
        }
        self.log(&run.show_name, &format!("Starting action: {name}"));
        let token = self.id();
        let (shared, run_id) = (self.clone(), run.id);
        let done = Done::new(move || shared.events.push(Event::Completed { run: run_id, token }));
        Running { token, item: index, started: now, handle: run.runner.execute(item, done) }
    }
}

struct Running {
    token: u64,
    item: usize,
    started: LocalDateTime,
    handle: Box<dyn RunningAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    PreProcessing,
    Startup,
    Playing,
    ShuttingDown,
}

/// One run of a show, on its own copy of the items (F100).
struct ShowRun {
    runner: Arc<dyn ActionRunner>,
    id: u64,
    show_name: String,
    /// By `ItemOrder`.
    items: Vec<ShowItem>,
    /// `InProcessEndTime`.
    end_time: LocalDateTime,
    phase: Phase,
    queue: VecDeque<usize>,
    current: Option<Running>,
    background: Vec<Running>,
    pass_started: LocalDateTime,
    requeue_at: Option<LocalDateTime>,
    restarts: Vec<(usize, LocalDateTime)>,
    idle_logged: bool,
    cancelled: Arc<AtomicBool>,
}

impl ShowRun {
    fn of_type(&self, item_type: ShowItemType) -> VecDeque<usize> {
        (0..self.items.len()).filter(|i| self.items[*i].item_type == item_type).collect()
    }
}

#[derive(Default)]
struct Slot {
    run: Option<ShowRun>,
    /// A graceful stop has been asked for.
    stopping: bool,
}

impl Slot {
    fn state(&self) -> State {
        match &self.run {
            None => State::Waiting,
            Some(run) if self.stopping || run.phase == Phase::ShuttingDown => State::Shutdown,
            Some(_) => State::Running,
        }
    }

    /// `CheckForShutdown`.
    fn shutdown_due(&self, now: &LocalDateTime) -> bool {
        self.stopping || self.run.as_ref().is_some_and(|run| run.end_time <= *now)
    }
}

fn start(shared: &Arc<Shared>, slot: &mut Slot, item: &ScheduleItem, show: Show, now: LocalDateTime) {
    let unsorted = show.items.clone();
    let mut items = show.items;
    items.sort_by_key(|i| i.item_order);
    let run = ShowRun {
        runner: shared.deps.runner.for_run().unwrap_or_else(|| shared.deps.runner.clone()),
        id: shared.id(),
        show_name: show.name.unwrap_or_default(),
        items,
        end_time: item.end_time_today(&now),
        phase: Phase::PreProcessing,
        queue: VecDeque::new(),
        current: None,
        background: Vec::new(),
        pass_started: now,
        requeue_at: None,
        restarts: Vec::new(),
        idle_logged: false,
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    shared.log(&run.show_name, "Show started");
    let (worker, name, id, cancelled) = (shared.clone(), run.show_name.clone(), run.id, run.cancelled.clone());
    let runner = run.runner.clone();
    slot.stopping = false;
    slot.run = Some(run);
    (shared.deps.spawn)(Box::new(move || {
        for item in &unsorted {
            if cancelled.load(Ordering::SeqCst) {
                return;
            }
            worker.log(&name, &format!("Pre-processing: {}", item.name_text()));
            runner.pre_process(item);
        }
        worker.events.push(Event::PreProcessed { run: id });
    }));
}

/// Runs the next queued item, or moves the run on when the queue is empty.
fn next_queued(shared: &Arc<Shared>, slot: &mut Slot, now: LocalDateTime) {
    let state = slot.state();
    let Some(run) = slot.run.as_mut() else { return };
    if run.queue.is_empty() {
        match run.phase {
            Phase::PreProcessing => return,
            Phase::Startup => {
                run.phase = Phase::Playing;
                run.pass_started = now;
                run.queue = run.of_type(ShowItemType::Sequential);
                shared.file(&run.show_name, "BeginSequential");
                match run.queue.pop_front() {
                    Some(index) => {
                        let running = shared.execute(run, state, index, now);
                        run.current = Some(running);
                    }
                    // KEEP: a show with no Sequential items sits here until it's stopped.
                    None => shared.file(&run.show_name, "BeginSequential: Nothing in queue."),
                }
                for index in run.of_type(ShowItemType::Background) {
                    let running = shared.execute(run, state, index, now);
                    run.background.push(running);
                }
                return;
            }
            Phase::Playing => {
                let sequential = run.of_type(ShowItemType::Sequential);
                if sequential.is_empty() {
                    return;
                }
                // GUARD: a pass in which nothing played for a second waits, where upstream recurses until the stack runs out.
                if millis_between(&run.pass_started, &now) < 1000 {
                    run.requeue_at = Some(plus_seconds(now, 1));
                    if !run.idle_logged {
                        run.idle_logged = true;
                        shared.log(&run.show_name, "Show has nothing to play");
                    }
                    return;
                }
                shared.file(&run.show_name, "ExecuteNextSequentialItem: Nothing left in the queue. Restarting the queue.");
                run.pass_started = now;
                run.queue = sequential;
            }
            Phase::ShuttingDown => {
                shared.log(&run.show_name, "Show complete");
                run.runner.release(&run.items);
                slot.run = None;
                slot.stopping = false;
                return;
            }
        }
    }
    if let Some(index) = run.queue.pop_front() {
        let running = shared.execute(run, state, index, now);
        run.current = Some(running);
    }
}

/// `BeginShutdown`: everything running stops, then the Shutdown items run in order.
fn begin_shutdown(shared: &Arc<Shared>, slot: &mut Slot, now: LocalDateTime) {
    let Some(run) = slot.run.as_mut() else { return };
    shared.log(&run.show_name, "Starting shutdown");
    run.cancelled.store(true, Ordering::SeqCst);
    for mut running in run.background.drain(..).chain(run.current.take()) {
        running.handle.stop();
    }
    run.restarts.clear();
    run.requeue_at = None;
    run.phase = Phase::ShuttingDown;
    run.queue = run.of_type(ShowItemType::Shutdown);
    next_queued(shared, slot, now);
}

fn completed(shared: &Arc<Shared>, slot: &mut Slot, token: u64, now: LocalDateTime) {
    let due = slot.shutdown_due(&now);
    let state = slot.state();
    let Some(run) = slot.run.as_mut() else { return };
    if run.current.as_ref().is_some_and(|r| r.token == token) {
        let name = run.current.take().map(|r| run.items[r.item].name_text().to_string()).unwrap_or_default();
        match run.phase {
            Phase::PreProcessing => {}
            Phase::Startup => {
                shared.log(&run.show_name, &format!("Startup action complete: {name}"));
                next_queued(shared, slot, now);
            }
            Phase::Playing => {
                shared.log(&run.show_name, &format!("Sequential action complete: {name}"));
                if due {
                    shared.file(&run.show_name, "OnSequentialActionComplete: Shutdown IS requested");
                    begin_shutdown(shared, slot, now);
                } else {
                    shared.file(&run.show_name, "OnSequentialActionComplete: Shutdown was NOT requested");
                    next_queued(shared, slot, now);
                }
            }
            Phase::ShuttingDown => {
                shared.log(&run.show_name, &format!("Shutdown action complete: {name}"));
                next_queued(shared, slot, now);
            }
        }
    } else if let Some(position) = run.background.iter().position(|r| r.token == token) {
        let ended = run.background.remove(position);
        shared.log(&run.show_name, &format!("Background action complete: {}", run.items[ended.item].name_text()));
        if due {
            return;
        }
        // GUARD: one that ends at once restarts a second later, not in a spin.
        if millis_between(&ended.started, &now) < 1000 {
            run.restarts.push((ended.item, plus_seconds(now, 1)));
        } else {
            let running = shared.execute(run, state, ended.item, now);
            run.background.push(running);
        }
    }
    // Anything else is from an action already stopped: nothing to do.
}

/// What's due without any action having ended: a waiting re-queue, a background restart.
fn timers(shared: &Arc<Shared>, slot: &mut Slot, now: LocalDateTime) {
    let due = slot.shutdown_due(&now);
    let state = slot.state();
    let Some(run) = slot.run.as_mut() else { return };
    let ready: Vec<usize> = run.restarts.iter().filter(|(_, at)| *at <= now).map(|(item, _)| *item).collect();
    run.restarts.retain(|(_, at)| *at > now);
    if !due {
        for index in ready {
            let running = shared.execute(run, state, index, now);
            run.background.push(running);
        }
    }
    if run.requeue_at.is_some_and(|at| at <= now) {
        run.requeue_at = None;
        run.pass_started = now;
        run.queue = run.of_type(ShowItemType::Sequential);
        next_queued(shared, slot, now);
    }
}

fn stop_slot(shared: &Arc<Shared>, slot: &mut Slot, gracefully: bool, now: LocalDateTime) {
    let Some(run) = slot.run.as_mut() else { return };
    run.cancelled.store(true, Ordering::SeqCst);
    if gracefully {
        shared.log(&run.show_name, "Show stopping gracefully");
        slot.stopping = true;
        // GUARD (F101): with no sequential item to wait for, upstream never reaches the Shutdown items and the schedule sticks.
        let waits = run.phase == Phase::Playing && run.current.is_some();
        if !waits && run.phase != Phase::ShuttingDown {
            begin_shutdown(shared, slot, now);
        }
    } else if let Some(mut run) = slot.run.take() {
        slot.stopping = false;
        for mut running in run.current.take().into_iter().chain(run.background.drain(..)) {
            shared.log(&run.show_name, &format!("Stopping action: {}", run.items[running.item].name_text()));
            running.handle.stop();
        }
        run.runner.release(&run.items);
        shared.log(&run.show_name, "Show stopped immediately");
    }
}

struct Inner {
    data: SchedulerData,
    /// One for each of `data.items`.
    slots: Vec<Slot>,
    enabled: bool,
    manually_disabled: bool,
    last_check: Option<LocalDateTime>,
    status: String,
    told: Option<(Status, u64, SchedulerData)>,
}

/// `ScheduleExecutor` and the schedule it runs.
pub struct Scheduler {
    shared: Arc<Shared>,
    inner: Mutex<Inner>,
    listener: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    quit: AtomicBool,
}

impl Scheduler {
    pub fn new(data: SchedulerData, deps: Deps) -> Arc<Scheduler> {
        let slots = data.items.iter().map(|_| Slot::default()).collect();
        Arc::new(Scheduler {
            shared: Arc::new(Shared {
                deps,
                events: Events::default(),
                lines: Mutex::new(VecDeque::new()),
                revision: AtomicU64::new(0),
                next_id: AtomicU64::new(1),
            }),
            inner: Mutex::new(Inner {
                data,
                slots,
                enabled: false,
                manually_disabled: false,
                last_check: None,
                status: INITIAL_STATUS.to_string(),
                told: None,
            }),
            listener: Mutex::new(None),
            quit: AtomicBool::new(false),
        })
    }

    /// Called, from any thread and with no lock held, when the status, the log or the schedule has changed.
    pub fn set_listener(&self, listener: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.listener.lock() = listener;
    }

    fn set_executor_enabled(&self, inner: &mut Inner, on: bool) {
        inner.enabled = on;
        logging::info(LOGGER, if on { "Schedule Enabled" } else { "Schedule Disabled" });
    }

    fn stop_all(&self, inner: &mut Inner, gracefully: bool, now: LocalDateTime) {
        inner.manually_disabled = true;
        for slot in &mut inner.slots {
            stop_slot(&self.shared, slot, gracefully, now);
        }
    }

    /// `CheckSchedule`.
    fn check(&self, inner: &mut Inner, now: LocalDateTime) {
        if inner.data.is_enabled && !inner.enabled {
            self.set_executor_enabled(inner, true);
        } else if !inner.data.is_enabled && inner.enabled {
            // KEEP: this leaves the scheduler manually disabled, so turning it back on waits for Start.
            self.stop_all(inner, false, now);
            self.set_executor_enabled(inner, false);
        }
        if inner.enabled && !inner.manually_disabled {
            let Inner { data, slots, .. } = inner;
            for (item, slot) in data.items.iter().zip(slots.iter_mut()) {
                if slot.run.is_none() && item.should_start(&now) {
                    // FIX (F100): the show as it is now, not as it was when the schedule first looked.
                    if let Some(show) = (self.shared.deps.shows)(item.show_id) {
                        start(&self.shared, slot, item, show, now);
                    }
                }
            }
        }
    }

    fn drain(&self, inner: &mut Inner, now: LocalDateTime) {
        loop {
            let events = std::mem::take(&mut *self.shared.events.queue.lock());
            if events.is_empty() {
                return;
            }
            for event in events {
                let (run, token) = match event {
                    Event::Completed { run, token } => (run, Some(token)),
                    Event::PreProcessed { run } => (run, None),
                };
                // A run that's over, or was stopped, has no slot: its late news is dropped here.
                let Some(slot) = inner.slots.iter_mut().find(|s| s.run.as_ref().is_some_and(|r| r.id == run)) else { continue };
                match token {
                    Some(token) => completed(&self.shared, slot, token, now),
                    None => {
                        if let Some(run) = slot.run.as_mut().filter(|r| r.phase == Phase::PreProcessing) {
                            run.phase = Phase::Startup;
                            run.queue = run.of_type(ShowItemType::Startup);
                            next_queued(&self.shared, slot, now);
                        }
                    }
                }
            }
        }
    }

    fn refresh_status(&self, inner: &mut Inner, now: LocalDateTime) {
        let rows: Vec<(&ScheduleItem, State, Option<&str>)> = inner
            .data
            .items
            .iter()
            .zip(&inner.slots)
            .map(|(item, slot)| (item, slot.state(), slot.run.as_ref().map(|r| r.show_name.as_str())))
            .collect();
        let shows = &self.shared.deps.shows;
        inner.status = status_text(inner.manually_disabled, &rows, &|id| shows(id).map(|s| s.name.unwrap_or_default()), &now);
    }

    fn work(&self, now: LocalDateTime, force: bool) {
        let changed = {
            let mut inner = self.inner.lock();
            self.drain(&mut inner, now);
            for slot in &mut inner.slots {
                timers(&self.shared, slot, now);
            }
            self.drain(&mut inner, now);
            let interval = inner.data.check_interval() * 1000;
            let due = inner.last_check.is_none_or(|last| millis_between(&last, &now) >= interval);
            if force || (inner.enabled && due) {
                inner.last_check = Some(now);
                self.check(&mut inner, now);
                self.drain(&mut inner, now);
                self.refresh_status(&mut inner, now);
            }
            let seen = (self.status_of(&inner), self.shared.revision.load(Ordering::SeqCst), inner.data.clone());
            let changed = inner.told.as_ref() != Some(&seen);
            inner.told = Some(seen);
            changed
        };
        if changed {
            let listener = self.listener.lock().clone();
            if let Some(listener) = listener {
                listener();
            }
        }
    }

    /// What the driver thread calls, several times a second: handles what has ended and, when the interval has passed, checks the
    /// schedule.
    pub fn poll(&self, now: LocalDateTime) {
        self.work(now, false);
    }

    /// A check right now: at startup (F59), and after a change that shouldn't wait for the timer.
    pub fn tick(&self, now: LocalDateTime) {
        self.work(now, true);
    }

    /// Scheduler > Enabled (F54). The caller saves.
    pub fn set_enabled(&self, on: bool) {
        self.inner.lock().data.is_enabled = on;
        self.tick(self.shared.deps.clock.now());
    }

    /// The Status window's Start.
    pub fn start(&self) {
        self.inner.lock().manually_disabled = false;
        self.tick(self.shared.deps.clock.now());
    }

    /// Stop Now and Stop Gracefully. Either leaves the scheduler off until Start.
    pub fn stop(&self, gracefully: bool) {
        let now = self.shared.deps.clock.now();
        {
            let mut inner = self.inner.lock();
            self.stop_all(&mut inner, gracefully, now);
            self.refresh_status(&mut inner, now);
        }
        self.poll(now);
    }

    fn status_of(&self, inner: &Inner) -> Status {
        Status {
            text: inner.status.clone(),
            enabled: inner.enabled,
            can_start: inner.manually_disabled,
            can_stop: !inner.manually_disabled,
        }
    }

    pub fn status(&self) -> Status {
        self.status_of(&self.inner.lock())
    }

    /// Newest first.
    pub fn log_lines(&self) -> Vec<String> {
        self.shared.lines.lock().iter().cloned().collect()
    }

    pub fn data(&self) -> SchedulerData {
        self.inner.lock().data.clone()
    }

    pub fn item_state(&self, index: usize) -> Option<State> {
        self.inner.lock().slots.get(index).map(Slot::state)
    }

    pub fn add_item(&self, item: ScheduleItem) {
        let mut inner = self.inner.lock();
        inner.data.items.push(item);
        inner.slots.push(Slot::default());
    }

    /// A run in progress carries on with the end time it started with.
    pub fn set_item(&self, index: usize, item: ScheduleItem) {
        if let Some(slot) = self.inner.lock().data.items.get_mut(index) {
            *slot = item;
        }
    }

    /// FIX (F51): a running schedule is stopped as it's deleted, where upstream leaves it playing with no way to reach it.
    pub fn delete_item(&self, index: usize) {
        let now = self.shared.deps.clock.now();
        let mut inner = self.inner.lock();
        if index < inner.data.items.len() {
            stop_slot(&self.shared, &mut inner.slots[index], false, now);
            inner.data.items.remove(index);
            inner.slots.remove(index);
        }
    }

    pub fn set_status_position(&self, x: i32, y: i32) {
        self.inner.lock().data.status_position = (x, y);
    }

    // STUDIO-BEGIN schedule-replace-items: the Schedule pane's live edits (and their Undo) replace the whole list at once
    /// The schedules as the Schedule pane or an agent left them. A schedule that didn't change keeps its run; one that was
    /// deleted or changed while its show plays stops it, as Delete does.
    pub fn studio_replace_items(&self, items: Vec<ScheduleItem>) {
        let now = self.shared.deps.clock.now();
        {
            let mut inner = self.inner.lock();
            let Inner { data, slots, .. } = &mut *inner;
            let mut old: Vec<Option<(ScheduleItem, Slot)>> =
                data.items.drain(..).zip(slots.drain(..)).map(Some).collect();
            for item in &items {
                let kept = old.iter().position(|o| o.as_ref().is_some_and(|(i, _)| i == item));
                slots.push(kept.and_then(|k| old[k].take()).map(|(_, slot)| slot).unwrap_or_default());
            }
            for (_, mut slot) in old.into_iter().flatten() {
                stop_slot(&self.shared, &mut slot, false, now);
            }
            data.items = items;
            self.refresh_status(&mut inner, now);
        }
        self.poll(now);
    }
    // STUDIO-END schedule-replace-items

    /// The thread behind the timer. It ends at `quit`.
    pub fn run(self: &Arc<Self>) {
        let scheduler = self.clone();
        let _ = std::thread::Builder::new().name("scheduler".into()).spawn(move || {
            while !scheduler.quit.load(Ordering::SeqCst) {
                scheduler.poll(scheduler.shared.deps.clock.now());
                let mut queue = scheduler.shared.events.queue.lock();
                if queue.is_empty() {
                    scheduler.shared.events.wake.wait_for(&mut queue, Duration::from_millis(100));
                }
            }
        });
    }

    /// The app is quitting: `Enabled = false`, which upstream logs on every exit. Running shows aren't told to stop, as there.
    pub fn quit(&self) {
        self.quit.store(true, Ordering::SeqCst);
        self.shared.events.wake.notify_all();
        let mut inner = self.inner.lock();
        if inner.enabled {
            self.set_executor_enabled(&mut inner, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::actions::Finished;
    use super::super::tests::at;
    use super::super::{Date, TimeOfDay};
    use super::*;
    use crate::shows::ActionType;

    struct TestClock(Mutex<LocalDateTime>);

    impl Clock for TestClock {
        fn now(&self) -> LocalDateTime {
            *self.0.lock()
        }
    }

    /// Records what was started and lets the test end it. Items named `instant…` end as they start.
    #[derive(Default)]
    struct TestRunner {
        started: Mutex<Vec<(String, Done)>>,
        stopped: Arc<Mutex<Vec<String>>>,
        released: Mutex<usize>,
    }

    struct TestHandle(String, Arc<Mutex<Vec<String>>>, Done);

    impl RunningAction for TestHandle {
        fn stop(&mut self) {
            self.1.lock().push(self.0.clone());
            self.2.complete();
        }
    }

    impl ActionRunner for TestRunner {
        fn execute(&self, item: &ShowItem, done: Done) -> Box<dyn RunningAction> {
            let name = item.name_text().to_string();
            if name.starts_with("instant") {
                done.complete();
                return Box::new(Finished);
            }
            self.started.lock().push((name.clone(), done.clone()));
            Box::new(TestHandle(name, self.stopped.clone(), done))
        }

        fn release(&self, _items: &[ShowItem]) {
            *self.released.lock() += 1;
        }
    }

    impl TestRunner {
        fn running(&self) -> Vec<String> {
            self.started.lock().iter().map(|(name, _)| name.clone()).collect()
        }

        fn finish(&self, name: &str) {
            let mut started = self.started.lock();
            let position = started.iter().position(|(n, _)| n == name).unwrap_or_else(|| panic!("{name} isn't running"));
            started.remove(position).1.complete();
        }
    }

    fn item(item_type: ShowItemType, name: &str, order: i32) -> ShowItem {
        let mut item = ShowItem::new(item_type, name);
        item.action = ActionType::Pause;
        item.item_order = order;
        item
    }

    fn show(items: Vec<ShowItem>) -> Show {
        Show { id: Uuid::from_u128(7), name: Some("Xmas".to_string()), items }
    }

    fn full_show() -> Show {
        show(vec![
            item(ShowItemType::Shutdown, "off", 0),
            item(ShowItemType::Sequential, "two", 1),
            item(ShowItemType::Startup, "hello", 0),
            item(ShowItemType::Sequential, "one", 0),
            item(ShowItemType::Background, "snow", 0),
        ])
    }

    /// 7 pm to 9 pm every day of December 2025.
    fn evening() -> ScheduleItem {
        ScheduleItem {
            show_id: Uuid::from_u128(7),
            start_date: Date { year: 2025, month: 12, day: 1 },
            end_date: Date { year: 2025, month: 12, day: 31 },
            start_time: TimeOfDay { hour: 19, minute: 0, second: 0 },
            end_time: TimeOfDay { hour: 21, minute: 0, second: 0 },
            ..ScheduleItem::default()
        }
    }

    struct Rig {
        scheduler: Arc<Scheduler>,
        runner: Arc<TestRunner>,
        clock: Arc<TestClock>,
        shows: Arc<Mutex<Vec<Show>>>,
    }

    impl Rig {
        fn new(shows: Vec<Show>, items: Vec<ScheduleItem>, now: LocalDateTime) -> Rig {
            let runner = Arc::new(TestRunner::default());
            let clock = Arc::new(TestClock(Mutex::new(now)));
            let shows = Arc::new(Mutex::new(shows));
            let lookup = shows.clone();
            let deps = Deps {
                clock: clock.clone(),
                runner: runner.clone(),
                shows: Arc::new(move |id| lookup.lock().iter().find(|s| s.id == id).cloned()),
                spawn: Arc::new(|job| job()),
            };
            let data = SchedulerData { is_enabled: true, items, ..SchedulerData::default() };
            Rig { scheduler: Scheduler::new(data, deps), runner, clock, shows }
        }

        /// Moves the clock and polls.
        fn at(&self, now: LocalDateTime) {
            *self.clock.0.lock() = now;
            self.scheduler.poll(now);
        }

        fn poll(&self) {
            self.scheduler.poll(self.clock.now());
        }

        /// The log's texts, oldest first, without the time and show.
        fn log(&self) -> Vec<String> {
            let mut lines: Vec<String> =
                self.scheduler.log_lines().iter().map(|l| l.split(" ) ").nth(1).unwrap_or(l).to_string()).collect();
            lines.reverse();
            lines
        }

        fn text(&self) -> String {
            self.scheduler.status().text
        }
    }

    #[test]
    fn a_show_runs_from_startup_to_complete() {
        let rig = Rig::new(vec![full_show()], vec![evening()], at(2025, 12, 13, 18, 59, 59));
        rig.scheduler.tick(rig.clock.now());
        assert_eq!(rig.text(), "Up Next: Xmas on 12/13/2025 7:00:00 PM");
        assert!(rig.runner.running().is_empty());

        rig.at(at(2025, 12, 13, 19, 0, 0));
        assert_eq!(rig.text(), "Running: Xmas");
        assert_eq!(rig.runner.running(), ["hello"]);
        rig.runner.finish("hello");
        rig.poll();
        assert_eq!(rig.runner.running(), ["one", "snow"]);

        rig.at(at(2025, 12, 13, 19, 5, 0));
        rig.runner.finish("one");
        rig.poll();
        rig.runner.finish("two");
        rig.poll();
        // The queue starts over, and a background item that ends starts again.
        assert_eq!(rig.runner.running(), ["snow", "one"]);
        rig.runner.finish("snow");
        rig.poll();
        assert_eq!(rig.runner.running(), ["one", "snow"]);

        // The end time doesn't cut an item off: the show ends when the one playing does.
        rig.at(at(2025, 12, 13, 21, 0, 1));
        assert_eq!(rig.text(), "Running: Xmas");
        rig.runner.finish("one");
        rig.poll();
        assert_eq!(*rig.runner.stopped.lock(), ["snow"]);
        assert_eq!(rig.runner.running(), ["snow", "off"]);
        assert_eq!(rig.scheduler.item_state(0), Some(State::Shutdown));
        rig.runner.finish("off");
        rig.at(at(2025, 12, 13, 21, 0, 2));
        assert_eq!(rig.scheduler.item_state(0), Some(State::Waiting));
        assert_eq!(*rig.runner.released.lock(), 1);
        assert_eq!(rig.text(), "Nothing scheduled.... ");
        assert_eq!(
            rig.log(),
            [
                "Show started",
                "Pre-processing: off",
                "Pre-processing: two",
                "Pre-processing: hello",
                "Pre-processing: one",
                "Pre-processing: snow",
                "Starting action: hello",
                "Startup action complete: hello",
                "Starting action: one",
                "Starting action: snow",
                "Sequential action complete: one",
                "Starting action: two",
                "Sequential action complete: two",
                "Starting action: one",
                "Background action complete: snow",
                "Starting action: snow",
                "Sequential action complete: one",
                "Starting shutdown",
                "Starting action: off",
                "Shutdown action complete: off",
                "Show complete",
            ]
        );
        assert_eq!(rig.scheduler.log_lines()[0], "12/13/2025 9:00:02 PM ( Xmas ) Show complete");
    }

    #[test]
    fn stop_now_ends_everything_and_waits_for_start() {
        let rig = Rig::new(vec![full_show()], vec![evening()], at(2025, 12, 13, 19, 30, 0));
        rig.scheduler.tick(rig.clock.now());
        rig.runner.finish("hello");
        rig.poll();
        rig.scheduler.stop(false);
        assert_eq!(*rig.runner.stopped.lock(), ["one", "snow"]);
        assert_eq!(rig.log()[rig.log().len() - 3..], ["Stopping action: one", "Stopping action: snow", "Show stopped immediately"]);
        assert_eq!(rig.text(), "All Show Disabled");
        let status = rig.scheduler.status();
        assert!(status.can_start && !status.can_stop);
        // The stopped actions' late completions change nothing, and nothing restarts while it's off.
        let lines = rig.log().len();
        rig.at(at(2025, 12, 13, 19, 30, 5));
        assert_eq!(rig.log().len(), lines);
        assert_eq!(rig.scheduler.item_state(0), Some(State::Waiting));

        // KEEP: Start inside the window runs the show again from the top.
        rig.runner.started.lock().clear();
        rig.scheduler.start();
        assert_eq!(rig.runner.running(), ["hello"]);
        assert_eq!(rig.text(), "Running: Xmas");
    }

    #[test]
    fn a_graceful_stop_lets_the_sequential_item_finish() {
        let rig = Rig::new(vec![full_show()], vec![evening()], at(2025, 12, 13, 19, 30, 0));
        rig.scheduler.tick(rig.clock.now());
        rig.runner.finish("hello");
        rig.poll();
        rig.scheduler.stop(true);
        assert!(rig.runner.stopped.lock().is_empty());
        assert_eq!(rig.scheduler.item_state(0), Some(State::Shutdown));
        // A background item that ends now isn't restarted.
        rig.runner.finish("snow");
        rig.poll();
        assert_eq!(rig.runner.running(), ["one"]);
        rig.runner.finish("one");
        rig.poll();
        assert_eq!(rig.runner.running(), ["off"]);
        rig.runner.finish("off");
        rig.poll();
        assert_eq!(rig.scheduler.item_state(0), Some(State::Waiting));
        assert_eq!(rig.log().last().unwrap(), "Show complete");
        assert_eq!(rig.text(), "All Show Disabled");
    }

    #[test]
    fn a_graceful_stop_during_startup_goes_straight_to_shutdown() {
        let rig = Rig::new(vec![full_show()], vec![evening()], at(2025, 12, 13, 19, 30, 0));
        rig.scheduler.tick(rig.clock.now());
        assert_eq!(rig.runner.running(), ["hello"]);
        rig.scheduler.stop(true);
        assert_eq!(*rig.runner.stopped.lock(), ["hello"]);
        assert_eq!(rig.runner.running(), ["hello", "off"]);
        rig.runner.finish("off");
        rig.poll();
        assert_eq!(rig.scheduler.item_state(0), Some(State::Waiting));
        assert!(!rig.log().iter().any(|l| l.starts_with("Startup action complete")));
    }

    #[test]
    fn a_show_with_nothing_to_play_waits_a_second_between_passes() {
        let mut idle = show(vec![item(ShowItemType::Sequential, "instant a", 0), item(ShowItemType::Sequential, "instant b", 1)]);
        idle.items.push(item(ShowItemType::Background, "instant snow", 0));
        let rig = Rig::new(vec![idle], vec![evening()], at(2025, 12, 13, 19, 30, 0));
        rig.scheduler.tick(rig.clock.now());
        let starts = |rig: &Rig| rig.log().iter().filter(|l| *l == "Starting action: instant a").count();
        assert_eq!(starts(&rig), 1);
        assert_eq!(rig.log().iter().filter(|l| *l == "Show has nothing to play").count(), 1);
        rig.poll();
        assert_eq!(starts(&rig), 1);
        rig.at(at(2025, 12, 13, 19, 30, 1));
        assert_eq!(starts(&rig), 2);
        assert_eq!(rig.log().iter().filter(|l| *l == "Starting action: instant snow").count(), 2);
        rig.at(at(2025, 12, 13, 19, 30, 2));
        assert_eq!(starts(&rig), 3);
        assert_eq!(rig.log().iter().filter(|l| *l == "Show has nothing to play").count(), 1);

        // Past the end time the pass's first item finds the shutdown due.
        rig.at(at(2025, 12, 13, 21, 0, 0));
        assert_eq!(rig.scheduler.item_state(0), Some(State::Waiting));
        assert_eq!(rig.log().last().unwrap(), "Show complete");
    }

    #[test]
    fn the_show_is_looked_up_when_a_run_starts() {
        let rig = Rig::new(vec![full_show()], vec![evening()], at(2025, 12, 13, 18, 0, 0));
        rig.scheduler.tick(rig.clock.now());
        rig.shows.lock()[0] = Show { name: Some("Edited".to_string()), items: vec![item(ShowItemType::Sequential, "new", 0)], ..full_show() };
        rig.at(at(2025, 12, 13, 19, 0, 0));
        assert_eq!(rig.text(), "Running: Edited");
        assert_eq!(rig.runner.running(), ["new"]);

        // A deleted show doesn't run, and its schedule says so.
        let gone = Rig::new(vec![full_show()], vec![evening()], at(2025, 12, 13, 18, 0, 0));
        gone.shows.lock().clear();
        gone.scheduler.tick(gone.clock.now());
        assert_eq!(gone.text(), "Up Next: Show not selected on 12/13/2025 7:00:00 PM");
        gone.at(at(2025, 12, 13, 19, 0, 0));
        assert_eq!(gone.scheduler.item_state(0), Some(State::Waiting));
        assert!(gone.log().is_empty());
    }

    #[test]
    fn turning_enabled_off_stops_now_and_stays_manually_disabled() {
        let rig = Rig::new(vec![full_show()], vec![evening()], at(2025, 12, 13, 19, 30, 0));
        rig.scheduler.tick(rig.clock.now());
        assert!(rig.scheduler.status().enabled);
        rig.scheduler.set_enabled(false);
        assert!(!rig.scheduler.status().enabled);
        assert_eq!(rig.log().last().unwrap(), "Show stopped immediately");
        rig.scheduler.set_enabled(true);
        assert_eq!(rig.text(), "All Show Disabled");
        assert_eq!(rig.scheduler.item_state(0), Some(State::Waiting));
        assert!(rig.scheduler.data().is_enabled);
    }

    #[test]
    fn deleting_a_running_schedule_stops_it() {
        let rig = Rig::new(vec![full_show()], vec![evening(), evening()], at(2025, 12, 13, 19, 30, 0));
        rig.scheduler.tick(rig.clock.now());
        assert_eq!(rig.runner.running(), ["hello", "hello"]);
        rig.scheduler.delete_item(0);
        assert_eq!(*rig.runner.stopped.lock(), ["hello"]);
        assert_eq!(rig.scheduler.data().items.len(), 1);
        assert_eq!(rig.scheduler.item_state(0), Some(State::Running));
        assert!(rig.scheduler.status().can_stop);
    }

    #[test]
    fn the_log_keeps_its_newest_250_lines_and_the_listener_hears_of_changes() {
        let rig = Rig::new(vec![], vec![], at(2025, 12, 13, 12, 0, 0));
        let heard = Arc::new(AtomicU64::new(0));
        let count = heard.clone();
        rig.scheduler.set_listener(Some(Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        })));
        for n in 0..300 {
            rig.scheduler.shared.log("Show", &format!("line {n}"));
        }
        let lines = rig.scheduler.log_lines();
        assert_eq!(lines.len(), LOG_LIMIT);
        assert_eq!(lines[0], "12/13/2025 12:00:00 PM ( Show ) line 299");
        assert_eq!(lines[249], "12/13/2025 12:00:00 PM ( Show ) line 50");
        rig.poll();
        rig.poll();
        assert_eq!(heard.load(Ordering::SeqCst), 1);
        rig.scheduler.add_item(evening());
        rig.poll();
        assert_eq!(heard.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn the_status_line_follows_upstreams_priorities() {
        let none = |_: Uuid| None;
        let noon = at(2025, 12, 13, 12, 0, 0);
        let (a, mut b) = (evening(), evening());
        b.start_time = TimeOfDay { hour: 17, minute: 30, second: 0 };
        assert_eq!(status_text(false, &[], &none, &noon), "Nothing scheduled.... ");
        let rows = [(&a, State::Waiting, None), (&b, State::Waiting, None)];
        assert_eq!(status_text(false, &rows, &|_| Some("B".to_string()), &noon), "Up Next: B on 12/13/2025 5:30:00 PM");
        assert_eq!(status_text(true, &rows, &none, &noon), "All Show Disabled");
        let rows = [(&a, State::Waiting, None), (&b, State::Shutdown, Some("B"))];
        assert_eq!(status_text(false, &rows, &none, &noon), "Shutdown: B");
        // KEEP: nothing is up next once today's start time has passed, or on the end date.
        let rows = [(&a, State::Waiting, None)];
        assert_eq!(status_text(false, &rows, &none, &at(2025, 12, 13, 21, 30, 0)), "Nothing scheduled.... ");
        assert_eq!(status_text(false, &rows, &none, &at(2025, 12, 31, 12, 0, 0)), "Nothing scheduled.... ");
        let mut off = evening();
        off.enabled = false;
        assert_eq!(status_text(false, &[(&off, State::Waiting, None)], &none, &noon), "Nothing scheduled.... ");
    }

    #[test]
    fn the_real_actions_run_under_the_driver_thread() {
        let mut echo = item(ShowItemType::Startup, "Launch: /bin/echo", 0);
        echo.action = ActionType::Launch;
        echo.launch_program_name = Some("/bin/echo".to_string());
        echo.launch_wait_for_exit = true;
        let mut wait = item(ShowItemType::Sequential, "Pause for 1", 0);
        wait.pause_seconds = 1;
        let mut window = evening();
        let now = LocalDateTime::now();
        window.start_date = Date::of(&now).plus_days(-1);
        window.end_date = Date::of(&now).plus_days(1);
        window.start_time = TimeOfDay::MIDNIGHT;
        window.end_time = TimeOfDay { hour: 23, minute: 59, second: 59 };
        let shows = vec![show(vec![echo, wait])];
        let runner = Arc::new(super::super::actions::StandardRunner { sequence: Arc::new(TestRunner::default()) });
        let deps = Deps::threaded(runner, Arc::new(move |id| shows.iter().find(|s| s.id == id).cloned()));
        let scheduler = Scheduler::new(SchedulerData { is_enabled: true, items: vec![window], ..SchedulerData::default() }, deps);
        scheduler.tick(LocalDateTime::now());
        scheduler.run();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let has = |text: &str| scheduler.log_lines().iter().any(|l| l.ends_with(text));
        while !has("Sequential action complete: Pause for 1") && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(has("Startup action complete: Launch: /bin/echo"), "{:?}", scheduler.log_lines());
        assert!(has("Sequential action complete: Pause for 1"));
        scheduler.stop(false);
        assert_eq!(scheduler.status().text, "All Show Disabled");
        scheduler.quit();
        assert!(!scheduler.status().enabled);
    }
}
