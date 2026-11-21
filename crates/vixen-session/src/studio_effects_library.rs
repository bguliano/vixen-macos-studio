//! Studio: the Effects library's Favorites and Recently Used (UI roadmap R38). Both sit above the categories and belong
//! to the user, not the show: they are kept in `Studio Effects Library.json` in the user's Vixen settings folder
//! (`vixen_io::settings_dir`, which a UI test points at its output folder), so they survive relaunch and follow the user
//! from show to show.
//!
//! Favorites are pinned by the star on an effect's row or by `effects_library` (`favorite` with `on`), in the order they
//! were pinned. Recently Used is every effect placed in the sequencer window, newest first: a drop on a row, Add Effect,
//! or a drag drawn in Draw mode (the hooks in `sequencer::state` and `timeline::host`). Agents' effects don't count,
//! since a script placing thousands of effects would push out what the user reached for. Like row height these are the
//! user's view rather than edits to a document, so they aren't on Undo: the star, pressed again, reverses a pin.
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;
use uuid::Uuid;

pub const FILE_NAME: &str = "Studio Effects Library.json";
/// Placements remembered; Recently Used shows the newest [`RECENT_SHOWN`] that aren't favorites.
const RECENT_KEPT: usize = 12;
pub const RECENT_SHOWN: usize = 5;

/// One user's favorites and recents, as stored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    pub favorites: Vec<Uuid>,
    pub recent: Vec<Uuid>,
}

