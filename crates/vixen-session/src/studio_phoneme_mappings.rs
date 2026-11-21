//! The profile's user phoneme dictionary (`<profile>/user_dictionary`, F84/F85) as live edits on the shared undo history
//! (`studio_display`): the Lyric Converter's and Breakdown Word's Text Conversion Failure sheet maps a word as one step, and agents
//! add, remove and list mappings with the same commands (rest of R40).
//!
//! The dictionary is a profile file, not part of the sequence or the system configuration, so a step keeps the file's bytes as they
//! were (absent included) beside the history's usual snapshot, as `studio_lipsync` keeps picture folders; Undo writes them back and
//! the next lookup reads the file again.
//!
//! A mapping appends `WORD PHONEMES` to the file, as upstream's `AddUserMaping` does, so Windows Vixen reads the same file and the last
//! line for a word wins. Removing a word rewrites the file without its lines, and the standard dictionary's pronunciation applies again.
use crate::app::AppCore;
use crate::marks::lipsync::{PhonemeDictionary, MAP_PHONEMES};
use crate::marks::lyric_converter as lyric;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const LOGGER: &str = "StudioPhonemeMappings";

/// The commands that edit the dictionary on the shared history.
pub const COMMANDS: [&str; 2] = ["phoneme_mapping_set", "phoneme_mapping_remove"];

/// The longest user dictionary an edit reads (it is kept in memory for Undo); upstream's files are a few kilobytes.
const FILE_LIMIT: u64 = 16 * 1024 * 1024;

pub fn is_command(command: &str) -> bool {
    COMMANDS.contains(&command)
}

/// The undo history's label, in the words Edit ▸ Undo shows: `Map Word “WIBBLE”`.
pub fn label(command: &str, args: &Value) -> String {
    let word = args["word"].as_str().map(|w| upper(w.trim())).unwrap_or_default();
    match command {
        "phoneme_mapping_remove" => format!("Remove Word Mapping “{word}”"),
        _ => format!("Map Word “{word}”"),
    }
}

/// `<profile>/user_dictionary`, where the vanilla Lyric Converter reads and writes it.
pub fn user_dictionary(core: &AppCore) -> PathBuf {
    core.paths.root().join("user_dictionary")
}

/// The dictionary file as it was before a step: `None` when there was no file.
#[derive(Clone, Debug, Default)]
pub struct Saved(Option<Vec<u8>>);

fn read(path: &Path) -> Result<Saved, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err("The user dictionary is a link; Studio doesn't edit it".into()),
        Ok(meta) if !meta.is_file() => Err("The user dictionary isn't a file".into()),
        Ok(meta) if meta.len() > FILE_LIMIT => Err("The user dictionary is larger than 16 MB, too large to edit with Undo".into()),
        Ok(_) => std::fs::read(path).map(|b| Saved(Some(b))).map_err(|e| format!("Cannot read the user dictionary: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Saved(None)),
        Err(e) => Err(format!("Cannot read the user dictionary: {e}")),
    }
}

impl Saved {
    /// Puts the file back as it was, and returns it as it was just before, for the opposite step. The next lookup reads it again.
    pub fn restore(&self, path: &Path) -> Saved {
        let current = read(path).unwrap_or_default();
        let outcome = match &self.0 {
            Some(bytes) => std::fs::write(path, bytes),
            None => match std::fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            },
        };
        if let Err(e) = outcome {
            crate::logging::error(LOGGER, "Could not restore the user dictionary.", Some(&e.to_string()));
        }
        lyric::release_dictionary(path);
        current
    }
}

/// `ToUpper` as the dictionary does it, one character at a time.
fn upper(text: &str) -> String {
    text.chars()
        .map(|c| {
            let mut u = c.to_uppercase();
            match (u.next(), u.next()) {
                (Some(x), None) => x,
                _ => c,
            }
        })
        .collect()
}

fn word(args: &Value) -> Result<String, String> {
    let word = args["word"].as_str().map(str::trim).unwrap_or_default();
    if word.is_empty() || word.len() > 256 || word.chars().any(char::is_control) {
        return Err("word must be a nonempty single line of at most 256 bytes".into());
    }
    if word == ";;;" {
        return Err("“;;;” marks a comment in the dictionary".into());
    }
    Ok(word.to_string())
}

/// The ten mouth shapes F82's buttons write, by name.
fn phonemes(args: &Value) -> Result<Vec<&'static str>, String> {
    let list = args["phonemes"].as_array().ok_or("phonemes must be a list of phoneme names")?;
    list.iter()
        .map(|p| {
            let name = p.as_str().unwrap_or_default();
            MAP_PHONEMES
                .iter()
                .map(|m| m.name())
                .find(|m| m.eq_ignore_ascii_case(name))
                .ok_or_else(|| format!("Unknown phoneme “{name}”; use {}", MAP_PHONEMES.iter().map(|m| m.name()).collect::<Vec<_>>().join(", ")))
        })
        .collect()
}

