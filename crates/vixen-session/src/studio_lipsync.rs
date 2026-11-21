//! The LipSync image map library (Tools ▸ LipSync ▸ Edit Image Maps) as live edits on the profile's shared undo history
//! (`studio_display`), shared by the Image Maps sheet and the matching MCP tools (rest of R40).
//!
//! A map lives in two places: its entry in the module store's library, and its pictures in `<module data>/LipSync/<map name>/`.
//! A history step keeps the module store as it was (as every step does) and, beside it, the picture folders the step changed, so
//! Undo puts both back. Pictures are read before an edit and compared after it; only the folders that differ are kept, so a step
//! costs the pictures it touched. A failed edit puts its folders back before the error is returned.
//!
//! The library isn't part of the output graph: an edit publishes without `apply_display`, works while Live Output is on, marks the
//! open sequences modified, renders their LipSync effects again and saves the configuration, as the vanilla sheet's close does.
use crate::app::AppCore;
use crate::marks::lipsync::{PhonemeType, MAP_PHONEMES};
use crate::marks::map_library::{self as maps, EditCommit, LipSyncLibrary, MapperChoice};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use vixen_io::SystemData;

const LOGGER: &str = "StudioLipSync";

/// The commands that publish through [`publish`] instead of rebuilding the display.
pub const COMMANDS: [&str; 6] =
    ["image_map_create", "image_map_rename", "image_map_clone", "image_map_delete", "image_map_update", "image_map_set_default"];

/// The most picture data one step reads; a library past it can't be edited live (it would be kept in memory for Undo).
const PICTURES_LIMIT: u64 = 256 * 1024 * 1024;
/// The largest picture file a map takes.
const PICTURE_FILE_LIMIT: u64 = 32 * 1024 * 1024;

/// The pictures a map on its defaults shows, as the editor writes them on first opening (`map_library`'s F98 pictures).
const DEFAULT_PICTURES: [(PhonemeType, &[u8]); 10] = [
    (PhonemeType::Ai, include_bytes!("../assets/lipsync/AI.png")),
    (PhonemeType::E, include_bytes!("../assets/lipsync/E.png")),
    (PhonemeType::Etc, include_bytes!("../assets/lipsync/ETC.png")),
    (PhonemeType::Fv, include_bytes!("../assets/lipsync/FV.png")),
    (PhonemeType::L, include_bytes!("../assets/lipsync/L.png")),
    (PhonemeType::Mbp, include_bytes!("../assets/lipsync/MBP.png")),
    (PhonemeType::O, include_bytes!("../assets/lipsync/O.png")),
    (PhonemeType::Rest, include_bytes!("../assets/lipsync/REST.png")),
    (PhonemeType::U, include_bytes!("../assets/lipsync/U.png")),
    (PhonemeType::Wq, include_bytes!("../assets/lipsync/WQ.png")),
];

pub fn is_command(command: &str) -> bool {
    COMMANDS.contains(&command)
}

/// The undo history's label, in the words the sheet and Edit ▸ Undo show.
pub fn label(command: &str) -> &'static str {
    match command {
        "image_map_create" => "New Image Map",
        "image_map_rename" => "Rename Image Map",
        "image_map_clone" => "Clone Image Map",
        "image_map_delete" => "Delete Image Map",
        "image_map_update" => "Change Image Map",
        _ => "Set Default Image Map",
    }
}

/// `<module data>/LipSync`.
pub fn lipsync_dir(core: &AppCore) -> PathBuf {
    core.lip_sync_root().join("LipSync")
}

fn phoneme(name: &str) -> Result<PhonemeType, String> {
    MAP_PHONEMES
        .iter()
        .copied()
        .find(|p| p.name().eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("Unknown phoneme “{name}”; use one of {}", phoneme_names().join(", ")))
}

fn phoneme_names() -> Vec<&'static str> {
    MAP_PHONEMES.iter().map(|p| p.name()).collect()
}

fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key].as_str().filter(|s| !s.trim().is_empty() && s.len() <= 256).ok_or(format!("{key} must be a nonempty string of at most 256 bytes"))
}

fn existing<'a>(library: &LipSyncLibrary, args: &'a Value, key: &str) -> Result<&'a str, String> {
    let name = text(args, key)?;
    library.get(name).map(|_| name).ok_or(format!("No image map named “{name}”"))
}

