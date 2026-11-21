//! Studio: the Text Conversion Failure sheet's word mapping as one step on the profile's shared undo history (`phoneme_mapping_set`,
//! the command agents run), with the Lyric Converter window's Undo and Redo of it, and the user dictionary's list.
use crate::core::VixenCore;
use crate::marks::MarkMessage;
use crate::sequence::SequencerSession;
use serde_json::{json, Value};
use vixen_session::studio_phoneme_mappings as mappings;

/// What Map Word did: `label` names its undo step (none when the word already had that mapping), `problem` is an alert to show.
#[derive(uniffi::Record)]
pub struct StudioWordMapping {
    pub label: Option<String>,
    pub problem: Option<MarkMessage>,
}

#[uniffi::export]
impl VixenCore {
    /// The user dictionary as JSON: `{mappings: [{word, phonemes}]}`.
    pub fn studio_phoneme_mapping_list(&self) -> String {
        mappings::list(&self.inner).to_string()
    }
}

#[uniffi::export]
impl SequencerSession {
    /// F82's Map Word: `word` as it was asked about, `mapped` as the buttons wrote it ("AI WQ "). An empty mapping is written too, as
    /// upstream's OK does, so the word is skipped from then on.
    pub fn studio_map_word(&self, word: String, mapped: String) -> StudioWordMapping {
        let args = json!({"word": word, "phonemes": mapped.split_whitespace().collect::<Vec<_>>()});
        match self.core.studio_display.execute(&self.core, "phoneme_mapping_set", &args, false) {
            Ok(reply) => StudioWordMapping {
                label: (reply["changed"] == true).then(|| mappings::label("phoneme_mapping_set", &args)),
                problem: None,
            },
            Err(e) => {
                vixen_session::logging::error("LipSyncTextConvert", "The user dictionary couldn't be written.", Some(&e));
                StudioWordMapping { label: None, problem: Some(MarkMessage { title: "Error".to_string(), message: e }) }
            }
        }
    }

    /// Undoes the shared history's latest step if it is still the one labelled `label` (another window or an agent may have moved it).
    pub fn studio_undo_word_mapping(&self, label: String) -> bool {
        self.step_word_mapping("undo", &label)
    }

    pub fn studio_redo_word_mapping(&self, label: String) -> bool {
        self.step_word_mapping("redo", &label)
    }
}

impl SequencerSession {
    fn step_word_mapping(&self, side: &str, label: &str) -> bool {
        let history = self.core.studio_display.history();
        if history[side].get(0).and_then(Value::as_str) != Some(label) {
            return false;
        }
        self.core.studio_display.execute(&self.core, &format!("display_{side}"), &json!({}), false).is_ok()
    }
}
