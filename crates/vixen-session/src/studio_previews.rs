//! The show window's Previews pane and the matching MCP tools: add, rename, duplicate, delete and turn previews on or off
//! as live edits on the profile's shared undo history (`studio_display`), plus the pane's list and layout thumbnails.
//!
//! A preview edit changes only the preview list, its module data and `DisabledDevices`, none of which the output graph is
//! built from, so it publishes without `apply_display`: nothing playing stops and no controller restarts, and it works
//! while Live Output is on. "On" is the stored setting (`DisabledDevices`): an on preview shows whenever Live Output is on.
use crate::app::AppCore;
use serde_json::{json, Value};
use std::collections::HashSet;
use uuid::Uuid;
use vixen_io::system::Preview;
use vixen_io::{ModuleEntry, SystemData};
use vixen_preview::{PreviewData, Scene, VIXEN_PREVIEW_MODULE_TYPE};
use vixen_xml::XNode;

/// The commands that publish through [`publish`] instead of rebuilding the display.
pub const COMMANDS: [&str; 6] = [
    "preview_create",
    "preview_rename",
    "preview_duplicate",
    "preview_delete",
    "preview_enable",
    "preview_location_offset",
];

pub fn is_command(command: &str) -> bool {
    COMMANDS.contains(&command)
}

/// The undo history's label, in the words the pane and Edit ▸ Undo show.
pub fn label(command: &str, args: &Value) -> &'static str {
    match command {
        "preview_create" => "Add Preview",
        "preview_rename" => "Rename Preview",
        "preview_duplicate" => "Duplicate Preview",
        "preview_delete" => "Delete Preview",
        "preview_location_offset" => "Set Location Offset",
        _ if args["enabled"] == true => "Turn On Preview",
        _ => "Turn Off Preview",
    }
}

fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or(format!("{key} must be a nonempty string of at most 256 bytes"))
}

fn preview_id(system: &SystemData, args: &Value) -> Result<Uuid, String> {
    let id = Uuid::parse_str(text(args, "previewId")?).map_err(|_| "previewId must be a UUID")?;
    system
        .previews
        .iter()
        .any(|p| p.instance_id == id)
        .then_some(id)
        .ok_or("Preview not found".into())
}

/// `base`, or `base 2`, `base 3`… so a new preview never repeats a name.
fn unique_name(system: &SystemData, base: &str) -> String {
    let taken: HashSet<&str> = system.previews.iter().map(|p| p.name.as_str()).collect();
    if !taken.contains(base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base} {n}")).find(|n| !taken.contains(n.as_str())).unwrap()
}

