//! Show-list authoring and disabled schedule drafts; never launches shows or external programs.
use crate::{scheduler, setup::SetupSession, shows};
use serde_json::{json, Value};
use uuid::Uuid;
fn text<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a[k].as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or(format!("Invalid {k}"))
}
fn id(a: &Value) -> Result<Uuid, String> {
    Uuid::parse_str(text(a, "id")?).map_err(|_| "Invalid id".into())
}
pub fn edit(s: &mut SetupSession, name: &str, a: &Value) -> Result<Value, String> {
    let mut shows = shows::load_shows(&s.system.module_store);
    let result = match name {
        "show_create" => {
            let files = a["sequences"]
                .as_array()
                .filter(|v| !v.is_empty() && v.len() <= 100)
                .ok_or("sequences must contain 1..100 filenames")?;
            let mut items = Vec::new();
            for (i, file) in files.iter().enumerate() {
                let file = file
                    .as_str()
                    .filter(|f| {
                        f.len() <= 256
                            && !f.contains('/')
                            && !f.contains(char::from(92))
                            && f.to_lowercase().ends_with(".tim")
                    })
                    .ok_or("Use .tim filenames under Sequence")?;
                let mut item = shows::ShowItem::new(shows::ShowItemType::Sequential, file);
                item.action = shows::ActionType::Sequence;
                item.sequence_file_name = Some(file.into());
                item.item_order = i as i32;
                items.push(item);
            }
            let show = shows::Show {
                id: Uuid::new_v4(),
                name: Some(text(a, "name")?.into()),
                items,
            };
            let id = show.id;
            shows.push(show);
            shows::save_shows(&mut s.system.module_store, &shows);
            json!({"id":id.to_string()})
        }
        "show_rename" => {
            let id = id(a)?;
            let index = shows
                .iter()
                .position(|x| x.id == id)
                .ok_or("Show not found")?;
            shows::rename_show(&mut shows, index, text(a, "name")?);
            shows::save_shows(&mut s.system.module_store, &shows);
            json!({})
        }
        "show_delete" => {
            if a["confirm"] != true {
                return Err("confirm:true required".into());
            }
            let id = id(a)?;
            if scheduler::load_scheduler(&s.system.module_store)
                .items
                .iter()
                .any(|item| item.show_id == id)
            {
                return Err("Remove schedules referencing this show first".into());
            }
            let index = shows
                .iter()
                .position(|x| x.id == id)
                .ok_or("Show not found")?;
            shows.remove(index);
            shows::save_shows(&mut s.system.module_store, &shows);
            json!({})
        }
        "schedule_create" => {
            let show = id(a)?;
            if !shows.iter().any(|x| x.id == show) {
                return Err("Show not found".into());
            }
            let start = vixen_io::datetime::LocalDateTime::parse(text(a, "start")?)
                .ok_or("Invalid local start date/time")?;
            let end = vixen_io::datetime::LocalDateTime::parse(text(a, "end")?)
                .ok_or("Invalid local end date/time")?;
            let start_date = scheduler::Date::of(&start);
            let end_date = scheduler::Date::of(&end);
            scheduler::validate_schedule_edit(start_date, end_date, Some(show))
                .map_err(|e| format!("{e:?}"))?;
            let values = a["days"]
                .as_array()
                .filter(|v| v.len() == 7)
                .ok_or("days must contain seven booleans, Sunday first")?;
            let mut days = [false; 7];
            for (i, v) in values.iter().enumerate() {
                days[i] = v.as_bool().ok_or("days values must be booleans")?;
            }
            let mut data = scheduler::load_scheduler(&s.system.module_store);
            let index = data.items.len();
            data.items.push(scheduler::ScheduleItem {
                show_id: show,
                days,
                enabled: false,
                start_date,
                end_date,
                start_time: scheduler::TimeOfDay::of(&start),
                end_time: scheduler::TimeOfDay::of(&end),
            });
            scheduler::save_scheduler(
                &mut s.system.module_store,
                &data,
                &vixen_io::datetime::LocalDateTime::now(),
            );
            json!({"index":index,"enabled":false,"note":"Enable the schedule locally after review"})
        }
        "schedule_delete" => {
            if a["confirm"] != true || a.get("expectedRevision").is_none() {
                return Err("confirm:true and expectedRevision required".into());
            }
            let index = a["index"].as_u64().ok_or("index must be integer")? as usize;
            let mut data = scheduler::load_scheduler(&s.system.module_store);
            if index >= data.items.len() {
                return Err("Schedule not found".into());
            }
            data.items.remove(index);
            scheduler::save_scheduler(
                &mut s.system.module_store,
                &data,
                &vixen_io::datetime::LocalDateTime::now(),
            );
            json!({})
        }
        _ => return Err("Unknown display/show command".into()),
    };
    s.dirty.studio_module = true;
    Ok(result)
}
pub fn tools() -> Vec<Value> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let boolean = json!({"type":"boolean"});
    vec![
        ("show_create","Create a sequential show from local .tim filenames; no program-launch actions or playback.",json!({"name":string,"sequences":{"type":"array","items":string,"minItems":1,"maxItems":100}}),vec!["name","sequences"],false),
        ("show_rename","Rename a show by UUID.",json!({"id":string,"name":string}),vec!["id","name"],false),
        ("show_delete","Delete an unreferenced show after confirmation.",json!({"id":string,"confirm":boolean}),vec!["id","confirm"],true),
        ("schedule_create","Draft a DISABLED schedule. id is show UUID; start/end are local ISO date-times; days is seven booleans Sunday-first. Enable locally after review.",json!({"id":string,"start":string,"end":string,"days":{"type":"array","items":boolean,"minItems":7,"maxItems":7}}),vec!["id","start","end","days"],false),
        ("schedule_delete","Delete a schedule by index with explicit confirmation and snapshot revision.",json!({"index":{"type":"integer","minimum":0},"confirm":boolean}),vec!["index","confirm","expectedRevision"],true),
    ].into_iter().map(|(name,description,mut properties,required,destructive)|{properties["expectedRevision"]=json!({"type":"integer","minimum":0});json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":destructive,"openWorldHint":false}})}).collect()
}