/// A picture file an edit may read: absolute, a regular file, at most 32 MB.
fn picture_path(file: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(file);
    if !path.is_absolute() {
        return Err(format!("Picture paths must be absolute: {file}"));
    }
    match std::fs::metadata(&path) {
        Ok(meta) if meta.is_file() && meta.len() <= PICTURE_FILE_LIMIT => Ok(path),
        Ok(meta) if meta.is_file() => Err(format!("{file} is larger than 32 MB")),
        Ok(_) => Err(format!("{file} isn't a file")),
        Err(e) => Err(format!("Cannot read {file}: {e}")),
    }
}

/// The picture folders as they are: each folder's regular files by name. Links and nested folders aren't read or written.
type Folders = BTreeMap<String, BTreeMap<String, Vec<u8>>>;

fn read_folder(dir: &Path, total: &mut u64) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        if !meta.file_type().is_file() {
            continue;
        }
        *total += meta.len();
        if *total > PICTURES_LIMIT {
            return Err("The LipSync picture folders are larger than 256 MB, too large to edit with Undo".into());
        }
        files.insert(entry.file_name().to_string_lossy().into_owned(), std::fs::read(entry.path()).map_err(|e| e.to_string())?);
    }
    Ok(files)
}

fn read_folders(lipsync: &Path) -> Result<Folders, String> {
    let mut folders = Folders::new();
    let Ok(entries) = std::fs::read_dir(lipsync) else { return Ok(folders) };
    let mut total = 0;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // `map_library`'s case-only rename passes through `.rename-…`; it never outlives the edit.
        if name.starts_with('.') || !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        folders.insert(name, read_folder(&entry.path(), &mut total)?);
    }
    Ok(folders)
}

/// The picture folders a step changed, as they were before it (`None`: the folder wasn't there).
#[derive(Clone, Debug, Default)]
pub struct Pictures(Vec<(String, Option<BTreeMap<String, Vec<u8>>>)>);