/// One preview edit on `system`. Returns the result and whether anything changed.
pub fn edit(system: &mut SystemData, command: &str, args: &Value) -> Result<(Value, bool), String> {
    match command {
        "preview_create" => {
            let name = match args.get("name") {
                Some(_) => text(args, "name")?.to_string(),
                None => unique_name(system, "New Preview"),
            };
            let id = Uuid::new_v4();
            let preview = Preview { name: name.clone(), type_id: VIXEN_PREVIEW_MODULE_TYPE, instance_id: id };
            system.previews.push(preview);
            let mut data = PreviewData::new();
            data.module_instance_id = Some(id.to_string());
            data.module_type_id = Some(VIXEN_PREVIEW_MODULE_TYPE.to_string());
            let entry = crate::preview::PreviewEntry { id, name: name.clone(), type_id: VIXEN_PREVIEW_MODULE_TYPE };
            crate::preview_design::save_preview_data(system, &entry, &data);
            Ok((json!({"previewId":id.to_string(),"name":name}), true))
        }
        "preview_rename" => {
            let id = preview_id(system, args)?;
            let name = text(args, "name")?;
            let preview = system.previews.iter_mut().find(|p| p.instance_id == id).unwrap();
            let changed = preview.name != name;
            preview.name = name.to_string();
            Ok((json!({"previewId":id.to_string(),"name":name}), changed))
        }
        "preview_duplicate" => {
            let id = preview_id(system, args)?;
            let original = system.previews.iter().find(|p| p.instance_id == id).cloned().unwrap();
            let new_id = Uuid::new_v4();
            // The layout comes along under the new id, as Setup Previews' Duplicate Selected copies it.
            if let Some(entry) = system.module_store.instance(original.type_id, original.instance_id) {
                let mut element = entry.element.clone();
                element.set_attr("moduleInstance", new_id.to_string());
                if let Some(data) = element.elements_mut().next() {
                    if let Some(member) = data.child_mut("ModuleInstanceId") {
                        member.children = vec![XNode::Text(new_id.to_string())];
                    }
                }
                system.module_store.set_instance(ModuleEntry { element });
            }
            let name = match args.get("name") {
                Some(_) => text(args, "name")?.to_string(),
                None => unique_name(system, &format!("{} copy", original.name)),
            };
            let index = system.previews.iter().position(|p| p.instance_id == id).unwrap() + 1;
            system.previews.insert(index, Preview { name: name.clone(), type_id: original.type_id, instance_id: new_id });
            if system.disabled_devices.contains(&id) {
                system.disabled_devices.push(new_id);
            }
            Ok((json!({"previewId":new_id.to_string(),"name":name}), true))
        }
        "preview_delete" => {
            if args["confirm"] != true {
                return Err("Explicit confirm:true is required".into());
            }
            let id = preview_id(system, args)?;
            let type_id = system.previews.iter().find(|p| p.instance_id == id).unwrap().type_id;
            system.previews.retain(|p| p.instance_id != id);
            system.module_store.remove_instance(type_id, id);
            system.disabled_devices.retain(|d| *d != id);
            Ok((json!({"previewId":id.to_string()}), true))
        }
        "preview_enable" => {
            let id = preview_id(system, args)?;
            let on = args["enabled"].as_bool().ok_or("enabled must be true or false")?;
            let was = !system.disabled_devices.contains(&id);
            if on {
                system.disabled_devices.retain(|d| *d != id);
            } else if was {
                system.disabled_devices.push(id);
            }
            Ok((json!({"previewId":id.to_string(),"enabled":on}), was != on))
        }
        "preview_location_offset" => {
            // The designer's Location Offset: added to every linked light's preview position when locations are saved.
            let id = preview_id(system, args)?;
            let coordinate = |key: &str| {
                args[key]
                    .as_i64()
                    .filter(|n| n.abs() <= 100_000)
                    .map(|n| n as f64)
                    .ok_or(format!("{key} must be a whole number between -100000 and 100000"))
            };
            let (x, y) = (coordinate("x")?, coordinate("y")?);
            let entry = crate::preview::preview_entries(system).into_iter().find(|p| p.id == id).ok_or("Preview not found")?;
            let mut data = crate::preview::load_preview_data(&system.module_store, &entry).map_err(|e| e.to_string())?;
            let changed = data.location_offset.x.value != x || data.location_offset.y.value != y;
            if changed {
                let mut design = vixen_preview::DesignSession::new(data, None);
                design.studio_set_location_offset(x, y);
                data = design.into_data();
                crate::preview_design::save_preview_data(system, &entry, &data);
            }
            Ok((json!({"previewId":id.to_string(),"x":x,"y":y}), changed))
        }
        _ => Err("Unknown preview command".into()),
    }
}

/// Swaps in `system` after a preview-only change, without rebuilding the output graph. Previews keep running or stopped as they
/// were, a preview just turned on or added starts if Live Output is on, and one turned off or deleted stops.
pub fn publish(core: &AppCore, system: SystemData) {
    let devices = core.playback.preview_devices();
    let running: HashSet<Uuid> = devices.running().into_iter().collect();
    let (known, was_on): (HashSet<Uuid>, HashSet<Uuid>) = {
        let old = core.system.read();
        (
            old.previews.iter().map(|p| p.instance_id).collect(),
            old.previews.iter().map(|p| p.instance_id).filter(|id| !old.disabled_devices.contains(id)).collect(),
        )
    };
    let start: Vec<Uuid> = system
        .previews
        .iter()
        .map(|p| p.instance_id)
        .filter(|id| !system.disabled_devices.contains(id))
        .filter(|id| running.contains(id) || !known.contains(id) || !was_on.contains(id))
        .collect();
    let ids: Vec<Uuid> = system.previews.iter().map(|p| p.instance_id).collect();
    devices.configure(&system.previews, &system.disabled_devices);
    *core.system.write() = system;
    core.display.studio_bump();
    // configure() stops every preview and tells no one; each call here notifies the preview windows. A start does nothing
    // while the engine is closed.
    for id in &ids {
        devices.set_running(id, start.contains(id));
    }
    if ids.is_empty() {
        devices.set_running(&Uuid::nil(), false);
    }
}