impl Library {
    pub fn parse(text: &str) -> Library {
        let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let ids = |key: &str| -> Vec<Uuid> {
            let mut out: Vec<Uuid> = Vec::new();
            for id in value[key].as_array().into_iter().flatten().filter_map(|v| v.as_str()?.parse().ok()) {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
            out
        };
        let mut recent = ids("recent");
        recent.truncate(RECENT_KEPT);
        Library { favorites: ids("favorites"), recent }
    }

    pub fn to_text(&self) -> String {
        let ids = |list: &[Uuid]| list.iter().map(Uuid::to_string).collect::<Vec<_>>();
        let value = json!({"favorites": ids(&self.favorites), "recent": ids(&self.recent)});
        serde_json::to_string_pretty(&value).unwrap_or_default()
    }

    /// Pins or unpins one effect. False when it already was as asked.
    pub fn set_favorite(&mut self, id: Uuid, on: bool) -> bool {
        let pinned = self.favorites.contains(&id);
        if pinned == on {
            return false;
        }
        if on {
            self.favorites.push(id);
        } else {
            self.favorites.retain(|f| *f != id);
        }
        true
    }

    /// One placement: the effect moves to the front. False when it already was the newest.
    pub fn used(&mut self, id: Uuid) -> bool {
        if self.recent.first() == Some(&id) {
            return false;
        }
        self.recent.retain(|r| *r != id);
        self.recent.insert(0, id);
        self.recent.truncate(RECENT_KEPT);
        true
    }

    /// What the library shows: favorites, then the newest recents that aren't favorites. Effects no longer registered
    /// are left out.
    pub fn to_json(&self) -> Value {
        let entry = |id: &Uuid| {
            vixen_effect_registry::by_type_id(id).map(|d| json!({"typeId": id.to_string(), "name": d.name}))
        };
        let favorites: Vec<Value> = self.favorites.iter().filter_map(entry).collect();
        let recent: Vec<Value> = self
            .recent
            .iter()
            .filter(|id| !self.favorites.contains(id))
            .filter_map(entry)
            .take(RECENT_SHOWN)
            .collect();
        json!({"favorites": favorites, "recent": recent})
    }
}

/// The library as this process last read or wrote it, with the file's stamp then, so a change by another process (a
/// headless bridge while the app was closed) is read again.
struct Store {
    path: PathBuf,
    library: Library,
    stamp: Option<(SystemTime, u64)>,
}

#[cfg(not(test))]
static STORE: Mutex<Option<Store>> = Mutex::new(None);
// Unit tests run in parallel threads, each with its own store and file, and never touch the user's own favorites.
#[cfg(test)]
thread_local! {
    static STORE: Mutex<Option<Store>> = const { Mutex::new(None) };
}
static REVISION: AtomicU64 = AtomicU64::new(1);

fn default_path() -> PathBuf {
    #[cfg(test)]
    return std::env::temp_dir().join(format!("studio-effects-library-{}", Uuid::new_v4())).join(FILE_NAME);
    #[cfg(not(test))]
    vixen_io::settings_dir().join(FILE_NAME)
}

fn locked<T>(f: impl FnOnce(&mut Option<Store>) -> T) -> T {
    #[cfg(not(test))]
    return f(&mut STORE.lock());
    #[cfg(test)]
    STORE.with(|store| f(&mut store.lock()))
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

fn with_store<T>(f: impl FnOnce(&mut Store) -> T) -> T {
    locked(|guard| with_loaded(guard, f))
}

fn with_loaded<T>(guard: &mut Option<Store>, f: impl FnOnce(&mut Store) -> T) -> T {
    let store = guard.get_or_insert_with(|| Store { path: default_path(), library: Library::default(), stamp: None });
    let now = stamp(&store.path);
    if now != store.stamp {
        let library = std::fs::read_to_string(&store.path).map(|t| Library::parse(&t)).unwrap_or_default();
        if library != store.library {
            REVISION.fetch_add(1, Ordering::Relaxed);
        }
        store.library = library;
        store.stamp = now;
    }
    f(store)
}

fn save(store: &mut Store) -> Result<(), String> {
    REVISION.fetch_add(1, Ordering::Relaxed);
    vixen_io::file::write_atomic(&store.path, store.library.to_text().as_bytes()).map_err(|e| e.to_string())?;
    store.stamp = stamp(&store.path);
    Ok(())
}

/// Points the store at another file (tests).
pub fn set_path(path: PathBuf) {
    locked(|store| *store = Some(Store { path, library: Library::default(), stamp: None }));
    REVISION.fetch_add(1, Ordering::Relaxed);
}

/// Changes whenever the favorites or recents do, for the window's poll: cheap, no file access.
pub fn revision() -> u64 {
    REVISION.load(Ordering::Relaxed)
}

/// The sequencer window placed an effect of this type. Writes only when the newest recent changes, so a drawn drag
/// across many rows writes once, and never fails the placement.
pub fn used(type_id: Uuid) {
    with_store(|store| {
        if store.library.used(type_id) {
            let _ = save(store);
        }
    });
}

pub fn state() -> Value {
    with_store(|store| store.library.to_json())
}

/// `effects_library`: reads the library, pins or unpins one effect (`favorite` with `on`), or clears Recently Used.
pub fn execute(a: &Value) -> Result<Value, String> {
    let favorite = a.get("favorite").and_then(Value::as_str);
    let clear = a["clearRecent"] == true;
    if a.get("on").is_some() && favorite.is_none() {
        return Err("on needs favorite, the effect's typeId".into());
    }
    let change = match favorite {
        Some(text) => {
            let id: Uuid = text.parse().map_err(|_| format!("{text} is not an effect typeId"))?;
            if vixen_effect_registry::by_type_id(&id).is_none() {
                return Err(format!("{text} is not a registered effect (see effect_catalog)"));
            }
            let on = a["on"].as_bool().ok_or("favorite needs on: true to pin it or false to unpin it")?;
            Some((id, on))
        }
        None => None,
    };
    with_store(|store| {
        let mut changed = false;
        if let Some((id, on)) = change {
            changed |= store.library.set_favorite(id, on);
        }
        if clear && !store.library.recent.is_empty() {
            store.library.recent.clear();
            changed = true;
        }
        if changed {
            save(store)?;
        }
        let mut out = store.library.to_json();
        out["changed"] = json!(changed);
        Ok(out)
    })
}

pub fn tools() -> Vec<(&'static str, &'static str, Value, Vec<&'static str>, bool)> {
    vec![(
        "effects_library",
        "The sequencer's Effects library above its categories: favorites (pinned with the star on an effect's row, in the order pinned) and recent (effects the user placed in the sequencer window, newest first, favorites left out, at most 5), each {typeId, name}. They belong to the user, not the show, and survive relaunch. With no arguments it only reads. favorite (an effect's typeId from effect_catalog) with on true pins it and on false unpins it, as the star does; clearRecent true empties Recently Used. changed says whether anything changed. Not on Undo: pinning again reverses it. Effects agents add don't count as recently used.",
        json!({"favorite":{"type":"string","minLength":1,"maxLength":64},"on":{"type":"boolean"},"clearRecent":{"type":"boolean"}}),
        vec![],
        false,
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registered(n: usize) -> Uuid {
        vixen_effect_registry::all()[n].type_id
    }

    #[test]
    fn recents_move_to_the_front_and_are_bounded() {
        let mut lib = Library::default();
        for n in 0..(RECENT_KEPT + 3) {
            assert!(lib.used(registered(n)));
        }
        assert_eq!(lib.recent.len(), RECENT_KEPT);
        assert_eq!(lib.recent[0], registered(RECENT_KEPT + 2));
        assert!(!lib.used(registered(RECENT_KEPT + 2)), "the newest again changes nothing");
        assert!(lib.used(registered(RECENT_KEPT)));
        assert_eq!(lib.recent[0], registered(RECENT_KEPT));
        assert_eq!(lib.recent.iter().filter(|r| **r == registered(RECENT_KEPT)).count(), 1);
    }

    #[test]
    fn shown_recents_leave_out_favorites_and_unknown_effects() {
        let mut lib = Library::default();
        lib.used(Uuid::new_v4());
        for n in 0..7 {
            lib.used(registered(n));
        }
        lib.set_favorite(registered(6), true);
        let shown = lib.to_json();
        let recent: Vec<&str> = shown["recent"].as_array().unwrap().iter().map(|e| e["typeId"].as_str().unwrap()).collect();
        assert_eq!(recent.len(), RECENT_SHOWN);
        assert_eq!(recent[0], registered(5).to_string());
        assert!(!recent.contains(&registered(6).to_string().as_str()));
        assert_eq!(shown["favorites"][0]["name"], vixen_effect_registry::all()[6].name);
    }

    #[test]
    fn favorites_keep_pin_order_and_round_trip() {
        let mut lib = Library::default();
        assert!(lib.set_favorite(registered(3), true));
        assert!(lib.set_favorite(registered(1), true));
        assert!(!lib.set_favorite(registered(1), true));
        lib.used(registered(2));
        let again = Library::parse(&lib.to_text());
        assert_eq!(again, lib);
        assert_eq!(again.favorites, vec![registered(3), registered(1)]);
        assert!(again.clone().set_favorite(registered(3), false));
        assert_eq!(Library::parse("not json"), Library::default());
    }

    /// An effect drawn on the timeline (its host's add) becomes the newest recent; a refused add doesn't.
    #[test]
    fn drawing_on_the_timeline_records_a_recent() {
        use crate::sequence::{SequenceSession, SessionEnvironment};
        use crate::timeline::TimelineHost;
        use std::sync::Arc;
        use vixen_core::{ElementTree, TimeSpan};
        let mut tree = ElementTree::new();
        let node = tree.add_element("Light");
        let target = tree.node(node).id;
        let env = SessionEnvironment {
            video_cache: None, tree: Arc::new(tree), store: Arc::new(vixen_io::ModuleStore::new()),
            module_data_root: None, frame_ms: 50, media_dir: std::env::temp_dir(),
        };
        let mut session = SequenceSession::new_untitled(&env);
        let set_level = uuid::uuid!("32cff8e0-5b10-4466-a093-0d232c55aac0");
        let host: &mut dyn TimelineHost = &mut session;
        assert!(host.add_effect(Uuid::new_v4(), target, "Light", TimeSpan::ZERO, TimeSpan::from_ms_i(1000)).is_none());
        assert_eq!(state()["recent"].as_array().unwrap().len(), 0);
        host.add_effect(set_level, target, "Light", TimeSpan::ZERO, TimeSpan::from_ms_i(1000)).unwrap();
        assert_eq!(state()["recent"][0]["typeId"], set_level.to_string());
    }

    /// The command and the window's placements share one store, written to its file, which a fresh read (a relaunch)
    /// finds again.
    #[test]
    fn command_and_placements_persist() {
        let dir = std::env::temp_dir().join(format!("studio-effects-library-cmd-{}", Uuid::new_v4()));
        let path = dir.join(FILE_NAME);
        set_path(path.clone());
        let before = revision();
        let pulse = registered(0).to_string();
        let out = execute(&json!({"favorite": pulse, "on": true})).unwrap();
        assert_eq!(out["changed"], true);
        assert_eq!(out["favorites"][0]["typeId"], pulse);
        assert!(revision() > before);
        assert_eq!(execute(&json!({"favorite": pulse, "on": true})).unwrap()["changed"], false);
        used(registered(4));
        used(registered(4));
        assert!(execute(&json!({"favorite": "nope", "on": true})).is_err());
        assert!(execute(&json!({"favorite": Uuid::new_v4().to_string(), "on": true})).is_err());
        assert!(execute(&json!({"favorite": pulse})).is_err());
        assert!(execute(&json!({"on": true})).is_err());

        let stored = Library::parse(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(stored.favorites, vec![registered(0)]);
        assert_eq!(stored.recent, vec![registered(4)]);
        set_path(path.clone());
        assert_eq!(state()["recent"][0]["typeId"], registered(4).to_string());
        let cleared = execute(&json!({"clearRecent": true})).unwrap();
        assert_eq!(cleared["recent"].as_array().unwrap().len(), 0);
        assert_eq!(cleared["favorites"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