impl Pictures {
    fn changed(before: &Folders, after: &Folders) -> Pictures {
        let names: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
        Pictures(names.into_iter().filter(|n| before.get(*n) != after.get(*n)).map(|n| (n.clone(), before.get(n).cloned())).collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Puts these folders back, and returns them as they were just before, for the opposite step.
    pub fn restore(&self, lipsync: &Path) -> Pictures {
        let mut inverse = Vec::new();
        for (name, files) in &self.0 {
            if maps::name_is_unsafe(name) || name.starts_with('.') {
                continue;
            }
            let dir = lipsync.join(name);
            let is_dir = std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir());
            let current = if is_dir { read_folder(&dir, &mut 0).ok() } else { None };
            if let Some(current) = &current {
                for file in current.keys() {
                    if let Err(e) = std::fs::remove_file(dir.join(file)) {
                        crate::logging::error(LOGGER, &format!("Could not remove {}", dir.join(file).display()), Some(&e.to_string()));
                    }
                }
            }
            match files {
                Some(files) => {
                    if let Err(e) = std::fs::create_dir_all(&dir) {
                        crate::logging::error(LOGGER, &format!("Could not create {}", dir.display()), Some(&e.to_string()));
                    }
                    for (file, bytes) in files {
                        if let Err(e) = std::fs::write(dir.join(file), bytes) {
                            crate::logging::error(LOGGER, &format!("Could not restore {}", dir.join(file).display()), Some(&e.to_string()));
                        }
                    }
                }
                // Only an emptied folder goes: anything else in it stays.
                None if is_dir => {
                    let _ = std::fs::remove_dir(&dir);
                }
                None => {}
            }
            inverse.push((name.clone(), current));
        }
        Pictures(inverse)
    }
}

/// One edit on `system`'s library and the picture folders under `lipsync`: the result, whether anything changed, and the pictures
/// it changed as they were. On an error the pictures are put back.
pub fn edit(system: &mut SystemData, lipsync: &Path, command: &str, args: &Value) -> Result<(Value, bool, Pictures), String> {
    let before = read_folders(lipsync)?;
    let root = lipsync.parent().ok_or("Invalid LipSync folder")?;
    let outcome = apply(system, root, command, args);
    let after = read_folders(lipsync).unwrap_or_default();
    let pictures = Pictures::changed(&before, &after);
    match outcome {
        Ok((value, changed)) => {
            let changed = changed || !pictures.is_empty();
            Ok((value, changed, pictures))
        }
        Err(e) => {
            pictures.restore(lipsync);
            Err(e)
        }
    }
}

fn apply(system: &mut SystemData, root: &Path, command: &str, args: &Value) -> Result<(Value, bool), String> {
    let mut library = LipSyncLibrary::from_store(&system.module_store);
    let (value, changed) = match command {
        "image_map_create" => {
            let made = library.new_map(root);
            library.keep_new();
            let name = match args.get("name") {
                Some(_) => {
                    let wanted = text(args, "name")?;
                    if maps::name_is_unsafe(wanted) {
                        return Err(maps::BAD_NAME_MESSAGE.into());
                    }
                    library.rename_map(&made, wanted, root)?.unwrap_or(made)
                }
                None => made,
            };
            (json!({"name": name}), true)
        }
        "image_map_rename" => {
            let old = existing(&library, args, "name")?.to_string();
            let new = text(args, "newName")?.trim();
            match library.rename_map(&old, new, root)? {
                Some(name) => (json!({"name": name}), true),
                None => (json!({"name": old}), false),
            }
        }
        "image_map_clone" => {
            let name = existing(&library, args, "name")?.to_string();
            let made = library.clone_map(&name, root).ok_or(format!("No image map named “{name}”"))?;
            if let Some(error) = made.error {
                return Err(error);
            }
            (json!({"name": made.name}), true)
        }
        "image_map_delete" => {
            if args["confirm"] != true {
                return Err("Deleting image maps requires confirm: true".into());
            }
            let names: Vec<String> = args["names"].as_array().into_iter().flatten().filter_map(|n| n.as_str().map(str::to_string)).collect();
            if names.is_empty() {
                return Err("names must list at least one image map".into());
            }
            if let Some(missing) = names.iter().find(|n| library.get(n).is_none()) {
                return Err(format!("No image map named “{missing}”"));
            }
            let distinct: std::collections::BTreeSet<&String> = names.iter().collect();
            if distinct.len() >= library.len() {
                return Err("The library keeps at least one image map".into());
            }
            library.remove_maps(&names, root)?;
            (json!({"deleted": distinct.len()}), true)
        }
        "image_map_update" => {
            let name = existing(&library, args, "name")?.to_string();
            let notes = args.get("notes").map(|n| n.as_str().filter(|s| s.len() <= 4096).ok_or("notes must be a string of at most 4096 bytes")).transpose()?;
            let pictures = match args.get("pictures") {
                Some(p) => p.as_object().ok_or("pictures must map phonemes to a file path or null")?.clone(),
                None => Default::default(),
            };
            if notes.is_none() && pictures.is_empty() {
                return Err("Give notes or pictures to change".into());
            }
            let mut changes = Vec::new();
            for (key, file) in &pictures {
                let phoneme = phoneme(key)?;
                let file = match file {
                    Value::Null => None,
                    Value::String(s) => Some(picture_path(s)?),
                    _ => return Err(format!("pictures.{key} must be a file path or null")),
                };
                changes.push((phoneme, file));
            }
            let unchanged_notes = notes.is_some_and(|n| library.get(&name).is_some_and(|m| m.notes == n && !m.using_defaults));
            if changes.is_empty() && unchanged_notes {
                return Ok((json!({"name": name}), false));
            }
            let mut map = library.begin_edit(&name, root)?;
            for (phoneme, file) in changes {
                match file {
                    Some(file) => {
                        if !map.set_image(phoneme, &file) {
                            return Err(format!("{} isn't a picture Vixen can read", file.display()));
                        }
                    }
                    None => map.clear_image(phoneme),
                }
            }
            if let Some(notes) = notes {
                map.notes = notes.to_string();
            }
            match library.commit_edit(&map, None, root) {
                EditCommit::Saved { name, alert: None } => (json!({"name": name}), true),
                EditCommit::Saved { alert: Some(alert), .. } => return Err(alert),
                EditCommit::NeedsOverwriteAnswer(name) => return Err(format!("Another map is named “{name}”")),
            }
        }
        "image_map_set_default" => {
            let name = existing(&library, args, "name")?.to_string();
            library.set_default(&name);
            (json!({"name": name}), library.changed())
        }
        _ => return Err("Unknown image map command".into()),
    };
    if changed {
        library.write_to(&mut system.module_store);
    }
    Ok((value, changed))
}

/// Puts `system` in place after an image map step or its Undo: the open sequences are modified and their LipSync effects render
/// again, and the configuration is saved (the pictures are already on disk).
pub fn publish(core: &AppCore, system: SystemData) {
    let library = LipSyncLibrary::from_store(&system.module_store);
    *core.system.write() = system;
    core.display.studio_bump();
    for session in core.open_sequence_sessions() {
        let mut session = session.lock();
        session.set_modified();
        session.reset_lip_sync_effects(&library);
    }
    for result in [core.save_system_config(), core.save_module_config()] {
        if let Err(e) = result {
            crate::logging::error(LOGGER, "Could not save the image maps.", Some(&e.to_string()));
        }
    }
}

/// The phonemes a map shows a picture for: every one while it is on its defaults, otherwise those with a file.
fn mapped(lipsync: &Path, name: &str, using_defaults: bool) -> Vec<&'static str> {
    MAP_PHONEMES
        .iter()
        .filter(|p| using_defaults || picture_file(lipsync, name, **p).is_some())
        .map(|p| p.name())
        .collect()
}