/// The file's key for a line: its first word, as `parse_line` reads it.
fn line_key(line: &str) -> &str {
    line.trim_end_matches('\r').split(' ').next().unwrap_or_default()
}

/// The words the file maps, each once (its last line wins), with what the dictionary makes of them.
fn mappings(path: &Path) -> Vec<(String, Vec<String>)> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    let text = vixen_io::marks_files::read_import_text(&bytes);
    let mut words: Vec<String> = text.lines().map(line_key).filter(|k| !k.is_empty() && *k != ";;;").map(str::to_string).collect();
    words.sort();
    words.dedup();
    let dictionary = PhonemeDictionary::load(Some(path));
    words.into_iter().map(|w| (w.clone(), dictionary.try_convert(&w).into_iter().map(|p| p.name().to_string()).collect())).collect()
}

/// One edit of the dictionary file: the result, whether anything changed, and the file as it was. On an error the file is put back
/// (a mapping that couldn't be written still serves this run, as upstream's F85 guard keeps it).
pub fn edit(path: &Path, command: &str, args: &Value) -> Result<(Value, bool, Saved), String> {
    let before = read(path)?;
    let word = word(args)?;
    let key = upper(&word);
    match command {
        "phoneme_mapping_set" => {
            let phonemes = phonemes(args)?;
            if mappings(path).iter().any(|(w, p)| *w == key && p.iter().map(String::as_str).eq(phonemes.iter().copied())) {
                return Ok((json!({"word": key, "phonemes": phonemes}), false, before));
            }
            let mapped: String = phonemes.iter().map(|p| format!("{p} ")).collect();
            if let Err(e) = lyric::with_dictionary(path, |d| d.add_user_mapping(&word, &mapped, path)) {
                before.restore(path);
                return Err(format!("The mapping couldn't be saved to the user dictionary, so it lasts only until Vixen closes.\n\n{e}"));
            }
            Ok((json!({"word": key, "phonemes": phonemes}), true, before))
        }
        "phoneme_mapping_remove" => {
            let text = before.0.as_deref().map(vixen_io::marks_files::read_import_text).unwrap_or_default();
            let lines: Vec<&str> = text.split('\n').collect();
            let kept: Vec<&str> = lines.iter().copied().filter(|l| upper(line_key(l)) != key).collect();
            if kept.len() == lines.len() {
                return Err(format!("“{key}” has no mapping in the user dictionary"));
            }
            let mut rest = kept.join("\n");
            // A Windows file whose last lines went: its new last line keeps no dangling CR.
            if rest.ends_with('\r') {
                rest.pop();
            }
            if let Err(e) = std::fs::write(path, rest) {
                before.restore(path);
                return Err(format!("The user dictionary couldn't be written: {e}"));
            }
            lyric::release_dictionary(path);
            let standard = PhonemeDictionary::load(None).try_convert(&key).into_iter().map(|p| p.name()).collect::<Vec<_>>();
            Ok((json!({"word": key, "standard": standard}), true, before))
        }
        _ => Err("Unknown phoneme mapping command".into()),
    }
}

/// `phoneme_mapping_list`: `{mappings: [{word, phonemes}]}`, words as the file keys them (upper case), sorted.
pub fn describe(path: &Path) -> Value {
    let mappings: Vec<Value> = mappings(path).into_iter().map(|(word, phonemes)| json!({"word": word, "phonemes": phonemes})).collect();
    json!({"mappings": mappings})
}

pub fn list(core: &AppCore) -> Value {
    describe(&user_dictionary(core))
}