/// Whether the designer changed the layout, so closing an untouched designer adds no undo step. The light locations it writes
/// back with the layout (`SaveLocations`) are derived from it and refreshed on every close, so they don't count on their own.
pub fn design_changes(system: &SystemData, entry: &crate::preview::PreviewEntry, data: &PreviewData) -> bool {
    // Read back the way the designer reads it, so a show written by Windows Vixen compares equal when nothing was touched.
    crate::preview::load_preview_data(&system.module_store, entry).ok().map(|d| d.to_xml()) != Some(data.to_xml())
}

/// The pane's rows: every preview in system order, whether it is on and showing now, and its size.
pub fn list(core: &AppCore) -> Value {
    let devices = core.playback.preview_devices();
    let system = core.system.read();
    let items: Vec<Value> = crate::preview::preview_entries(&system)
        .iter()
        .map(|entry| {
            let data = crate::preview::load_preview_data(&system.module_store, entry).ok();
            let (lights, linked, props) = data.as_ref().map_or((0, 0, 0), |data| {
                let scene = Scene::build(data, Some(&system.tree));
                (scene.lights.len(), scene.linked_pixel_count(&system.tree), data.display_items.len())
            });
            json!({
                "id": entry.id.to_string(),
                "name": entry.name,
                "title": title(&entry.name),
                "enabled": !system.disabled_devices.contains(&entry.id),
                "running": devices.is_running(&entry.id),
                "props": props,
                "lights": lights,
                "linkedLights": linked,
                "camera": data.as_ref().is_some_and(|d| d.use_open_gl),
            })
        })
        .collect();
    json!({"revision": core.display.revision(), "liveOutput": core.execution().is_open(), "items": items})
}

/// A preview's name for its window title: the "(OpenGL)" or "(GDI)" renderer tag older shows carry in the name is left off.
pub fn title(name: &str) -> String {
    let mut trimmed = name.trim_end();
    for tag in ["(opengl)", "(gdi)"] {
        if trimmed.to_ascii_lowercase().ends_with(tag) {
            trimmed = trimmed[..trimmed.len() - tag.len()].trim_end().trim_end_matches(['-', '–']).trim_end();
        }
    }
    if trimmed.is_empty() { "Preview".to_string() } else { trimmed.to_string() }
}

