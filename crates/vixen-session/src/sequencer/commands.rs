//! The window's commands that aren't transport or the clipboard: the effect context menu (section E), adding on marks, clones, Edit Time, drops of effects, marks and audio.

use crate::sequence::EffectId;
use crate::sequencer::dialogs::{offset_marks_in_range, AddMultipleState, EffectTimeEditorState};
use crate::sequencer::effect_props::{collect_props, set_nested, PropType, ALL_COLOR_TYPES};
use crate::sequencer::find::{self, FindMode, FindRow};
use crate::sequencer::effect_ops::{self, AddOnMarksOptions};
use crate::sequencer::grid::DEPRECATED_TAG;
use crate::sequencer::state::{Alert, AlertAction, AlertConfirm, AlertStyle, CommandOutcome, EffectCatalogEntry, SequencerState, NO_SELECTION_WARNING};
use crate::sequencer::clipboard::{self, ClipboardData, ClipboardEffect, ContextPasteState};
use crate::marks::map_library::LipSyncLibrary;
use crate::sequencer::papagayo::{import_summary, parse_papagayo};
use crate::effect_editor::libraries::{save_libraries, unique_import_name};
use crate::sequencer::libraries::{read_vcl, read_vgl, save_color_collections, write_vcl, write_vgl, ColorCollection};
use crate::undo::{AddRemoveKind, MoveKind, UndoAction};
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;
use vixen_core::color::Rgb;
use vixen_core::layer::SequenceLayers;
use vixen_core::{Color, ColorGradient, Mark, MarkCollection, TimeSpan};
use vixen_effects::framework::{EffectGroup, ParamValue};
use vixen_io::{IoError, MediaEntry};

/// `MinElemWidthPx`: `MinimumElementDuration` is this many pixels of time at the present zoom. F245.
const MINIMUM_ELEMENT_WIDTH_PX: i64 = 10;

/// The supported audio extensions: F283.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "mpeg3", "wav", "wave", "flac", "fla", "aiff", "aif", "aifc", "aac", "adt", "adts", "m2ts", "mp2", "3g2", "3gp2", "3gp", "3gpp", "m4a", "m4v",
    "mp4v", "mp4", "mov", "asf", "wm", "wmv", "wma", "mp1", "avi", "ac3", "ec3", "ogg",
];

/// `AudioDescriptor.ModuleID`: what every media surrogate in a real show is.
const AUDIO_MODULE_TYPE: Uuid = uuid::uuid!("fe460392-3fab-4c63-99dd-d76c48354150");

/// One layer in the context menu. F227.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerMenuItem {
    pub id: Uuid,
    pub name: String,
    pub filter_name: String,
    pub in_use: bool,
}

/// The context menu's Layer submenu. Items in reverse layer order. `single_in_use` picks the full check mark rather than the medium one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerMenu {
    pub items: Vec<LayerMenuItem>,
    pub single_in_use: bool,
}

/// One entry in the context menu's Collections submenu. F253.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectionMenuItem {
    pub name: String,
    pub description: String,
}

/// `timelineControl_ContextSelected`'s menu. F226 to F254.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenuModel {
    pub row_deprecated: bool,
    /// Sorted by group then name, for the separators between groups.
    pub add_effect_groups: Vec<Vec<EffectCatalogEntry>>,
    pub add_effect_enabled: bool,
    pub add_effect_tooltip: String,
    pub layers: Option<LayerMenu>,
    pub alignment_enabled: bool,
    pub alignment_tooltip: String,
    pub reference_enabled: bool,
    pub distribute_enabled: bool,
    pub marks_enabled: bool,
    pub show_manipulation: bool,
    pub clone_to_selected_enabled: bool,
    pub show_edit_time: bool,
    pub edit_time_enabled: bool,
    pub edit_time_tooltip: String,
    pub cut_copy_enabled: bool,
    pub paste: ContextPasteState,
    pub show_delete: bool,
    pub collections: Vec<CollectionMenuItem>,
    pub element: Option<EffectId>,
}

/// `AudioOutputManager.DefaultDevicePlaceholder`.
const SYSTEM_DEFAULT_DEVICE_ID: &str = "-0";
const SYSTEM_DEFAULT_DEVICE_NAME: &str = "System Default";

/// One row of the Audio Devices menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDeviceEntry {
    pub id: String,
    pub name: String,
    pub checked: bool,
}

/// What a file's own name decides. F311.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddAudioOutcome {
    /// "The selected file is not a supported type."
    Unsupported,
    /// Zero length: all media cleared. F302's FIX marks it modified.
    Unprocessable,
    /// Added. `resize_prompt`: the length differs from the audio's and isn't the default.
    Added { resize_prompt: bool },
}

impl SequencerState {
    fn visible_end(&self) -> TimeSpan {
        self.grid.visible_start() + self.grid.visible_span()
    }

    // ------------------------------------------------------------------------------------------------------------------
    // The context menu
    // ------------------------------------------------------------------------------------------------------------------

    pub fn context_menu(&self, row: Option<u32>, under_pointer: &[EffectId]) -> ContextMenuModel {
        let element = under_pointer.first().copied();
        let summary = row.and_then(|r| self.grid.row_summary(r));
        let row_deprecated = summary.as_ref().is_some_and(|r| r.deprecated);
        let selected = self.grid.selected_elements();
        let count = selected.len();
        let session = self.session.lock();

        let mut catalog: Vec<EffectCatalogEntry> = crate::sequencer::effect_catalog();
        catalog.sort_by_key(|e| (group_rank(e.group), e.name));
        let mut groups: Vec<Vec<EffectCatalogEntry>> = Vec::new();
        for entry in catalog {
            match groups.last_mut() {
                Some(list) if list.first().is_some_and(|e| e.group == entry.group) => list.push(entry),
                _ => groups.push(vec![entry]),
            }
        }

        let allowed = self.grid.ok_to_use_alignment(&selected);
        let element_selected = element.is_some_and(|e| selected.contains(&e));
        let reference_enabled = element.is_some() && allowed && (count > 1 || (count == 1 && !element_selected));
        let distribute_enabled = allowed && count > 1;
        let marks_enabled = session.marks().iter().any(|c| !c.marks.is_empty());
        let mut alignment_enabled = reference_enabled || distribute_enabled;
        let mut alignment_tooltip = String::new();
        if !alignment_enabled {
            alignment_tooltip = if !allowed {
                "Disabled, maximum selected effects per row is 32.".to_string()
            } else if count == 1 {
                "Select more then one effect or ensure you have Marks added to enable the Alignment feature.".to_string()
            } else {
                "Select more then one effect to enable the Alignment feature.".to_string()
            };
        }
        if marks_enabled {
            alignment_enabled = true;
            alignment_tooltip = String::new();
        }

        let marks_present = clipboard::marks_present_after(self.grid.cursor(), session.marks());
        let layers = self.layer_menu_locked(&session, under_pointer);
        let show_effect_items = element.is_some() && !row_deprecated;

        let collections = if count > 0 && !row_deprecated && (count > 1 || self.supports_color_lists_locked(&session, selected[0])) {
            self.color_collections.iter().filter(|c| !c.colors.is_empty()).map(|c| CollectionMenuItem { name: c.name.clone(), description: c.description.clone() }).collect()
        } else {
            Vec::new()
        };

        ContextMenuModel {
            row_deprecated,
            add_effect_groups: groups,
            add_effect_enabled: !row_deprecated,
            add_effect_tooltip: if row_deprecated {
                "Disabled, this row is tagged as Deprecated.".to_string()
            } else {
                "Use Shift key to add multiple effects of the same type.".to_string()
            },
            layers,
            // A deprecated row disables it entirely, even with marks.
            alignment_enabled: alignment_enabled && !row_deprecated,
            alignment_tooltip,
            reference_enabled,
            distribute_enabled,
            marks_enabled,
            show_manipulation: show_effect_items,
            // More than 2 selected, so 2 isn't enough (KEEP F244).
            clone_to_selected_enabled: count > 2,
            show_edit_time: show_effect_items,
            edit_time_enabled: allowed,
            edit_time_tooltip: if allowed { String::new() } else { "Disabled, maximum selected effects per row is 32.".to_string() },
            cut_copy_enabled: count > 0,
            paste: clipboard::context_paste_state(self.clipboard_count, marks_present, row_deprecated),
            show_delete: count > 0,
            collections,
            element,
        }
    }