fn picture_file(lipsync: &Path, name: &str, phoneme: PhonemeType) -> Option<PathBuf> {
    if maps::name_is_unsafe(name) || name.trim().is_empty() {
        return None;
    }
    let file = lipsync.join(name).join(format!("{}.bmp", phoneme.name()));
    std::fs::symlink_metadata(&file).is_ok_and(|m| m.is_file()).then_some(file)
}

/// The library as the sheet and `image_map_list` show it: `{revision, phonemes, maps: [{name, notes, isDefault, editable,
/// usingDefaults, pictures}]}`, maps sorted by name.
pub fn describe(system: &SystemData, lipsync: &Path, revision: u64) -> Value {
    let library = LipSyncLibrary::from_store(&system.module_store);
    let maps: Vec<Value> = library
        .list()
        .into_iter()
        .filter_map(|row| {
            let map = library.get(&row.name)?;
            Some(json!({
                "name": row.name,
                "notes": if map.using_defaults { "" } else { row.notes.as_str() },
                "isDefault": row.is_default,
                "editable": map.is_matrix,
                "usingDefaults": map.using_defaults,
                "pictures": if map.is_matrix { mapped(lipsync, &row.name, map.using_defaults) } else { vec![] },
            }))
        })
        .collect();
    json!({"revision": revision, "phonemes": phoneme_names(), "maps": maps})
}

pub fn list(core: &AppCore) -> Value {
    describe(&core.system.read(), &lipsync_dir(core), core.display.revision())
}

/// A map's picture for one phoneme, as stored (PNG, or BMP from Windows Vixen). A map on its defaults shows the bundled one.
pub fn picture(core: &AppCore, name: &str, phoneme_name: &str) -> Option<Vec<u8>> {
    let phoneme = phoneme(phoneme_name).ok()?;
    let using_defaults = LipSyncLibrary::from_store(&core.system.read().module_store).get(name)?.using_defaults;
    if using_defaults {
        return DEFAULT_PICTURES.iter().find(|(p, _)| *p == phoneme).map(|(_, png)| png.to_vec());
    }
    std::fs::read(picture_file(&lipsync_dir(core), name, phoneme)?).ok()
}

/// The Image Mapper's first choices for `file_count` files, one per phoneme in the navigator's order (F102).
pub fn mapper_choices(current: &str, file_count: usize, mapped: &[String]) -> Vec<MapperChoice> {
    let current = phoneme(current).unwrap_or(MAP_PHONEMES[0]);
    let mapped: Vec<PhonemeType> = mapped.iter().filter_map(|m| phoneme(m).ok()).collect();
    maps::mapper_initial(current, file_count, &mapped)
}