/// A PNG of the preview's layout no larger than `max_width` × `max_height`: its background image as the preview dims it, and
/// every light in its colour (white for full-colour lights), so the pane's thumbnails follow every layout change.
pub fn thumbnail(core: &AppCore, id: Uuid, max_width: u32, max_height: u32) -> Result<Vec<u8>, String> {
    let (lights, background, alpha) = {
        let system = core.system.read();
        let entry = crate::preview::preview_entries(&system)
            .into_iter()
            .find(|p| p.id == id)
            .ok_or("Preview not found")?;
        let data = crate::preview::load_preview_data(&system.module_store, &entry).map_err(|e| e.to_string())?;
        let scene = Scene::build(&data, Some(&system.tree));
        let resolved = scene.resolve_nodes(&system.tree);
        let lights: Vec<(f32, f32, f32, [u8; 3])> = scene
            .lights
            .iter()
            .zip(&scene.links)
            .map(|(light, link)| {
                let color = match resolved.get(link.node as usize).copied().flatten() {
                    Some(node) if link.color_count > 1 || system.tree.is_discrete(node) => system
                        .tree
                        .valid_colors(node, false)
                        .get(link.color_slot as usize)
                        .map_or([255, 255, 255], |c| [c.r, c.g, c.b]),
                    Some(_) => [255, 255, 255],
                    None => [110, 110, 110],
                };
                (light.x, light.y, light.size, color)
            })
            .collect();
        let background = data
            .background_file_name
            .as_deref()
            .filter(|n| !n.is_empty())
            .map(|n| crate::preview::background_path(&core.paths, n));
        (lights, background, data.background_alpha.clamp(0, 255) as u8)
    };
    let background = background.and_then(|path| image::open(path).ok()).map(|i| i.to_rgba8());
    // Framed on what the preview shows, its background and its lights, not the empty canvas around them.
    let mut frame: Option<(f32, f32, f32, f32)> = background.as_ref().map(|b| (0.0, 0.0, b.width() as f32, b.height() as f32));
    for &(x, y, size, _) in &lights {
        let r = size / 2.0;
        let (x0, y0, x1, y1) = frame.unwrap_or((x - r, y - r, x + r, y + r));
        frame = Some((x0.min(x - r), y0.min(y - r), x1.max(x + r), y1.max(y + r)));
    }
    let (x0, y0, x1, y1) = frame.unwrap_or((0.0, 0.0, 160.0, 100.0));
    let pad = if background.is_some() { 0.0 } else { (x1 - x0).max(y1 - y0) * 0.06 + 4.0 };
    let (x0, y0) = (x0 - pad, y0 - pad);
    let (width, height) = ((x1 + pad - x0).max(16.0), (y1 + pad - y0).max(10.0));
    let scale = (max_width.max(16) as f32 / width).min(max_height.max(16) as f32 / height).min(4.0);
    let (w, h) = (((width * scale).round() as u32).max(1), ((height * scale).round() as u32).max(1));
    let mut image = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]));
    if let Some(background) = background {
        let (sw, sh) = (((background.width() as f32) * scale).round().max(1.0) as u32, ((background.height() as f32) * scale).round().max(1.0) as u32);
        let small = image::imageops::resize(&background, sw, sh, image::imageops::FilterType::Triangle);
        let (ox, oy) = ((-x0 * scale).round() as i64, (-y0 * scale).round() as i64);
        for (x, y, pixel) in small.enumerate_pixels() {
            let (tx, ty) = (i64::from(x) + ox, i64::from(y) + oy);
            if (0..i64::from(w)).contains(&tx) && (0..i64::from(h)).contains(&ty) {
                let a = u32::from(pixel[3]) * u32::from(alpha) / 255;
                let target = image.get_pixel_mut(tx as u32, ty as u32);
                for i in 0..3 {
                    target[i] = (u32::from(pixel[i]) * a / 255) as u8;
                }
            }
        }
    }
    for (x, y, size, [r, g, b]) in lights {
        let radius = (size / 2.0 * scale).max(0.6);
        crate::studio_preview::paint_disc(&mut image, w, h, (x - x0) * scale, (y - y0) * scale, radius, |pixel, _| {
            pixel[0] = r;
            pixel[1] = g;
            pixel[2] = b;
        });
    }
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(png.into_inner())
}