    fn supports_color_lists_locked(&self, session: &crate::sequence::SequenceSession, id: EffectId) -> bool {
        let Some(entry) = session.effect(id) else { return false };
        let Some(descriptor) = entry.descriptor else { return false };
        let Some(info) = session.target_info(id) else { return false };
        crate::sequencer::effect_props::color_properties(descriptor, &entry.node.params, &info).iter().any(|p| p.is_list())
    }

    /// F227: only with effects selected or under the pointer, and more than one layer.
    fn layer_menu_locked(&self, session: &crate::sequence::SequenceSession, under_pointer: &[EffectId]) -> Option<LayerMenu> {
        let selected = self.grid.selected_elements();
        let targets: &[EffectId] = if selected.is_empty() { under_pointer } else { &selected };
        let layers = &session.document().layers;
        if targets.is_empty() || layers.layers.len() <= 1 {
            return None;
        }
        let used: std::collections::HashSet<Uuid> = targets.iter().filter_map(|id| session.layer_of(*id)).collect();
        let items = layers
            .layers
            .iter()
            .rev()
            .map(|l| LayerMenuItem {
                id: l.id,
                name: l.name.clone(),
                // `Layer.FilterName` for the default layer, which has no filter.
                filter_name: l.filter.as_ref().map_or_else(|| "None".to_string(), crate::sequencer::layers::filter_type_name),
                in_use: used.contains(&l.id),
            })
            .collect();
        Some(LayerMenu { items, single_in_use: used.len() == 1 })
    }

    /// F227's click: applies to the selection if any, else what's under the pointer, and only to effects whose layer differs. One undo step, only if something changed.
    pub fn assign_layer(&mut self, under_pointer: &[EffectId], layer: Uuid) {
        let selected = self.grid.selected_elements();
        let targets: Vec<EffectId> = if selected.is_empty() { under_pointer.to_vec() } else { selected };
        let mut session = self.session.lock();
        let before: Vec<(EffectId, Uuid)> =
            targets.iter().filter_map(|id| session.layer_of(*id).filter(|l| *l != layer).map(|l| (*id, l))).collect();
        if before.is_empty() {
            return;
        }
        let ids: Vec<EffectId> = before.iter().map(|(id, _)| *id).collect();
        session.set_layer(&ids, layer);
        session.record_undo(UndoAction::LayersChanged { layers: before });
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Adding
    // ------------------------------------------------------------------------------------------------------------------

    /// F226's single add at the grid's time, or F305's drop: default duration, selected.
    pub fn add_effect_at(&mut self, type_id: Uuid, row: u32, time: TimeSpan) -> Option<EffectId> {
        let duration = effect_ops::default_effect_duration(self.visible_end(), time);
        self.add_effect(type_id, row, time, duration, true)
    }

    /// F291's set-up: the dialog's fields.
    pub fn add_multiple_open(&self, effect_name: &str, time: TimeSpan) -> AddMultipleState {
        let session = self.session.lock();
        let remembered = self.grid.remembered_range().unwrap_or_else(|| self.transport.remembered());
        AddMultipleState::open(effect_name, time, session.length(), remembered.start, remembered.end, session.marks())
    }

    /// F291's OK and F292. One undo step; the plain path pushes it even for zero effects.
    pub fn add_multiple_apply(&mut self, type_id: Uuid, row: u32, state: &AddMultipleState) -> usize {
        state.remember();
        let Some(summary) = self.grid.row_summary(row) else { return 0 };
        let mut session = self.session.lock();
        let placed = if state.align_to_marks {
            let mut marks = Vec::new();
            for name in &state.checked_marks {
                for collection in session.marks().iter().filter(|c| &c.name == name) {
                    marks.extend(collection.marks.iter().filter(|m| m.start_time >= state.start).map(|m| (m.start_time, m.duration)));
                }
            }
            if marks.is_empty() {
                // Undo and modified only if at least one mark time matched.
                return 0;
            }
            effect_ops::plan_add_on_marks(
                &marks,
                AddOnMarksOptions {
                    count: state.count,
                    duration: state.duration,
                    fill: state.fill,
                    skip_every_other: state.skip_every_other,
                    align_to_mark_duration: state.mark_start_end,
                },
            )
        } else {
            effect_ops::plan_add_multiple(state.start, state.duration, state.between, state.count, session.length())
        };
        let mut added = Vec::with_capacity(placed.len());
        for (start, duration) in &placed {
            if let Some(id) = session.add_new_effect(type_id, summary.node, &summary.name, *start, *duration) {
                added.push(id);
            }
        }
        session.set_modified();
        session.record_undo(UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids: added.clone() });
        drop(session);
        self.grid.effects_added(&added);
        if !added.is_empty() && state.select_effects {
            self.grid.set_selection(&added);
        }
        added.len()
    }