pub fn tools() -> Vec<Value> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let boolean = json!({"type":"boolean"});
    let path = json!({"type":["string","null"],"minLength":1,"maxLength":1024});
    let phonemes: serde_json::Map<String, Value> = phoneme_names().into_iter().map(|p| (p.to_string(), path.clone())).collect();
    vec![
        ("image_map_create","Add a LipSync image map (Tools > LipSync > Edit Image Maps), on the default mouth pictures until changed. name defaults to \"New Map\" made unique. One undo step on the shared history.",json!({"name":string}),vec![],false),
        ("image_map_rename","Rename an image map; its picture folder moves with it. A taken name gets a number.",json!({"name":string,"newName":string}),vec!["name","newName"],false),
        ("image_map_clone","Copy an image map and its pictures under the same name made unique.",json!({"name":string}),vec!["name"],false),
        ("image_map_delete","Delete image maps and their pictures (confirm). LipSync effects using one fall back to the default map. The library keeps at least one map. Undo brings them back.",json!({"names":{"type":"array","items":string,"minItems":1,"maxItems":100},"confirm":boolean}),vec!["names","confirm"],true),
        ("image_map_update","Change an image map's notes and/or pictures. pictures maps a phoneme (AI, E, ETC, FV, L, MBP, O, REST, U, WQ) to an absolute path of a .png, .jpg or .bmp file (read only, at most 32 MB), or null to clear it. A map on its defaults gets the default pictures first. One undo step.",json!({"name":string,"notes":{"type":"string","maxLength":4096},"pictures":{"type":"object","properties":phonemes,"additionalProperties":false}}),vec!["name"],false),
        ("image_map_set_default","Make an image map the default, which LipSync effects without a map of their own use.",json!({"name":string}),vec!["name"],false),
    ].into_iter().map(|(name,description,mut properties,required,destructive)|{properties["expectedRevision"]=json!({"type":"integer","minimum":0});json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":destructive,"openWorldHint":false}})}).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use uuid::Uuid;

    fn png(shade: u8) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([shade, shade, shade, 255])).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn exec(core: &AppCore, command: &str, args: Value) -> Value {
        core.studio_display.execute(core, command, &args, false).unwrap_or_else(|e| panic!("{command}: {e}"))
    }

    fn names(core: &AppCore) -> Vec<String> {
        list(core)["maps"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn edits_undo_with_their_pictures_on_the_shared_history() {
        let root = std::env::temp_dir().join(format!("studio-lipsync-{}", Uuid::new_v4().simple()));
        let core = Arc::new(AppCore::open(root.join("profile"), true).unwrap());
        let files = root.join("files");
        std::fs::create_dir_all(&files).unwrap();
        let (dark, light) = (files.join("dark.png"), files.join("light.png"));
        std::fs::write(&dark, png(10)).unwrap();
        std::fs::write(&light, png(240)).unwrap();
        let lipsync = lipsync_dir(&core);
        let start = names(&core);

        let made = exec(&core, "image_map_create", json!({"name": "Singer"}))["result"]["name"].as_str().unwrap().to_string();
        assert_eq!(made, "Singer");
        exec(&core, "image_map_update", json!({"name": "Singer", "notes": "Lead", "pictures": {"AI": dark.to_string_lossy(), "REST": light.to_string_lossy()}}));
        let rest = lipsync.join("Singer/REST.bmp");
        let first_rest = std::fs::read(&rest).unwrap();
        exec(&core, "image_map_update", json!({"name": "Singer", "pictures": {"REST": dark.to_string_lossy(), "O": null}}));
        assert_ne!(std::fs::read(&rest).unwrap(), first_rest);
        assert!(!lipsync.join("Singer/O.bmp").exists());
        exec(&core, "image_map_rename", json!({"name": "Singer", "newName": "Lead Singer"}));
        assert!(lipsync.join("Lead Singer/REST.bmp").exists() && !lipsync.join("Singer").exists());
        exec(&core, "image_map_clone", json!({"name": "Lead Singer"}));
        exec(&core, "image_map_set_default", json!({"name": "Lead Singer"}));
        let listed = list(&core);
        let lead = listed["maps"].as_array().unwrap().iter().find(|m| m["name"] == "Lead Singer").unwrap();
        assert_eq!(lead["isDefault"], true);
        assert_eq!(lead["notes"], "Lead");
        assert!(!lead["pictures"].as_array().unwrap().contains(&json!("O")));
        assert_eq!(picture(&core, "Lead Singer", "REST").unwrap(), std::fs::read(lipsync.join("Lead Singer/REST.bmp")).unwrap());
        assert_eq!(
            core.studio_display.history()["undo"],
            json!(["Set Default Image Map", "Clone Image Map", "Rename Image Map", "Change Image Map", "Change Image Map", "New Image Map"])
        );
        // The configuration is saved with each step.
        let saved = std::fs::read_to_string(core.paths.module_store_file()).unwrap();
        assert!(saved.contains("Lead Singer"));

        // Undo walks back through the pictures as well as the library.
        exec(&core, "display_undo", json!({}));
        assert_eq!(names(&core).iter().filter(|n| n.starts_with("Lead Singer")).count(), 2);
        exec(&core, "display_undo", json!({}));
        assert_eq!(names(&core).iter().filter(|n| n.starts_with("Lead Singer")).count(), 1, "Undo takes the clone away");
        exec(&core, "display_undo", json!({}));
        assert!(lipsync.join("Singer/REST.bmp").exists() && !lipsync.join("Lead Singer").exists());
        exec(&core, "display_undo", json!({}));
        assert_eq!(std::fs::read(lipsync.join("Singer/REST.bmp")).unwrap(), first_rest);
        assert!(lipsync.join("Singer/O.bmp").exists(), "the cleared default picture comes back");
        exec(&core, "display_redo", json!({}));
        assert!(!lipsync.join("Singer/O.bmp").exists());
        exec(&core, "display_undo", json!({}));
        exec(&core, "display_undo", json!({}));
        exec(&core, "display_undo", json!({}));
        assert_eq!(names(&core), start);
        assert!(!lipsync.join("Singer").exists(), "Undo of New takes away the folder its pictures made");

        // Refusals change nothing.
        let all: Vec<Value> = names(&core).into_iter().map(Value::from).collect();
        let fail = |command: &str, args: Value| core.studio_display.execute(&core, command, &args, true).unwrap_err();
        assert!(fail("image_map_delete", json!({"names": all, "confirm": true})).contains("at least one"));
        assert!(fail("image_map_update", json!({"name": start[0], "pictures": {"AI": "dark.png"}})).contains("absolute"));
        assert!(fail("image_map_update", json!({"name": start[0], "pictures": {"XX": dark.to_string_lossy()}})).contains("XX"));
        assert!(fail("image_map_rename", json!({"name": start[0], "newName": "../x"})).contains("Can not set new name"));
        let text = files.join("words.png");
        std::fs::write(&text, "not a picture").unwrap();
        assert!(fail("image_map_update", json!({"name": start[0], "pictures": {"AI": text.to_string_lossy()}})).contains("isn't a picture"));
        assert!(!lipsync.join(&start[0]).join("AI.bmp").exists(), "a failed edit puts the pictures back");
        assert!(core.studio_display.history()["undo"].as_array().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delete_and_its_undo_restore_the_map_and_its_folder() {
        let root = std::env::temp_dir().join(format!("studio-lipsync-delete-{}", Uuid::new_v4().simple()));
        let core = Arc::new(AppCore::open(root.join("profile"), true).unwrap());
        let picture_file = root.join("p.png");
        std::fs::write(&picture_file, png(90)).unwrap();
        exec(&core, "image_map_create", json!({"name": "Tree"}));
        exec(&core, "image_map_update", json!({"name": "Tree", "pictures": {"MBP": picture_file.to_string_lossy()}}));
        let folder = lipsync_dir(&core).join("Tree");
        let before = read_folder(&folder, &mut 0).unwrap();
        assert!(core.studio_display.execute(&core, "image_map_delete", &json!({"names": ["Tree"]}), true).unwrap_err().contains("confirm"));
        exec(&core, "image_map_delete", json!({"names": ["Tree"], "confirm": true}));
        assert!(!folder.exists() && !names(&core).contains(&"Tree".to_string()));
        assert_eq!(core.studio_display.history()["undoPreviewOnly"], true, "an image map step undoes with Live Output on");
        exec(&core, "display_undo", json!({}));
        assert!(names(&core).contains(&"Tree".to_string()));
        assert_eq!(read_folder(&folder, &mut 0).unwrap(), before);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mapper_choices_follow_the_current_phoneme() {
        let choices = mapper_choices("O", 2, &["AI".into()]);
        assert_eq!(choices.len(), MAP_PHONEMES.len());
        let o = MAP_PHONEMES.iter().position(|p| *p == PhonemeType::O).unwrap();
        assert_eq!(choices[o], MapperChoice::File(0));
        assert_eq!(choices[o + 1], MapperChoice::File(1));
    }
}