pub fn tools() -> Vec<Value> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    vec![
        ("preview_create","Add an empty preview, as the Previews pane's Add button does. name defaults to \"New Preview\" (numbered when taken). It is on, so it shows whenever Live Output is on. Add props with preview_prop_create.",json!({"name":string}),vec![],false),
        ("preview_rename","Rename a preview. Its window title follows; an \"(OpenGL)\" tag at the end of the name is left out of the title.",json!({"previewId":string,"name":string}),vec!["previewId","name"],false),
        ("preview_duplicate","Duplicate a preview with its whole layout and links, inserted after it. name defaults to \"<name> copy\".",json!({"previewId":string,"name":string}),vec!["previewId"],false),
        ("preview_delete","Delete a preview and its layout. Undo brings it back. Requires explicit confirmation.",json!({"previewId":string,"confirm":{"type":"boolean"}}),vec!["previewId","confirm"],true),
        ("preview_location_offset","Set a preview's location offset, the designer's Location Offset: x and y are added to every linked light's preview position when light locations are written (element_location, the designer's Save Locations). One undo step; the same values add none.",json!({"previewId":string,"x":{"type":"integer","minimum":-100000,"maximum":100000},"y":{"type":"integer","minimum":-100000,"maximum":100000}}),vec!["previewId","x","y"],false),
        ("preview_enable","Turn a preview on or off: an on preview shows in its own window whenever Live Output is on (saved as the profile's disabled devices). Takes effect at once, without restarting controllers.",json!({"previewId":string,"enabled":{"type":"boolean"}}),vec!["previewId","enabled"],false),
    ].into_iter().map(|(name,description,mut properties,required,destructive)|{properties["expectedRevision"]=json!({"type":"integer","minimum":0});json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":destructive,"openWorldHint":false}})}).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_leave_out_the_renderer_tag() {
        assert_eq!(title("Christmas Tree 2022 (OpenGL)"), "Christmas Tree 2022");
        assert_eq!(title("Yard - (GDI)"), "Yard");
        assert_eq!(title("(OpenGL)"), "Preview");
        assert_eq!(title("Front Yard"), "Front Yard");
    }

    fn core() -> (AppCore, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("studio-previews-{}", Uuid::new_v4()));
        (AppCore::open(root.clone(), true).unwrap(), root)
    }

    fn run(core: &AppCore, command: &str, args: Value) -> Value {
        core.studio_display.execute(core, command, &args, false).unwrap()
    }

    #[test]
    fn pane_edits_are_live_and_undoable() {
        let (core, root) = core();
        let before = core.display.revision();
        let made = run(&core, "preview_create", json!({}));
        let id = made["result"]["previewId"].as_str().unwrap().to_string();
        assert_eq!(made["result"]["name"], "New Preview");
        assert!(core.display.revision() > before);
        assert_eq!(run(&core, "preview_create", json!({}))["result"]["name"], "New Preview 2");
        run(&core, "preview_rename", json!({"previewId":id,"name":"Front Yard (OpenGL)"}));
        let copy = run(&core, "preview_duplicate", json!({"previewId":id}));
        let copy_id = Uuid::parse_str(copy["result"]["previewId"].as_str().unwrap()).unwrap();
        assert_eq!(copy["result"]["name"], "Front Yard (OpenGL) copy");
        {
            let system = core.system.read();
            assert_eq!(system.previews.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Front Yard (OpenGL)", "Front Yard (OpenGL) copy", "New Preview 2"]);
            assert!(system.module_store.instance(VIXEN_PREVIEW_MODULE_TYPE, copy_id).is_some());
        }
        run(&core, "preview_enable", json!({"previewId":id,"enabled":false}));
        let listed = list(&core);
        assert_eq!(listed["items"][0]["enabled"], false);
        assert_eq!(listed["items"][0]["title"], "Front Yard");
        assert!(core.studio_display.execute(&core, "preview_delete", &json!({"previewId":id}), false).is_err());
        run(&core, "preview_delete", json!({"previewId":id,"confirm":true}));
        {
            let system = core.system.read();
            assert_eq!(system.previews.len(), 2);
            let id = Uuid::parse_str(&id).unwrap();
            assert!(!system.disabled_devices.contains(&id));
            assert!(system.module_store.instance(VIXEN_PREVIEW_MODULE_TYPE, id).is_none());
        }
        assert_eq!(core.studio_display.history()["undo"][0], "Delete Preview");
        run(&core, "display_undo", json!({}));
        assert_eq!(core.system.read().previews.len(), 3);
        assert_eq!(list(&core)["items"][0]["enabled"], false);
        run(&core, "display_undo", json!({}));
        assert_eq!(list(&core)["items"][0]["enabled"], true);
        run(&core, "display_redo", json!({}));
        assert_eq!(list(&core)["items"][0]["enabled"], false);
        // Unchanged edits add no undo step.
        let steps = core.studio_display.history()["undo"].as_array().unwrap().len();
        let unchanged = run(&core, "preview_enable", json!({"previewId":id,"enabled":false}));
        assert_eq!(unchanged["changed"], false);
        assert_eq!(core.studio_display.history()["undo"].as_array().unwrap().len(), steps);
        // An agent's preview edit is labelled, and the profile saves it.
        core.studio_display.execute(&core, "preview_rename", &json!({"previewId":id,"name":"Yard"}), true).unwrap();
        assert_eq!(core.studio_display.history()["undo"][0], "Rename Preview (Agent)");
        run(&core, "profile_save", json!({"confirm":true}));
        let loaded = vixen_io::load_system(&core.paths).unwrap();
        assert!(loaded.previews.iter().any(|p| p.name == "Yard"));
        assert!(loaded.disabled_devices.contains(&Uuid::parse_str(&id).unwrap()));
        let png = thumbnail(&core, Uuid::parse_str(&id).unwrap(), 160, 100).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn location_offset_is_one_undo_step() {
        let (core, root) = core();
        let id = run(&core, "preview_create", json!({}))["result"]["previewId"].as_str().unwrap().to_string();
        let entry = crate::preview::preview_entries(&core.system.read()).into_iter().find(|p| p.id.to_string() == id).unwrap();
        let offset = |core: &AppCore| {
            let o = crate::preview::load_preview_data(&core.system.read().module_store, &entry).unwrap().location_offset;
            (o.x.value, o.y.value)
        };
        assert_eq!(offset(&core), (0.0, 0.0));
        core.studio_display.execute(&core, "preview_location_offset", &json!({"previewId":id,"x":40,"y":-12}), true).unwrap();
        assert_eq!(offset(&core), (40.0, -12.0));
        assert_eq!(core.studio_display.history()["undo"][0], "Set Location Offset (Agent)");
        assert_eq!(run(&core, "preview_location_offset", json!({"previewId":id,"x":40,"y":-12}))["changed"], false);
        assert!(core.studio_display.execute(&core, "preview_location_offset", &json!({"previewId":id,"x":1.5,"y":0}), false).is_err());
        run(&core, "display_undo", json!({}));
        assert_eq!(offset(&core), (0.0, 0.0));
        run(&core, "display_redo", json!({}));
        assert_eq!(offset(&core), (40.0, -12.0));
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn designer_write_back_is_one_undo_step() {
        let (core, root) = core();
        let id = run(&core, "preview_create", json!({}))["result"]["previewId"].as_str().unwrap().to_string();
        let entry = crate::preview::preview_entries(&core.system.read()).into_iter().find(|p| p.id.to_string() == id).unwrap();
        let mut data = crate::preview::load_preview_data(&core.system.read().module_store, &entry).unwrap();
        assert!(!design_changes(&core.system.read(), &entry, &data), "an untouched designer changes nothing");
        core.studio_display.record_preview_edit(&core, "Edit Preview Layout", false, |_| {});
        assert_eq!(core.studio_display.history()["undo"][0], "Add Preview");
        data.width = 1234;
        assert!(design_changes(&core.system.read(), &entry, &data));
        let revision = core.display.revision();
        core.studio_display.record_preview_edit(&core, "Edit Preview Layout", true, |system| {
            crate::preview_design::save_preview_data(system, &entry, &data);
        });
        assert!(core.display.revision() > revision);
        let history = core.studio_display.history();
        assert_eq!(history["undo"][0], "Edit Preview Layout");
        assert_eq!(history["undoPreviewOnly"], true);
        let width = |core: &AppCore| crate::preview::load_preview_data(&core.system.read().module_store, &entry).unwrap().width;
        assert_eq!(width(&core), 1234);
        run(&core, "display_undo", json!({}));
        assert_ne!(width(&core), 1234);
        run(&core, "display_redo", json!({}));
        assert_eq!(width(&core), 1234);
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preview_edits_keep_the_engine_running() {
        let (core, root) = core();
        let id = run(&core, "preview_create", json!({}))["result"]["previewId"].as_str().unwrap().to_string();
        core.execution().open();
        assert!(core.execution().is_open());
        assert_eq!(core.running_previews().len(), 1);
        run(&core, "preview_enable", json!({"previewId":id,"enabled":false}));
        assert!(core.running_previews().is_empty());
        let copy = run(&core, "preview_create", json!({"name":"Roof"}))["result"]["previewId"].as_str().unwrap().to_string();
        assert_eq!(core.running_previews(), [Uuid::parse_str(&copy).unwrap()]);
        run(&core, "display_undo", json!({}));
        assert!(core.running_previews().is_empty());
        run(&core, "display_undo", json!({}));
        assert_eq!(core.running_previews(), [Uuid::parse_str(&id).unwrap()]);
        // Still open: a preview edit never closes execution or rebuilds the output graph.
        assert!(core.execution().is_open());
        core.execution().close();
        drop(core);
        std::fs::remove_dir_all(root).unwrap();
    }
}