    /// `EffectDropped`, replacing: F305. Each selected effect is replaced by a new one at the same row, start and duration: two undo steps (KEEP).
    pub fn replace_selected(&mut self, type_id: Uuid) -> Vec<EffectId> {
        let selected = self.grid.selected_elements();
        let places: Vec<(Uuid, String, TimeSpan, TimeSpan)> = {
            let session = self.session.lock();
            selected
                .iter()
                .filter_map(|id| {
                    let entry = session.effect(*id)?;
                    let (node, name) = entry.node.targets.first()?;
                    Some((*node, name.clone(), entry.node.start, entry.node.time_span))
                })
                .collect()
        };
        self.delete_selected();
        let mut session = self.session.lock();
        let added: Vec<EffectId> = places.iter().filter_map(|(node, name, start, span)| session.add_new_effect(type_id, *node, name, *start, *span)).collect();
        session.record_undo(UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids: added.clone() });
        drop(session);
        self.grid.effects_added(&added);
        self.grid.set_selection(&added);
        added
    }

    /// F305's prompt for a ⌘-drop onto a selection.
    pub fn replace_prompt(&self, type_id: Uuid) -> Alert {
        Alert {
            title: "Replace existing effects?".to_string(),
            message: format!("This action will replace {} effects, are you sure ?", self.grid.selected_elements().len()),
            style: AlertStyle::Warning,
            confirm: Some(AlertConfirm {
                label: "Replace Effects".to_string(),
                cancel_label: "Cancel".to_string(),
                action: AlertAction::ReplaceSelected(type_id),
            }),
        }
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Clones and times
    // ------------------------------------------------------------------------------------------------------------------

    /// `CloneElements`: F243, with the selection if any, else the clicked effect. One undo step.
    pub fn clone_elements(&mut self, clicked: EffectId) -> Vec<EffectId> {
        let selected = self.grid.selected_elements();
        let list = if selected.is_empty() { vec![clicked] } else { selected };
        let mut session = self.session.lock();
        let mut added = Vec::with_capacity(list.len());
        for id in list {
            let Some(entry) = session.effect(id) else { continue };
            let node = entry.node.clone();
            let Some((target, name)) = node.targets.first().cloned() else { continue };
            let (start, span) = (node.start, node.time_span);
            let layer = session.layer_of(id);
            if let Some(new_id) = session.add_copy(node, target, &name, start, span) {
                if let Some(layer) = layer {
                    session.set_layer(&[new_id], layer);
                }
                added.push(new_id);
            }
        }
        session.set_modified();
        session.record_undo(UndoAction::EffectsAdded { kind: AddRemoveKind::AddedOrRemoved, ids: added.clone() });
        drop(session);
        self.grid.effects_added(&added);
        added
    }

    /// F244's warning when some selected effects differ in type.
    pub fn clone_to_selected_prompt(&self, source: EffectId) -> Option<Alert> {
        let session = self.session.lock();
        let source_entry = session.effect(source)?;
        let selected = self.grid.selected_elements();
        let differs = selected.iter().any(|id| session.effect(*id).is_some_and(|e| e.node.type_id != source_entry.node.type_id));
        differs.then(|| Alert {
            title: "Multiple type effect selected".to_string(),
            message: format!(
                "Some of the selected effects are not of the same type, only effects of {} type will be modified.",
                source_entry.descriptor.map_or("Unknown", |d| d.name)
            ),
            style: AlertStyle::Warning,
            confirm: Some(AlertConfirm { label: "OK".to_string(), cancel_label: "Cancel".to_string(), action: AlertAction::CloneToSelected(source) }),
        })
    }

    /// F244: copies the clicked effect's values to the other selected effects of its type. With F244's FIX: one undo step, and modified.
    pub fn clone_to_selected(&mut self, source: EffectId) {
        let selected = self.grid.selected_elements();
        let mut session = self.session.lock();
        let Some(source_entry) = session.effect(source) else { return };
        let (type_id, params) = (source_entry.node.type_id, source_entry.node.params.clone());
        let mut before = Vec::new();
        for id in selected.iter().filter(|id| **id != source) {
            if let Some(entry) = session.effect(*id) {
                if entry.node.type_id == type_id {
                    before.push((*id, entry.node.params.clone()));
                }
            }
        }
        for (id, _) in &before {
            session.set_params(*id, params.clone());
        }
        if !before.is_empty() {
            session.record_undo(UndoAction::ParamsChanged { label: "properties".to_string(), params: before });
            session.render_all_dirty();
        }
    }

    /// F245's dialog values for the clicked effect.
    pub fn edit_time_values(&self, clicked: EffectId) -> Option<EffectTimeEditorState> {
        let session = self.session.lock();
        let entry = session.effect(clicked)?;
        let minimum = TimeSpan(self.grid.time_per_pixel().0 * MINIMUM_ELEMENT_WIDTH_PX);
        Some(EffectTimeEditorState::new(entry.node.start, entry.node.time_span, session.length(), minimum))
    }

    /// `ApplyColorCollection`: F253. Sequential runs one index across every effect and property. One undo step per effect. An effect with discrete colours and none of the collection's is
    /// skipped, and an effect with two colour properties keeps its first old value, where upstream crashes on both (GUARD). Modified is set at the end whatever happened (KEEP).
    pub fn apply_color_collection(&mut self, name: &str, random: bool) -> CommandOutcome {
        let Some(collection) = self.color_collections.iter().find(|c| c.name == name).cloned() else { return CommandOutcome::handled() };
        if collection.colors.is_empty() {
            return CommandOutcome::handled();
        }
        let selected = self.grid.selected_elements();
        let session_handle = Arc::clone(&self.session);
        let mut session = session_handle.lock();
        let mut rng = vixen_effects::framework::Rand::default();
        let mut index = 0usize;
        let mut skipped = false;
        for id in selected {
            let Some(entry) = session.effect(id) else { continue };
            let Some(descriptor) = entry.descriptor else {
                skipped = true;
                continue;
            };
            let Some(info) = session.target_info(id) else { continue };
            let props = collect_props(descriptor, &entry.node.params, &info, ALL_COLOR_TYPES);
            if props.is_empty() {
                skipped = true;
                continue;
            }
            let discrete = info.valid_colors();
            let colors: Vec<Color> = if discrete.is_empty() {
                collection.colors.clone()
            } else {
                discrete.into_iter().filter(|d| collection.colors.iter().any(|c| c.r == d.r && c.g == d.g && c.b == d.b)).collect()
            };
            if colors.is_empty() {
                continue;
            }
            let before = entry.node.params.clone();
            let mut params = before.clone();
            let mut next = |index: &mut usize| -> Color {
                if random {
                    colors[rng.next_max(colors.len() as i32) as usize]
                } else {
                    let color = colors[*index % colors.len()];
                    *index += 1;
                    color
                }
            };
            for prop in &props {
                let Some(current) = prop.value(&params).cloned() else { continue };
                let updated = match (prop.kind, current) {
                    (PropType::Color, ParamValue::Rgb(_)) => ParamValue::Rgb(Rgb::from_color(next(&mut index))),
                    (PropType::Color, _) => ParamValue::Color(next(&mut index)),
                    (PropType::Gradient, _) => ParamValue::Gradient(ColorGradient::solid(next(&mut index))),
                    (PropType::GradientList, ParamValue::GradientList(list)) => {
                        ParamValue::GradientList(list.iter().map(|_| ColorGradient::solid(next(&mut index))).collect())
                    }
                    (PropType::GradientLevels, ParamValue::GradientLevels(list)) => ParamValue::GradientLevels(
                        list.into_iter().map(|mut pair| {
                            pair.color_gradient = ColorGradient::solid(next(&mut index));
                            pair
                        }).collect(),
                    ),
                    _ => continue,
                };
                if prop.owner.is_some() {
                    set_nested(&mut params, prop, updated);
                } else {
                    params.set(prop.name, updated);
                }
            }
            session.set_params(id, params);
            session.record_undo(UndoAction::ParamsChanged { label: "Color".to_string(), params: vec![(id, before)] });
        }
        session.set_modified();
        session.render_all_dirty();
        if skipped {
            return CommandOutcome {
                alert: Some(Alert {
                    title: "Information".to_string(),
                    message: "One or more effects were selected that do not support colors.\nAll effects that do were updated.".to_string(),
                    style: AlertStyle::Information,
                    confirm: None,
                }),
                handled: true,
                ..CommandOutcome::default()
            };
        }
        CommandOutcome::handled()
    }

    // ------------------------------------------------------------------------------------------------------------------
    // The Layers panel: F330 to F340
    // ------------------------------------------------------------------------------------------------------------------

    /// Runs a change to the layers. `f` says whether anything changed; if so the mix follows and the sequence is modified.
    /// Renaming, Quick rename and the Type combo count too, which upstream doesn't watch and loses at close (FIX F330).
    pub fn edit_layers<R>(&mut self, f: impl FnOnce(&mut SequenceLayers) -> (R, bool)) -> R {
        let mut session = self.session.lock();
        let (result, changed) = f(&mut session.document_mut().layers);
        if changed {
            session.layers_changed();
            session.set_modified();
        }
        result
    }

    pub fn layers_snapshot(&self) -> SequenceLayers {
        self.session.lock().document().layers.clone()
    }

    /// The Configuration dialog changed a layer's filter behind this state's back.
    pub fn note_layer_configured(&mut self) {
        let mut session = self.session.lock();
        session.layers_changed();
        session.set_modified();
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Library panels
    // ------------------------------------------------------------------------------------------------------------------

    /// `%AppData%/Vixen`: `ColorPalette.xml` and `ColorCollections.xml` live here.
    pub fn library_dir(&self) -> &Path {
        &self.settings_dir
    }

    pub fn panel_scale(&self, key: &str) -> f64 {
        self.settings.get_f64(vixen_io::SettingSection::AppSettings, key, 1.0)
    }

    pub fn set_panel_scale(&self, key: &str, value: f64) {
        let _ = self.settings.put_f64(vixen_io::SettingSection::AppSettings, key, value);
    }

    /// F86's OK: edits are kept and written, and F253's menu follows.
    pub fn set_color_collections(&mut self, list: Vec<ColorCollection>) -> Result<(), IoError> {
        self.color_collections = list;
        save_color_collections(&self.settings_dir, &self.color_collections)
    }

    /// F360 and F363's Import: existing names get ` 2`, ` 3` and so on with no prompt.
    pub fn import_curve_library(&self, path: &Path) -> Result<usize, IoError> {
        let items = read_vcl(path)?;
        let mut names: Vec<String> = self.core.library_curves().curves.iter().map(|(n, _)| n.clone()).collect();
        for (name, curve) in &items {
            let unique = unique_import_name(&names, name);
            self.core.add_library_curve(&unique, curve.clone());
            names.push(unique);
        }
        save_libraries(&self.core);
        Ok(items.len())
    }

    pub fn export_curve_library(&self, path: &Path) -> Result<(), IoError> {
        write_vcl(path, &self.core.library_curves().curves)
    }

    pub fn import_gradient_library(&self, path: &Path) -> Result<usize, IoError> {
        let items = read_vgl(path)?;
        let mut names: Vec<String> = self.core.library_gradients().gradients.iter().map(|(n, _)| n.clone()).collect();
        for (name, gradient) in &items {
            let unique = unique_import_name(&names, name);
            self.core.add_library_gradient(&unique, gradient.clone());
            names.push(unique);
        }
        save_libraries(&self.core);
        Ok(items.len())
    }

    pub fn export_gradient_library(&self, path: &Path) -> Result<(), IoError> {
        write_vgl(path, &self.core.library_gradients().gradients)
    }

    // ------------------------------------------------------------------------------------------------------------------
    // LipSync default image map: F87
    // ------------------------------------------------------------------------------------------------------------------

    /// The Default Image Map submenu: every map in library order, with the one acting as default flagged. With none flagged that is the
    /// first.
    pub fn lipsync_maps(&self) -> Vec<(String, bool)> {
        let library = LipSyncLibrary::from_store(&self.core.system.read().module_store);
        let default = library.default_name();
        library
            .names()
            .into_iter()
            .map(|name| {
                let is_default = default.as_deref() == Some(name.as_str());
                (name, is_default)
            })
            .collect()
    }

    /// Choosing a map other than the default flags it alone and marks the sequence modified, though the flag lives in the module store.
    /// A sequence save never writes that store, so upstream loses the choice at exit; here it's saved at once (FIX, marks F89).
    pub fn set_default_lipsync_map(&mut self, name: &str) -> bool {
        {
            let mut system = self.core.system.write();
            let mut library = LipSyncLibrary::from_store(&system.module_store);
            if library.default_name().as_deref() == Some(name) || !library.set_default(name) {
                return false;
            }
            library.write_to(&mut system.module_store);
        }
        self.session.lock().set_modified();
        save_libraries(&self.core);
        true
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Papagayo Import: F91, F404
    // ------------------------------------------------------------------------------------------------------------------

    /// Each phoneme with a duration becomes a LipSync effect on the clipboard, a row per voice. The user then pastes. A bad file shows the
    /// mark import's error alert, where upstream's handler doesn't catch it (GUARD, marks F88).
    pub fn papagayo_import(&mut self, path: &Path) -> CommandOutcome {
        let failed = |detail: String| {
            let message = "There was an error importing the Papagayo marks.";
            crate::logging::error("TimedSequenceEditorForm", message, Some(&detail));
            CommandOutcome {
                alert: Some(Alert { title: "Papagayo Import Error".to_string(), message: message.to_string(), style: AlertStyle::Error, confirm: None }),
                handled: true,
                ..CommandOutcome::default()
            }
        };
        let text = match std::fs::read(path) {
            Ok(bytes) => vixen_io::marks_files::read_import_text(&bytes),
            Err(e) => return failed(e.to_string()),
        };
        let doc = match parse_papagayo(&text) {
            Ok(doc) => doc,
            Err(e) => return failed(e),
        };
        let Some(descriptor) = vixen_effect_registry::all().iter().copied().find(|d| d.name == "LipSync") else {
            return failed("The LipSync effect isn't available.".to_string());
        };
        let mut data = ClipboardData { effects: Vec::new(), first_visible_row: 0, earliest_start: TimeSpan(i64::MAX) };
        let mut any = false;
        for (row, voice) in doc.voices.iter().enumerate() {
            any |= !voice.phonemes.is_empty();
            for phoneme in voice.phonemes.iter().filter(|p| p.duration_ms != 0.0) {
                let mut params = descriptor.new_params();
                // The two enums differ in case: `phoneme.TypeName.ToUpper()`.
                params.set("StaticPhoneme", ParamValue::Enum(phoneme.phoneme.name().to_uppercase()));
                params.set("LyricData", ParamValue::Str(phoneme.word.clone()));
                let start = TimeSpan::from_ms(phoneme.start_ms as f64);
                let Some(text) = clipboard::effect_data_text(&descriptor.type_id, &Uuid::new_v4(), &params) else { continue };
                data.effects.push(ClipboardEffect {
                    type_id: descriptor.type_id,
                    start,
                    // The subtraction is in single precision, as upstream's is.
                    duration: TimeSpan::from_ms((phoneme.duration_ms - 1.0) as f64),
                    layer_id: Uuid::nil(),
                    layer_name: String::new(),
                    layer_type_id: Uuid::nil(),
                    data: text,
                    row_offset: row as i32,
                });
                if start < data.earliest_start {
                    data.earliest_start = start;
                }
            }
        }
        if any {
            self.session.lock().set_modified();
        }
        let names: Vec<String> = doc.voices.iter().map(|v| v.name.clone()).collect();
        CommandOutcome {
            alert: Some(Alert { title: "Papagayo Import".to_string(), message: import_summary(&names), style: AlertStyle::Information, confirm: None }),
            clipboard: any.then_some(data),
            handled: true,
            ..CommandOutcome::default()
        }
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Audio Devices: F132, F283
    // ------------------------------------------------------------------------------------------------------------------

    /// `PopulateAudioDropdown`, rebuilt each opening. The saved id is the device's name here. If none matches, the first is checked and made preferred, which on the Mac is the system default row.
    pub fn audio_devices(&self) -> Vec<AudioDeviceEntry> {
        let mut names = vec![(SYSTEM_DEFAULT_DEVICE_ID.to_string(), SYSTEM_DEFAULT_DEVICE_NAME.to_string())];
        names.extend(vixen_audio::output_devices().into_iter().map(|n| (n.clone(), n)));
        let preferences = vixen_io::load_audio_preferences(&self.settings_dir);
        let found = names.iter().any(|(id, _)| *id == preferences.sound_out_device_id);
        if !found {
            self.set_audio_device(SYSTEM_DEFAULT_DEVICE_ID);
        }
        let chosen = if found { preferences.sound_out_device_id } else { SYSTEM_DEFAULT_DEVICE_ID.to_string() };
        names.into_iter().map(|(id, name)| AudioDeviceEntry { checked: id == chosen, id, name }).collect()
    }

    /// The click. Upstream only moves the check while no audio is loaded (FIX F132): the manager's device is set directly.
    pub fn set_audio_device(&self, id: &str) {
        let mut preferences = vixen_io::load_audio_preferences(&self.settings_dir);
        preferences.sound_out_device_id = id.to_string();
        let _ = vixen_io::save_audio_preferences(&preferences, &self.settings_dir);
        self.core.playback.set_audio_device(if id == SYSTEM_DEFAULT_DEVICE_ID { None } else { Some(id) });
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Effect Defaults: F93 to F95, F403
    // ------------------------------------------------------------------------------------------------------------------

    /// `GetSummaries`, sorted by effect name for F403's list: `(type id, effect name)`.
    pub fn effect_default_summaries(&self) -> Vec<(Uuid, String)> {
        let store = self.core.effect_defaults();
        let store = store.lock();
        let mut list: Vec<(Uuid, String)> = store.entries().iter().map(|e| (e.type_id, e.effect_name.clone())).collect();
        list.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        list
    }

    pub fn export_effect_defaults(&self, path: &Path, type_ids: &[Uuid]) -> Result<(), IoError> {
        self.core.effect_defaults().lock().export(path, type_ids)
    }

    /// `(new, updated)`.
    pub fn import_effect_defaults(&self, path: &Path) -> Result<(usize, usize), IoError> {
        self.core.effect_defaults().lock().import(path)
    }

    pub fn dump_effect_defaults(&self, path: &Path) -> Result<(), IoError> {
        self.core.effect_defaults().lock().write_diagnostic_dump(path)
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Find Effects/Layers: F321 to F327
    // ------------------------------------------------------------------------------------------------------------------

    pub fn find_available(&self, mode: FindMode) -> Vec<String> {
        find::available_names(&self.session.lock(), mode)
    }

    pub fn find_rows(&self, mode: FindMode, search: &str) -> Vec<FindRow> {
        find::find_rows(&self.session.lock(), mode, search)
    }

    /// `DisplaySelectedEffects`.
    pub fn find_display(&mut self, elements: &[EffectId], collapse_groups: bool) {
        if collapse_groups {
            self.grid.collapse_all();
        }
        self.grid.set_selection(&[]);
        self.grid.display_selected(elements);
    }

    /// F245's OK: all selected go to that same start and duration, otherwise just the clicked one.
    pub fn apply_edit_time(&mut self, clicked: EffectId, start: TimeSpan, duration: TimeSpan) {
        let selected = self.grid.selected_elements();
        let list = if selected.is_empty() { vec![clicked] } else { selected };
        let moves: Vec<(EffectId, TimeSpan, TimeSpan)> = list.into_iter().map(|id| (id, start, start + duration)).collect();
        self.move_resize(&moves, MoveKind::Moved);
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Marks: F82, F92, F296, F297
    // ------------------------------------------------------------------------------------------------------------------

    /// F296's warning when nothing's selected, then F297 for each effect, as one undo step.
    pub fn add_marks_to_effects(&mut self, confirmed: bool) -> CommandOutcome {
        let selected = self.grid.selected_elements();
        if selected.is_empty() && !confirmed {
            return CommandOutcome {
                alert: Some(Alert {
                    title: "Align effects to marks".to_string(),
                    message: NO_SELECTION_WARNING.to_string(),
                    style: AlertStyle::Information,
                    confirm: Some(AlertConfirm {
                        label: "Apply to Entire Sequence".to_string(),
                        cancel_label: "Cancel".to_string(),
                        action: AlertAction::AddMarksEverywhere,
                    }),
                }),
                handled: true,
                ..CommandOutcome::default()
            };
        }
        let mut session = self.session.lock();
        let starts: Vec<TimeSpan> = if selected.is_empty() {
            session.effects().map(|e| e.node.start).collect()
        } else {
            selected.iter().filter_map(|id| session.effect(*id).map(|e| e.node.start)).collect()
        };
        let mut marks = session.marks().to_vec();
        let Some(index) = marks.iter().position(|c| c.is_default && clipboard::is_collection_visible(c)) else {
            // F297's prompts for no collection or a hidden default come up through the timeline's own path. With none usable, there's nowhere for these to go.
            drop(session);
            return self.mark_collection_prompt(&marks);
        };
        let before = marks.clone();
        let mut added = 0;
        for start in starts {
            if marks[index].marks.iter().all(|m| m.start_time != start) {
                marks[index].add_mark(Mark::new(start));
                added += 1;
            }
        }
        if added > 0 {
            marks[index].sort_marks();
            session.set_marks(marks);
            session.record_undo(UndoAction::MarksChanged { label: format!("Added {added} Mark{}", if added == 1 { "" } else { "s" }), marks: before });
        }
        CommandOutcome { handled: true, ..CommandOutcome::default() }
    }

    fn mark_collection_prompt(&self, marks: &[MarkCollection]) -> CommandOutcome {
        let alert = if marks.is_empty() {
            Alert {
                title: "Create a Mark Collection".to_string(),
                message: "Marks are stored in Mark Collections. There are no mark collections available to store this mark. Would you like to create a new one?".to_string(),
                style: AlertStyle::Information,
                confirm: Some(AlertConfirm { label: "Create Collection".to_string(), cancel_label: "Cancel".to_string(), action: AlertAction::CreateDefaultMarks }),
            }
        } else {
            Alert {
                title: "New Mark".to_string(),
                message: "The active mark collection is not visible on the timeline. Would you like to enable it to add the mark?".to_string(),
                style: AlertStyle::Error,
                confirm: Some(AlertConfirm { label: "Enable and Add".to_string(), cancel_label: "Cancel".to_string(), action: AlertAction::EnableDefaultMarks }),
            }
        };
        CommandOutcome { alert: Some(alert), handled: true, ..CommandOutcome::default() }
    }

    /// F297's "Create Collection": `GetOrAddNewMarkCollection(Color.White, "Default Marks")`. The first collection is the default.
    pub fn create_default_marks(&mut self) {
        let mut session = self.session.lock();
        let mut marks = session.marks().to_vec();
        if marks.iter().any(|c| c.name.trim().eq_ignore_ascii_case("Default Marks")) {
            return;
        }
        let mut collection = MarkCollection::default();
        collection.name = crate::sequencer::lifecycle::unique_mark_collection_name("Default Marks", &marks);
        collection.is_default = marks.is_empty();
        marks.push(collection);
        session.set_marks(marks);
    }

    /// F297's "Enable and Add": shows the default collection's grid lines and bar, tail lines off. With no collection flagged default, nothing happens.
    pub fn enable_default_marks(&mut self) {
        let mut session = self.session.lock();
        let mut marks = session.marks().to_vec();
        if let Some(collection) = marks.iter_mut().find(|c| c.is_default) {
            collection.show_grid_lines = true;
            collection.show_marks = true;
            collection.show_tail_lines = false;
            session.set_marks(marks);
        }
    }

    /// `CreateEvenMarksForm`'s values: F394. Start is the remembered start or 0, end the remembered end or 0.
    pub fn even_marks_values(&self) -> (TimeSpan, TimeSpan, TimeSpan) {
        let remembered = self.grid.remembered_range().unwrap_or_else(|| self.transport.remembered());
        (remembered.start.unwrap_or(TimeSpan::ZERO), remembered.end.unwrap_or(TimeSpan::ZERO), self.session.lock().length())
    }

    /// F82's OK. The error is the message to show, and the dialog reopens. No undo (KEEP).
    pub fn create_even_marks(&mut self, start: TimeSpan, end: TimeSpan, divisions: i32) -> Result<(), &'static str> {
        let min_ms = TimeSpan(self.grid.time_per_pixel().0 * 12).total_ms();
        let mut session = self.session.lock();
        let mut marks = session.marks().to_vec();
        // Checked first, so a refused plan removes nothing.
        let names: Vec<String> = marks.iter().map(|c| c.name.clone()).collect();
        effect_ops::plan_even_marks(start, end, divisions, min_ms, &names.iter().map(String::as_str).collect::<Vec<_>>())?;
        if let Some(index) = effect_ops::matching_even_collection(&marks, start, end, divisions) {
            marks.remove(index);
        }
        let names: Vec<String> = marks.iter().map(|c| c.name.clone()).collect();
        let plan = effect_ops::plan_even_marks(start, end, divisions, min_ms, &names.iter().map(String::as_str).collect::<Vec<_>>())?;
        let mut collection = MarkCollection::default();
        collection.name = plan.name;
        for (time, duration) in plan.marks {
            let mut mark = Mark::new(time);
            mark.duration = duration;
            collection.add_mark(mark);
        }
        marks.push(collection);
        session.set_marks(marks);
        Ok(())
    }

    /// F92's OK: effects by the grid, then the marks with their own undo step, pushed even if none moved (KEEP: it reads `Bulk Change 0 Mark`).
    pub fn bulk_move(&mut self, start: TimeSpan, end: TimeSpan, offset: TimeSpan, visible_only: bool, clip: bool, process_marks: bool) {
        let applied = self.grid.move_in_range_by_time(start, end, offset, visible_only, clip);
        if !process_marks || applied == TimeSpan::ZERO {
            return;
        }
        let mut session = self.session.lock();
        let total = session.length();
        let before = session.marks().to_vec();
        let mut marks = before.clone();
        let mut changed = 0;
        for collection in marks.iter_mut().filter(|c| (!visible_only || c.show_marks) && !c.is_locked) {
            changed += offset_marks_in_range(collection, start, end, applied, total);
        }
        session.set_marks(marks);
        session.record_undo(UndoAction::MarksChanged { label: format!("Bulk Change {changed} Mark{}", if changed > 1 { "s" } else { "" }), marks: before });
    }

    // ------------------------------------------------------------------------------------------------------------------
    // Audio: F302, F303
    // ------------------------------------------------------------------------------------------------------------------

    /// F302's warning when audio's already there, as the text it shows.
    pub fn existing_audio_warning(&self) -> Option<Alert> {
        let session = self.session.lock();
        let media = session.document().media.first()?;
        let path = self.core.paths.media_dir().join(&media.file_name);
        Some(Alert {
            title: "Remove existing audio?".to_string(),
            message: format!(
                "Current audio file: {}\n\nOnly one audio file can be associated with a sequence at a time. If you choose another, the first will be removed. Continue?",
                path.display()
            ),
            style: AlertStyle::Warning,
            confirm: Some(AlertConfirm { label: "Replace Audio".to_string(), cancel_label: "Cancel".to_string(), action: AlertAction::AssociateAudio }),
        })
    }

    /// F303's confirmation. With no audio there's nothing to confirm.
    pub fn remove_audio_prompt(&self) -> Option<Alert> {
        self.session.lock().document().media.first()?;
        Some(Alert {
            title: "Remove existing audio?".to_string(),
            message: "Are you sure you want to remove the audio association?".to_string(),
            style: AlertStyle::Information,
            confirm: Some(AlertConfirm { label: "Remove Audio".to_string(), cancel_label: "Cancel".to_string(), action: AlertAction::RemoveAudio }),
        })
    }

    /// F303. The stale timing provider stays, and is saved with the file, as upstream's does. Not undoable.
    pub fn remove_audio(&mut self) {
        let mut session = self.session.lock();
        let undo = crate::studio_sequence::audio::before(&session, "Remove Audio"); // STUDIO: Remove Audio is one undo step (B3)
        session.document_mut().media.clear();
        session.set_audio_clip(None);
        session.set_modified();
        // STUDIO-BEGIN sequence-audio-record: Remove Audio on the undo stack
        let undo = crate::studio_sequence::audio::settled(undo, &session);
        session.record_undo(undo);
        // STUDIO-END sequence-audio-record
    }

    /// What `MediaService.ImportMedia` will copy over, when the profile already has a file by that name that isn't this one.
    pub fn audio_import_conflict(&self, path: &Path) -> bool {
        let target = path.file_name().map(|n| self.core.paths.media_dir().join(n));
        target.is_some_and(|t| t != path && t.exists())
    }

    /// F302 from the open panel on. `replace_existing_copy` answers "A Media file with the same name already exists".
    pub fn add_audio(&mut self, path: &Path, replace_existing_copy: bool) -> AddAudioOutcome {
        let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
            return AddAudioOutcome::Unsupported;
        }
        let Some(file_name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { return AddAudioOutcome::Unsupported };
        let media_dir = self.core.paths.media_dir();
        let target = media_dir.join(&file_name);
        // `MediaService.ImportMedia`.
        if target != path && (!target.exists() || replace_existing_copy) {
            let _ = std::fs::create_dir_all(&media_dir);
            if std::fs::copy(path, &target).is_err() {
                return AddAudioOutcome::Unsupported;
            }
        }
        let clip = vixen_audio::AudioClip::load(&target).ok().map(Arc::new);
        let length = clip.as_ref().map_or(TimeSpan::ZERO, |c| c.duration());
        let mut session = self.session.lock();
        let undo = crate::studio_sequence::audio::before(&session, crate::studio_sequence::audio::change_label(&session)); // STUDIO: one undo step (B3)
        session.document_mut().media.clear();
        session.set_audio_clip(None);
        if length == TimeSpan::ZERO {
            // F302's FIX: the old audio has gone, so it's marked modified and the controls follow.
            session.set_modified();
            // STUDIO-BEGIN sequence-audio-record-failed: the old song is gone, so Undo brings it back
            let undo = crate::studio_sequence::audio::settled(undo, &session);
            session.record_undo(undo);
            drop(session);
            self.apply_timing_enabled_state();
            // STUDIO-END sequence-audio-record-failed
            return AddAudioOutcome::Unprocessable;
        }
        let instance_id = Uuid::new_v4();
        session.document_mut().media.push(MediaEntry { type_id: AUDIO_MODULE_TYPE, instance_id, file_name: file_name.clone(), data: Some(audio_data_model(&instance_id, &target)) });
        session.set_audio_clip(clip);
        // `_UpdateTimingSourceToSelectedMedia`.
        session.document_mut().selected_timing_provider =
            vixen_io::SelectedTimingProvider { provider_type: Some("Media".to_string()), source_name: Some(file_name) };
        let resize_prompt = if session.length() == crate::sequencer::DEFAULT_SEQUENCE_LENGTH {
            session.set_length(length);
            false
        } else {
            session.length() != length
        };
        self.pending_audio_length = Some(length);
        session.set_modified();
        // STUDIO-BEGIN sequence-audio-record-added: the new song and the length it set, one undo step
        let undo = crate::studio_sequence::audio::settled(undo, &session);
        session.record_undo(undo);
        // STUDIO-END sequence-audio-record-added
        session.render_all_dirty();
        drop(session);
        self.grid.sync_from_document();
        self.apply_timing_enabled_state();
        AddAudioOutcome::Added { resize_prompt }
    }

    /// F302's "Resize Sequence".
    pub fn resize_to_audio(&mut self) {
        if let Some(length) = self.pending_audio_length.take() {
            self.set_sequence_length(length);
        }
    }

    /// F9's Select New Audio and Forget This Audio use the same two as the menu.
    pub fn supported_audio_extensions() -> &'static [&'static str] {
        AUDIO_EXTENSIONS
    }

    /// `row.Tag`'s tag test for a row by id: F226, F305.
    pub fn row_is_deprecated(&self, row: u32) -> bool {
        self.grid.row_summary(row).is_some_and(|r| r.deprecated)
    }

    /// Whether an effect's target carries the Deprecated tag.
    pub fn node_is_deprecated(&self, node: &Uuid) -> bool {
        let session = self.session.lock();
        let tree = session.tree();
        tree.find(node).is_some_and(|n| tree.node(n).tags.contains(&DEPRECATED_TAG))
    }
}

fn group_rank(group: EffectGroup) -> u8 {
    match group {
        EffectGroup::Basic => 0,
        EffectGroup::Pixel => 1,
        EffectGroup::Device => 2,
        EffectGroup::IntelligentFixture => 3,
    }
}

/// `AudioData`, as a save writes it. `FilePath` is this machine's path, which real Vixen on Windows reads as a missing file and resolves through the surrogate's name.
fn audio_data_model(instance_id: &Uuid, path: &Path) -> vixen_xml::XElement {
    const NS: &str = "http://schemas.datacontract.org/2004/07/VixenModules.Media.Audio";
    let mut model = vixen_xml::XElement::new("http://schemas.microsoft.com/2003/10/Serialization/Arrays", "anyType");
    model.xsi_type = Some((NS.to_string(), "AudioData".to_string()));
    model.push(vixen_xml::XElement::with_text("", "ModuleInstanceId", instance_id.to_string()));
    model.push(vixen_xml::XElement::with_text("", "ModuleTypeId", AUDIO_MODULE_TYPE.to_string()));
    model.push(vixen_xml::XElement::with_text(NS, "FilePath", path.to_string_lossy().into_owned()));
    model
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppCore;
    use crate::sequence::{SequenceSession, SessionEnvironment};
    use parking_lot::Mutex;
    use std::path::PathBuf;
    use std::sync::Arc;
    use vixen_core::{ElementTree, Layer, MixingFilter};
    use vixen_io::{ModuleStore, ProfileSettings};

    const SET_LEVEL: Uuid = uuid::uuid!("32cff8e0-5b10-4466-a093-0d232c55aac0");
    const PULSE: Uuid = uuid::uuid!("cbd76d3b-c924-40ff-bad6-d1437b3dbdc0");

    fn s(secs: f64) -> TimeSpan {
        TimeSpan::from_secs(secs)
    }

    fn fixture(name: &str) -> (SequencerState, Vec<(Uuid, String)>, PathBuf) {
        let root = std::env::temp_dir().join(format!("vixen-cmd-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let core = Arc::new(AppCore::open(root.clone(), true).unwrap());
        let mut tree = ElementTree::new();
        let mut nodes = Vec::new();
        for n in ["A", "B", "C"] {
            let id = tree.add_element(n);
            nodes.push((tree.node(id).id, n.to_string()));
        }
        let env = SessionEnvironment { video_cache: None,
            tree: Arc::new(tree),
            store: Arc::new(ModuleStore::new()),
            module_data_root: None,
            frame_ms: 25,
            media_dir: core.paths.media_dir(),
        };
        let session = Arc::new(Mutex::new(SequenceSession::new_untitled(&env)));
        let settings = Arc::new(ProfileSettings::open(root.join("Settings.xml")));
        let mut state = SequencerState::with_settings(core, session, settings, root.clone());
        state.finish_load();
        (state, nodes, root)
    }

    #[test]
    fn context_menu_for_nothing_selected() {
        let (state, ..) = fixture("menu");
        let menu = state.context_menu(Some(0), &[]);
        assert!(!menu.row_deprecated && menu.add_effect_enabled);
        assert_eq!(menu.add_effect_tooltip, "Use Shift key to add multiple effects of the same type.");
        assert!(menu.add_effect_groups.len() >= 3);
        assert!(!menu.alignment_enabled);
        assert_eq!(menu.alignment_tooltip, "Select more then one effect to enable the Alignment feature.");
        assert!(!menu.show_manipulation && !menu.show_edit_time && !menu.show_delete);
        assert!(!menu.cut_copy_enabled);
        assert_eq!(menu.layers, None);
    }

    #[test]
    fn context_menu_alignment_rules() {
        let (mut state, ..) = fixture("menu2");
        let a = state.add_effect_at(SET_LEVEL, 0, s(1.0)).unwrap();
        let b = state.add_effect_at(SET_LEVEL, 0, s(5.0)).unwrap();
        state.grid.set_selection(&[a]);
        // One selected, right-clicking that one: reference doesn't work, and the tooltip mentions marks.
        let menu = state.context_menu(Some(0), &[a]);
        assert!(!menu.reference_enabled && !menu.distribute_enabled && !menu.alignment_enabled);
        assert!(menu.alignment_tooltip.starts_with("Select more then one effect or ensure"));
        // One selected, right-clicking another: the clicked is the reference.
        let menu = state.context_menu(Some(0), &[b]);
        assert!(menu.reference_enabled && !menu.distribute_enabled && menu.alignment_enabled);
        state.grid.set_selection(&[a, b]);
        let menu = state.context_menu(Some(0), &[a]);
        assert!(menu.reference_enabled && menu.distribute_enabled);
        assert!(!menu.clone_to_selected_enabled, "more than 2, so 2 isn't enough");
        assert!(menu.show_manipulation && menu.show_edit_time && menu.show_delete && menu.edit_time_enabled);
    }

    #[test]
    fn marks_enable_alignment_on_their_own() {
        let (state, ..) = fixture("menu3");
        let mut collection = MarkCollection::default();
        collection.marks.push(Mark::new(s(1.0)));
        state.session.lock().set_marks(vec![collection]);
        let menu = state.context_menu(Some(0), &[]);
        assert!(menu.alignment_enabled && menu.marks_enabled);
        assert_eq!(menu.alignment_tooltip, "");
    }

    #[test]
    fn layer_menu_lists_in_reverse_and_marks_what_is_used() {
        let (mut state, ..) = fixture("layers");
        let a = state.add_effect_at(SET_LEVEL, 0, s(1.0)).unwrap();
        let top = {
            let mut session = state.session.lock();
            let layer = Layer::standard("Top", MixingFilter::HighestValue);
            let id = layer.id;
            session.document_mut().layers.add_layer(layer);
            session.layers_changed();
            id
        };
        let menu = state.context_menu(Some(0), &[a]);
        let layers = menu.layers.unwrap();
        assert_eq!(layers.items.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Default", "Top"]);
        assert!(layers.items[0].in_use && !layers.items[1].in_use);
        assert_eq!(layers.items[0].filter_name, "None");
        assert_ne!(layers.items[1].filter_name, "None");
        assert!(layers.single_in_use);

        state.assign_layer(&[a], top);
        assert_eq!(state.session.lock().layer_of(a), Some(top));
        assert_eq!(state.ui_state().undo_name.as_deref(), Some("Modified 1 effect layer"));
        // Assigning the same layer again changes nothing and adds no step.
        let before = state.session.lock().undo_stack().undo_descriptions().len();
        state.assign_layer(&[a], top);
        assert_eq!(state.session.lock().undo_stack().undo_descriptions().len(), before);
    }

    #[test]
    fn clone_makes_one_undo_step_with_the_same_place_and_layer() {
        let (mut state, ..) = fixture("clone");
        let a = state.add_effect_at(SET_LEVEL, 1, s(3.0)).unwrap();
        state.grid.set_selection(&[]);
        let clones = state.clone_elements(a);
        assert_eq!(clones.len(), 1);
        {
            let session = state.session.lock();
            let (o, c) = (session.effect(a).unwrap(), session.effect(clones[0]).unwrap());
            assert_eq!((o.node.start, o.node.time_span), (c.node.start, c.node.time_span));
            assert_eq!(o.node.targets, c.node.targets);
            assert_ne!(o.node.instance_id, c.node.instance_id);
        }
        assert_eq!(state.ui_state().undo_name.as_deref(), Some("Added 1 effect"));
    }

    #[test]
    fn clone_to_selected_copies_values_of_the_same_type_only_and_undoes() {
        let (mut state, ..) = fixture("clone2");
        let a = state.add_effect_at(SET_LEVEL, 0, s(1.0)).unwrap();
        let b = state.add_effect_at(SET_LEVEL, 0, s(4.0)).unwrap();
        let c = state.add_effect_at(PULSE, 0, s(8.0)).unwrap();
        state.grid.set_selection(&[a, b, c]);
        let prompt = state.clone_to_selected_prompt(a).unwrap();
        assert_eq!(prompt.title, "Multiple type effect selected");
        assert!(prompt.message.contains("only effects of Set Level type"));
        let mut changed = state.session.lock().effect(a).unwrap().node.params.clone();
        changed.set("IntensityLevel", vixen_effects::framework::ParamValue::Double(0.25));
        state.session.lock().set_params(a, changed.clone());
        let pulse_before = state.session.lock().effect(c).unwrap().node.params.clone();
        state.clone_to_selected(a);
        {
            let session = state.session.lock();
            assert!(session.effect(b).unwrap().node.params == changed);
            assert!(session.effect(c).unwrap().node.params == pulse_before);
            assert!(session.is_modified());
        }
        assert_eq!(state.ui_state().undo_name.as_deref(), Some("1 Effect(s) properties modified"));
        state.undo(1);
        assert!(state.session.lock().effect(b).unwrap().node.params != changed);
    }

    #[test]
    fn edit_time_moves_all_selected_to_one_place() {
        let (mut state, ..) = fixture("edit");
        let a = state.add_effect_at(SET_LEVEL, 0, s(1.0)).unwrap();
        let b = state.add_effect_at(SET_LEVEL, 1, s(4.0)).unwrap();
        state.grid.set_selection(&[a, b]);
        state.apply_edit_time(a, s(10.0), s(3.0));
        {
            let session = state.session.lock();
            for id in [a, b] {
                assert_eq!((session.effect(id).unwrap().node.start, session.effect(id).unwrap().node.time_span), (s(10.0), s(3.0)));
            }
        }
        assert_eq!(state.ui_state().undo_name.as_deref(), Some("Moved 2 effects"));
    }

    #[test]
    fn default_image_map_lists_in_library_order_and_sets_one_flag() {
        let (mut state, _, root) = fixture("lipmaps");
        // An empty store reads as the one map the module makes for itself.
        assert_eq!(state.lipsync_maps(), [("New Map".to_string(), true)]);
        {
            let mut system = state.core.system.write();
            let mut library = LipSyncLibrary::from_store(&system.module_store);
            let second = library.new_map(&root);
            library.keep_new();
            assert_ne!(second, "New Map");
            library.write_to(&mut system.module_store);
        }
        let maps = state.lipsync_maps();
        assert_eq!(maps.len(), 2);
        // Nothing is flagged yet, so the first acts as default, and choosing it changes nothing.
        assert_eq!((maps[0].0.as_str(), maps[0].1, maps[1].1), ("New Map", true, false));
        state.session.lock().set_unmodified();
        assert!(!state.set_default_lipsync_map("New Map"));
        assert!(!state.session.lock().is_modified());
        assert!(!state.set_default_lipsync_map("No such map"));
        assert!(!state.session.lock().is_modified());

        let second = maps[1].0.clone();
        assert!(state.set_default_lipsync_map(&second));
        assert!(state.session.lock().is_modified());
        let maps = state.lipsync_maps();
        assert_eq!((maps[0].1, maps[1].1), (false, true));
        // Back again: the first map's flag is written this time, and the second's cleared.
        assert!(state.set_default_lipsync_map("New Map"));
        let library = LipSyncLibrary::from_store(&state.core.system.read().module_store);
        assert!(library.get("New Map").unwrap().is_default);
        assert!(!library.get(&second).unwrap().is_default);
    }

    #[test]
    fn papagayo_makes_a_clipboard_row_per_voice() {
        let (mut state, ..) = fixture("pgo");
        let dir = std::env::temp_dir().join(format!("vixen-pgo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song.pgo");
        std::fs::write(&path, "lipsync\nx\n10\n6\n2\nA\nt\n1\nhi\n0\n5\n1\nhi 1 4 2\n1 AI\n3 etc\nB\nu\n0\n").unwrap();
        let outcome = state.papagayo_import(&path);
        let alert = outcome.alert.unwrap();
        assert_eq!(alert.title, "Papagayo Import");
        assert_eq!(alert.message, "2 Voices imported to clipboard as separate rows\n\nRow #1 - A\nRow #2 - B\n");
        let data = outcome.clipboard.unwrap();
        // Voice A: Rest, AI, etc, Rest. Voice B: one Rest, on its own row.
        assert_eq!(data.effects.len(), 5);
        assert_eq!(data.effects[1].row_offset, 0);
        assert_eq!(data.effects[4].row_offset, 1);
        assert_eq!(data.effects[1].start, s(0.1));
        // 200 ms less the millisecond upstream takes off.
        assert_eq!(data.effects[1].duration, TimeSpan::from_ms(199.0));
        assert_eq!(data.earliest_start, TimeSpan::ZERO);
        let params = clipboard::effect_data_from_text(&data.effects[2].type_id, &data.effects[2].data).unwrap();
        assert_eq!(params.get("StaticPhoneme"), &ParamValue::Enum("ETC".to_string()));
        assert_eq!(params.get("LyricData"), &ParamValue::Str("hi".to_string()));
        assert!(state.session.lock().is_modified());
        std::fs::write(&path, "nope").unwrap();
        let alert = state.papagayo_import(&path).alert.unwrap();
        assert_eq!((alert.title.as_str(), alert.message.as_str()), ("Papagayo Import Error", "There was an error importing the Papagayo marks."));
    }

    #[test]
    fn replacing_takes_two_undo_steps() {
        let (mut state, ..) = fixture("replace");
        let a = state.add_effect_at(SET_LEVEL, 0, s(1.0)).unwrap();
        state.grid.set_selection(&[a]);
        assert_eq!(state.replace_prompt(PULSE).message, "This action will replace 1 effects, are you sure ?");
        let added = state.replace_selected(PULSE);
        assert_eq!(added.len(), 1);
        let session = state.session.lock();
        assert!(session.effect(a).is_none());
        assert_eq!(session.effect(added[0]).unwrap().node.type_id, PULSE);
        assert_eq!(session.effect(added[0]).unwrap().node.start, s(1.0));
        assert_eq!(session.undo_stack().undo_descriptions()[..2], ["Added 1 effect", "Removed 1 effect"]);
    }

    #[test]
    fn add_multiple_places_and_selects_when_asked() {
        let _memory = crate::sequencer::dialogs::clear_add_multiple_memory();
        let (mut state, ..) = fixture("multi");
        let mut dialog = state.add_multiple_open("Set Level", s(0.0));
        assert_eq!(dialog.count, 15);
        dialog.count = 3;
        dialog.select_effects = true;
        assert_eq!(state.add_multiple_apply(SET_LEVEL, 0, &dialog), 3);
        let ui = state.ui_state();
        assert_eq!(ui.selection_count, 3);
        assert_eq!(ui.undo_name.as_deref(), Some("Added 3 effects"));
    }

    #[test]
    fn add_on_marks_uses_checked_collections() {
        let _memory = crate::sequencer::dialogs::clear_add_multiple_memory();
        let (mut state, ..) = fixture("multi2");
        let mut collection = MarkCollection::default();
        collection.name = "Beats".to_string();
        collection.marks = vec![Mark::new(s(1.0)), Mark::new(s(2.0)), Mark::new(s(3.0))];
        state.session.lock().set_marks(vec![collection.clone()]);
        let marks = vec![collection];
        let mut dialog = state.add_multiple_open("Set Level", s(0.0));
        dialog.set_align(true, &marks);
        // No check: no times, so no step.
        let steps = state.session.lock().undo_stack().undo_descriptions().len();
        assert_eq!(state.add_multiple_apply(SET_LEVEL, 0, &dialog), 0);
        assert_eq!(state.session.lock().undo_stack().undo_descriptions().len(), steps);
        dialog.set_mark_checked("Beats", true, &marks);
        dialog.set_fill(true, &marks);
        assert_eq!(state.add_multiple_apply(SET_LEVEL, 0, &dialog), 2);
    }

    #[test]
    fn marks_go_on_the_default_visible_collection_as_one_step() {
        let (mut state, ..) = fixture("marks");
        let a = state.add_effect_at(SET_LEVEL, 0, s(1.0)).unwrap();
        let b = state.add_effect_at(SET_LEVEL, 0, s(4.0)).unwrap();
        // No collections: the prompt to make one.
        state.grid.set_selection(&[a, b]);
        let outcome = state.add_marks_to_effects(false);
        assert_eq!(outcome.alert.unwrap().confirm.unwrap().action, AlertAction::CreateDefaultMarks);
        state.create_default_marks();
        let outcome = state.add_marks_to_effects(false);
        assert!(outcome.alert.is_none());
        {
            let session = state.session.lock();
            assert_eq!(session.marks()[0].name, "Default Marks");
            assert_eq!(session.marks()[0].marks.len(), 2);
        }
        assert_eq!(state.ui_state().undo_name.as_deref(), Some("Added 2 Marks"));
        // Already there: nothing added and no step.
        let steps = state.session.lock().undo_stack().undo_descriptions().len();
        state.add_marks_to_effects(false);
        assert_eq!(state.session.lock().undo_stack().undo_descriptions().len(), steps);
        // Nothing selected: asks first.
        state.grid.set_selection(&[]);
        let outcome = state.add_marks_to_effects(false);
        assert_eq!(outcome.alert.unwrap().confirm.unwrap().action, AlertAction::AddMarksEverywhere);
    }

    #[test]
    fn hidden_default_collection_asks_to_enable() {
        let (mut state, ..) = fixture("marks2");
        let a = state.add_effect_at(SET_LEVEL, 0, s(1.0)).unwrap();
        let mut collection = MarkCollection::default();
        collection.is_default = true;
        collection.show_grid_lines = false;
        collection.show_marks = false;
        state.session.lock().set_marks(vec![collection]);
        state.grid.set_selection(&[a]);
        let outcome = state.add_marks_to_effects(false);
        let alert = outcome.alert.unwrap();
        assert_eq!(alert.title, "New Mark");
        state.enable_default_marks();
        assert!(state.add_marks_to_effects(false).alert.is_none());
        let session = state.session.lock();
        assert_eq!(session.marks()[0].marks.len(), 1);
        assert!(session.marks()[0].show_marks && !session.marks()[0].show_tail_lines);
    }

    #[test]
    fn even_marks_replace_an_exact_match_with_no_undo() {
        let (mut state, ..) = fixture("even");
        state.create_even_marks(s(0.0), s(8.0), 4).unwrap();
        state.create_even_marks(s(0.0), s(8.0), 4).unwrap();
        {
            let session = state.session.lock();
            assert_eq!(session.marks().len(), 1);
            assert_eq!(session.marks()[0].name, "4 Divisions");
            assert_eq!(session.marks()[0].marks.len(), 5);
            assert_eq!(session.marks()[0].marks[0].duration, s(1.6));
        }
        state.create_even_marks(s(0.0), s(16.0), 4).unwrap();
        assert_eq!(state.session.lock().marks()[1].name, "4 Divisions #2");
        assert_eq!(state.ui_state().undo_name, None);
        // Too many for the zoom: refused, and nothing removed.
        assert!(state.create_even_marks(s(0.0), s(8.0), 8000).is_err());
        assert_eq!(state.session.lock().marks().len(), 2);
    }

    #[test]
    fn audio_association_rules() {
        let (mut state, _, root) = fixture("audio");
        let dir = root.join("incoming");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(state.add_audio(&dir.join("notes.txt"), false), AddAudioOutcome::Unsupported);
        // A file that won't decode counts as zero length.
        let broken = dir.join("broken.mp3");
        std::fs::write(&broken, b"not audio").unwrap();
        assert_eq!(state.add_audio(&broken, false), AddAudioOutcome::Unprocessable);
        assert!(state.session.lock().is_modified());
        assert!(!state.audio_ui().remove_enabled);
        // Importing copied it into the profile's folder, and a second one with the same name is a conflict.
        assert!(state.audio_import_conflict(&broken));
        assert!(!state.audio_import_conflict(&state.core.paths.media_dir().join("broken.mp3")));
    }

    #[test]
    fn removing_audio_needs_audio_and_keeps_the_provider() {
        let (mut state, ..) = fixture("audio2");
        assert_eq!(state.remove_audio_prompt(), None);
        {
            let mut session = state.session.lock();
            session.document_mut().media.push(MediaEntry { type_id: AUDIO_MODULE_TYPE, instance_id: Uuid::new_v4(), file_name: "a.mp3".to_string(), data: None });
            session.document_mut().selected_timing_provider =
                vixen_io::SelectedTimingProvider { provider_type: Some("Media".to_string()), source_name: Some("a.mp3".to_string()) };
        }
        assert_eq!(state.remove_audio_prompt().unwrap().message, "Are you sure you want to remove the audio association?");
        assert!(state.existing_audio_warning().unwrap().message.starts_with("Current audio file: "));
        state.remove_audio();
        let session = state.session.lock();
        assert!(session.document().media.is_empty());
        assert_eq!(session.document().selected_timing_provider.source_name.as_deref(), Some("a.mp3"));
        assert!(session.is_modified());
    }

    #[test]
    fn extension_list_matches_the_spec() {
        assert!(SequencerState::supported_audio_extensions().contains(&"mp3"));
        assert!(SequencerState::supported_audio_extensions().contains(&"ogg"));
        assert_eq!(SequencerState::supported_audio_extensions().len(), 32);
    }

    #[test]
    fn bulk_move_with_marks_uses_the_grid_offset() {
        let (mut state, ..) = fixture("bulk");
        let mut collection = MarkCollection::default();
        collection.marks = vec![Mark::new(s(1.0)), Mark::new(s(9.0))];
        let mut locked = MarkCollection::default();
        locked.is_locked = true;
        locked.marks = vec![Mark::new(s(1.0))];
        state.session.lock().set_marks(vec![collection, locked]);
        // The plain grid takes the offset as given.
        state.bulk_move(s(0.0), s(5.0), s(2.0), false, false, true);
        {
            let session = state.session.lock();
            assert_eq!(session.marks()[0].marks[0].start_time, s(3.0));
            assert_eq!(session.marks()[0].marks[1].start_time, s(9.0));
            assert_eq!(session.marks()[1].marks[0].start_time, s(1.0));
        }
        assert_eq!(state.ui_state().undo_name.as_deref(), Some("Bulk Change 1 Mark"));
    }
}