pub fn tools() -> Vec<Value> {
    let word = json!({"type":"string","minLength":1,"maxLength":256});
    let names: Vec<&str> = MAP_PHONEMES.iter().map(|m| m.name()).collect();
    let phonemes = json!({"type":"array","items":{"type":"string","enum":names},"maxItems":64});
    vec![
        ("phoneme_mapping_set","Map a word to mouth shapes in the profile's user phoneme dictionary, as the Lyric Converter's Text Conversion Failure sheet does: the Lyric Converter, Breakdown Word and LipSync use it in place of (or where there is no) standard pronunciation. phonemes in order, from AI, E, ETC, FV, L, MBP, O, REST, U, WQ; an empty list maps the word to nothing (it is skipped). One word: the dictionary keys a line by its first word. One undo step on the shared history (display_undo puts the file back).",json!({"word":word,"phonemes":phonemes}),vec!["word","phonemes"],false),
        ("phoneme_mapping_remove","Remove a word's mappings from the profile's user phoneme dictionary; its standard pronunciation, if any, applies again (returned as standard). One undo step on the shared history.",json!({"word":word}),vec!["word"],false),
    ].into_iter().map(|(name,description,mut properties,required,destructive)|{properties["expectedRevision"]=json!({"type":"integer","minimum":0});json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":destructive,"openWorldHint":false}})}).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use uuid::Uuid;

    fn exec(core: &AppCore, command: &str, args: Value) -> Value {
        core.studio_display.execute(core, command, &args, false).unwrap_or_else(|e| panic!("{command}: {e}"))
    }

    fn lookup(core: &AppCore, word: &str) -> Vec<String> {
        lyric::with_dictionary(&user_dictionary(core), |d| lyric::word_phonemes(d, word))
    }

    #[test]
    fn mappings_undo_with_the_file_on_the_shared_history() {
        let root = std::env::temp_dir().join(format!("studio-phoneme-mappings-{}", Uuid::new_v4().simple()));
        let core = Arc::new(AppCore::open(root.join("profile"), true).unwrap());
        let path = user_dictionary(&core);
        assert!(!path.exists());
        assert!(lookup(&core, "zorbleplex").is_empty());

        let set = exec(&core, "phoneme_mapping_set", json!({"word": "Zorbleplex", "phonemes": ["ETC", "O", "MBP"]}));
        assert_eq!(set["result"], json!({"word": "ZORBLEPLEX", "phonemes": ["ETC", "O", "MBP"]}));
        assert_eq!(lookup(&core, "zorbleplex"), ["ETC", "O", "MBP"], "live at once");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "\nZORBLEPLEX ETC O MBP ", "upstream's line format");
        // The same mapping again changes nothing and adds no step.
        let again = exec(&core, "phoneme_mapping_set", json!({"word": "zorbleplex", "phonemes": ["ETC", "O", "MBP"]}));
        assert_eq!(again["changed"], false);
        exec(&core, "phoneme_mapping_set", json!({"word": "hello", "phonemes": ["MBP"]}));
        exec(&core, "phoneme_mapping_set", json!({"word": "zorbleplex", "phonemes": []}));
        assert!(lookup(&core, "zorbleplex").is_empty(), "the last line wins");
        assert_eq!(
            list(&core)["mappings"],
            json!([{"word": "HELLO", "phonemes": ["MBP"]}, {"word": "ZORBLEPLEX", "phonemes": []}])
        );
        let removed = exec(&core, "phoneme_mapping_remove", json!({"word": "Hello"}));
        assert_eq!(removed["result"]["standard"], json!(["ETC", "AI", "L", "O"]), "the standard pronunciation is back");
        assert_eq!(lookup(&core, "hello"), ["ETC", "AI", "L", "O"]);
        assert_eq!(
            core.studio_display.history()["undo"],
            json!(["Remove Word Mapping “HELLO”", "Map Word “ZORBLEPLEX”", "Map Word “HELLO”", "Map Word “ZORBLEPLEX”"])
        );
        assert_eq!(core.studio_display.history()["undoPreviewOnly"], true, "a dictionary step undoes with Live Output on");

        exec(&core, "display_undo", json!({}));
        assert_eq!(lookup(&core, "hello"), ["MBP"], "Undo of Remove puts the line back");
        exec(&core, "display_undo", json!({}));
        assert_eq!(lookup(&core, "zorbleplex"), ["ETC", "O", "MBP"]);
        exec(&core, "display_redo", json!({}));
        assert!(lookup(&core, "zorbleplex").is_empty());
        exec(&core, "display_undo", json!({}));
        exec(&core, "display_undo", json!({}));
        exec(&core, "display_undo", json!({}));
        assert!(!path.exists(), "Undo of the first mapping takes away the file it made");
        assert!(lookup(&core, "zorbleplex").is_empty());

        // Refusals change nothing.
        let fail = |command: &str, args: Value| core.studio_display.execute(&core, command, &args, true).unwrap_err();
        assert!(fail("phoneme_mapping_set", json!({"word": "x", "phonemes": ["XX"]})).contains("not an allowed value"));
        assert!(fail("phoneme_mapping_set", json!({"word": "a\nb", "phonemes": []})).contains("single line"));
        assert!(fail("phoneme_mapping_remove", json!({"word": "nothing"})).contains("no mapping"));
        assert!(!path.exists());
        assert!(core.studio_display.history()["undo"].as_array().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removing_keeps_other_lines_and_their_line_endings() {
        let root = std::env::temp_dir().join(format!("studio-phoneme-remove-{}", Uuid::new_v4().simple()));
        let core = Arc::new(AppCore::open(root.join("profile"), true).unwrap());
        let path = user_dictionary(&core);
        // A Windows file, with CRLF and a word mapped twice.
        std::fs::write(&path, "\r\nWIBBLE AI WQ \r\nHELLO MBP\r\nWIBBLE U").unwrap();
        lyric::release_dictionary(&path);
        exec(&core, "phoneme_mapping_remove", json!({"word": "wibble"}));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "\r\nHELLO MBP");
        assert!(lookup(&core, "wibble").is_empty());
        exec(&core, "display_undo", json!({}));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "\r\nWIBBLE AI WQ \r\nHELLO MBP\r\nWIBBLE U");
        assert_eq!(lookup(&core, "wibble"), ["U"]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
