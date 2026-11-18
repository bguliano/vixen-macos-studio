//! OK and Cancel: F6 and F7. The working copy reaches the system, the disk and the running show in one step, or is dropped.

use super::flow::CompKind;
use super::tree::Nid;
use super::SetupSession;
use crate::app::{AppCore, Saving};
use crate::sequence::SequenceSession;
use crate::SessionError;
use parking_lot::Mutex;
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use uuid::Uuid;
use vixen_core::element::property_ids;
use vixen_engine::schedule::ElementMap;
use vixen_io::system::effective_store;
use vixen_io::{Channel, ModuleEntry, OutputFilter, SystemData};
use vixen_output::ids::{filter_descriptor, module_element};

/// What depends on the display, and how it hears about a change. Part of `AppCore`.
#[derive(Default)]
pub struct DisplayHub {
    revision: AtomicU64,
    sessions: Mutex<Vec<Weak<Mutex<SequenceSession>>>>,
}

impl DisplayHub {
    pub(crate) fn tag_colors_changed(&self) {
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    /// Bumps each time Display Setup's OK changes the display, for a window that keeps anything derived from the tree.
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }
}

impl AppCore {
    /// An open sequence registers here, so it hears when the display changes under it.
    pub fn register_sequence_session(&self, session: &Arc<Mutex<SequenceSession>>) {
        // A LipSync effect naming an image map the library doesn't have draws the default one: see `marks::map_library`.
        session.lock().resolve_lip_sync_mappings(&self.lip_sync_library());
        self.apply_lip_sync_migration(&session.lock().document().migrated_matrix_maps);
        let mut sessions = self.display.sessions.lock();
        sessions.retain(|s| s.strong_count() > 0);
        sessions.push(Arc::downgrade(session));
    }

    /// The sequences open now.
    pub fn open_sequence_sessions(&self) -> Vec<Arc<Mutex<SequenceSession>>> {
        self.display.sessions.lock().iter().filter_map(Weak::upgrade).collect()
    }

    /// Swaps in a changed display: the system everyone reads, the running output graph and controllers, and every open sequence, which re-resolves its targets and renders again.
    /// What's playing stops, because its schedule is built against the old elements. Controllers keep running unless their own configuration or output count changed.
    pub fn apply_display(&self, system: SystemData) {
        let tree = Arc::new(system.tree.clone());
        let elements = Arc::new(ElementMap::from_tree(&tree));
        self.playback.replace_display(&system, elements);
        *self.system.write() = system;
        self.display.revision.fetch_add(1, Ordering::Relaxed);
        let environment = self.session_environment();
        let sessions: Vec<Arc<Mutex<SequenceSession>>> = {
            let mut list = self.display.sessions.lock();
            list.retain(|s| s.strong_count() > 0);
            list.iter().filter_map(Weak::upgrade).collect()
        };
        for session in sessions {
            session.lock().display_changed(&environment);
        }
    }
}

impl SetupSession {
    /// `ApplyConfirmedChanges`: orphaned filters and orphaned property data go. Filters that were left with no source go with the filters hanging off them.
    fn apply_confirmed_changes(&mut self) {
        let orphans = self.flow.orphaned_filter_chains();
        if !orphans.is_empty() {
            for c in orphans {
                // Their outputs' patches go with them, which is what makes "keep the filters" on unpatching only a postponement (F63).
                self.remove_filter(c);
            }
            self.patching_changed();
        }
        // `RemoveOrphanedProperties`: Color and Location data not on any current leaf. A group keeps the property itself, which comes back with its defaults: that is
        // what the next load would give it upstream, where the data in memory lasts until then.
        let on_leaves: HashSet<Uuid> = self
            .tree
            .live_nodes()
            .filter(|(_, n)| n.children.is_empty())
            .flat_map(|(_, n)| n.properties.iter().map(|p| p.instance_id))
            .collect();
        let orphaned: Vec<(Uuid, Uuid)> = self
            .system
            .module_store
            .instance_entries()
            .iter()
            .filter_map(|e| Some((e.module_type()?, e.module_instance()?)))
            .filter(|(t, i)| (*t == property_ids::COLOR || *t == property_ids::LOCATION) && !on_leaves.contains(i))
            .collect();
        let mut gone: HashSet<Uuid> = orphaned.iter().map(|(_, i)| *i).collect();
        // A group's, too, when its data isn't in the store yet: upstream's `Properties.Add` puts it there at once.
        let groups: Vec<Nid> = self.tree.live_nodes().filter(|(_, n)| !n.children.is_empty()).map(|(id, _)| id).collect();
        let mut extra = Vec::new();
        for id in groups {
            for p in &mut self.tree.node_mut(id).properties {
                if p.type_id != property_ids::COLOR && p.type_id != property_ids::LOCATION {
                    continue;
                }
                if gone.insert(p.instance_id) {
                    extra.push((p.type_id, p.instance_id));
                }
                let default = vixen_io::system::decode_property(&p.type_id, None);
                if p.data != default {
                    p.data = default;
                    self.dirty.tree = true;
                }
            }
        }
        self.group_properties_dropped = gone;
        self.removed_properties.extend(orphaned);
        self.removed_properties.extend(extra);
    }

    /// Brings `self.system` up to date, part by part. A part that hasn't changed isn't touched, so its bytes stay what the file had.
    // STUDIO-BEGIN display-materialize: materialize a candidate transaction without saving or hardware side effects
    pub(crate) fn studio_materialize(&mut self) { self.write_working_parts(); }
    // STUDIO-END display-materialize
    fn write_working_parts(&mut self) {
        // Everything on the removal lists goes in one pass, unless it's still in use: one store rebuild rather than one per entry.
        let mut remove: HashSet<(Uuid, Uuid)> = HashSet::new();
        remove.extend(self.removed_properties.drain(..));
        remove.extend(self.removed_filter_data.drain(..));
        remove.extend(self.removed_controller_data.drain(..));
        for (_, n) in self.tree.live_nodes() {
            for p in &n.properties {
                if !self.group_properties_dropped.contains(&p.instance_id) {
                    remove.remove(&(p.type_id, p.instance_id));
                }
            }
        }
        for f in self.filters.iter().filter(|f| f.alive) {
            remove.remove(&(f.type_id, f.instance_id));
        }
        self.system.module_store.remove_instances(&remove);

        if self.dirty.filters {
            self.system.filters =
                self.filters.iter().filter(|f| f.alive).map(|f| OutputFilter { type_id: f.type_id, instance_id: f.instance_id }).collect();
            for f in self.filters.iter_mut().filter(|f| f.alive && f.changed) {
                let (Some(config), Some(descriptor)) = (&f.config, filter_descriptor(&f.type_id)) else { continue };
                let element = module_element(descriptor, config.ids(), config.to_xml());
                self.system.module_store.set_instance(ModuleEntry { element });
                f.changed = false;
            }
        }

        if self.dirty.flow {
            self.system.patches = self.flow.to_patches();
        }

        if self.dirty.controllers {
            self.system.controllers = self.controller_list();
            for entry in self.controller_store_entries() {
                self.system.module_store.set_instance(entry);
            }
        }

        if self.dirty.run_state || self.dirty.controllers {
            // `SaveDisabledDevices`, keeping the list's order and whatever else is on it, such as previews.
            let mut list = std::mem::take(&mut self.system.disabled_devices);
            let gone: HashSet<Uuid> = self.controllers.iter().filter(|c| !c.alive).map(|c| c.id).collect();
            let running: HashSet<Uuid> = self.controllers.iter().filter(|c| c.alive && c.running).map(|c| c.id).collect();
            list.retain(|id| !gone.contains(id) && !running.contains(id));
            for c in self.controllers.iter().filter(|c| c.alive && !c.running) {
                if !list.contains(&c.id) {
                    list.push(c.id);
                }
            }
            self.system.disabled_devices = list;
        }

        if self.dirty.tree || self.dirty.flow {
            let color_sets = std::mem::take(&mut self.system.tree.color_sets);
            self.system.tree = self.tree.to_element_tree(color_sets);
            let dropped = std::mem::take(&mut self.group_properties_dropped);
            vixen_io::system::ensure_property_entries(&mut self.system, &dropped);

            // `Channels`: the file's order, then new elements.
            let mut channels: Vec<Channel> = std::mem::take(&mut self.system.channels);
            channels.retain(|c| self.tree.find_by_element(&c.id).is_some() || self.nodeless_channels.contains(&c.id));
            let known: HashSet<Uuid> = channels.iter().map(|c| c.id).collect();
            for (_, comp) in self.flow.components() {
                if comp.kind == CompKind::Element && !known.contains(&comp.id) {
                    if let Some(n) = self.tree.find_by_element(&comp.id) {
                        channels.push(Channel { id: comp.id, name: self.tree.node(n).name.clone() });
                    }
                }
            }
            self.system.channels = channels;
        }

        // `ElementNode`'s constructor and its `Name` setter keep an element's name equal to its node's, so any save writes the node's name, whatever the file had.
        for c in &mut self.system.channels {
            if let Some(n) = self.tree.find_by_element(&c.id) {
                let name = &self.tree.node(n).name;
                if c.name != *name {
                    c.name.clone_from(name);
                }
            }
        }

        // On any save upstream writes the seeded tags too, so a file from before tags gets them.
        self.system.tags.clone_from(&self.tags);

        // Property edits go into the store now, so later readers don't redo the comparison.
        if let Cow::Owned(store) = effective_store(&self.system) {
            self.system.module_store = store;
        }
    }

    /// F6. `progress` gets upstream's percentages and texts. Nothing has changed: nothing is written, the running show isn't touched, and both files stay as they are. On an error the
    /// session stays open, so the user can try again or Cancel.
    pub fn commit(&mut self, core: &AppCore, progress: impl Fn(u32, &str)) -> Result<bool, SessionError> {
        if !self.is_dirty() {
            return Ok(false);
        }
        progress(0, "Preparing Display Setup Changes");
        self.apply_confirmed_changes();
        self.write_working_parts();
        progress(50, "Saving System Configuration");
        {
            let _saving = Saving::begin(&core.saving_system);
            vixen_io::system::save_system_config_file(&self.system, &core.paths)?;
        }
        progress(100, "Saving Module Configuration");
        {
            let _saving = Saving::begin(&core.saving_module);
            vixen_io::system::save_module_config_file(&self.system, &core.paths)?;
        }
        core.apply_display(self.system.clone());
        Ok(true)
    }

    /// The working copy reaches the system and the running show, and nothing is written. For a window that edits the display as upstream does, live, and leaves saving
    /// to someone else: a preview's element templates, which Setup Previews' OK keeps and its Cancel reloads away. Orphaned filters stay, since no one confirmed anything.
    pub fn apply_live(&mut self, core: &AppCore) -> bool {
        if !self.is_dirty() {
            return false;
        }
        self.write_working_parts();
        core.apply_display(self.system.clone());
        true
    }

    /// F7: the working copy goes. All that needs undoing outside it is controllers started or stopped this session, and only those restart.
    pub fn discard(&mut self, core: &AppCore) {
        if !self.hardware_allowed {
            return;
        }
        for c in self.controllers.iter().filter(|c| !c.is_new && c.running != c.running_at_open) {
            core.playback.set_controller_running(&c.id, c.running_at_open);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::patching::PatchOptions;
    use super::super::test_support::*;
    use super::*;
    use std::collections::HashSet;
    use vixen_core::element::PropertyData;
    use vixen_io::load_system;

    fn temp_core(name: &str) -> AppCore {
        let root = std::env::temp_dir().join(format!("vixen-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        AppCore::open(root, true).unwrap()
    }

    fn row(s: &SetupSession, name: &str) -> String {
        s.row_path_of(s.tree.live_nodes().find(|(_, n)| n.name == name).unwrap().0)
    }

    fn open(core: &AppCore) -> SetupSession {
        SetupSession::open(core)
    }

    #[test]
    fn a_clean_session_writes_nothing() {
        let core = temp_core("clean");
        let mut s = open(&core);
        assert!(!s.commit(&core, |_, _| {}).unwrap());
        assert!(!core.paths.system_config_file().exists());
        assert_eq!(core.display.revision(), 0);
    }

    #[test]
    fn an_edit_reaches_the_disk_the_system_and_the_elements() {
        let core = temp_core("edit");
        let mut s = open(&core);
        s.add_element(None, "Arch");
        let steps = Mutex::new(Vec::new());
        assert!(s.commit(&core, |p, t| steps.lock().push((p, t.to_string()))).unwrap());
        assert_eq!(
            steps.into_inner(),
            [(0, "Preparing Display Setup Changes".to_string()), (50, "Saving System Configuration".to_string()), (100, "Saving Module Configuration".to_string())]
        );

        let loaded = load_system(&core.paths).unwrap();
        assert_eq!(loaded.channels.len(), 1);
        assert_eq!(loaded.tree.len(), 1);
        assert_eq!(loaded.tree.node(loaded.tree.roots[0]).name, "Arch");
        assert_eq!(core.system.read().channels.len(), 1);
        assert_eq!(core.display.revision(), 1);
        assert_eq!(core.playback.elements().len(), 1);
        assert_eq!(core.session_environment().tree.len(), 1);
    }

    #[test]
    fn a_live_apply_reaches_the_system_and_the_elements_but_not_the_disk() {
        let core = temp_core("live");
        let mut s = open(&core);
        assert!(!s.apply_live(&core));
        assert_eq!(core.display.revision(), 0);
        s.add_element(None, "Arch");
        assert!(s.apply_live(&core));
        assert!(!core.paths.system_config_file().exists());
        assert_eq!(core.system.read().tree.len(), 1);
        assert_eq!(core.system.read().channels.len(), 1);
        assert_eq!(core.display.revision(), 1);
        assert_eq!(core.playback.elements().len(), 1);
        // The copy stays good for the next edit.
        s.add_element(None, "Star");
        assert!(s.apply_live(&core));
        assert_eq!(core.system.read().tree.len(), 2);
        assert_eq!(core.system.read().channels.len(), 2);
        assert_eq!(core.playback.elements().len(), 2);
        // What a later save writes is what was applied.
        core.save_system_config().unwrap();
        assert_eq!(load_system(&core.paths).unwrap().tree.len(), 2);
    }

    #[test]
    fn dropping_the_session_leaves_the_system_alone() {
        let core = temp_core("cancel");
        let mut s = open(&core);
        s.add_element(None, "Arch");
        s.discard(&core);
        drop(s);
        assert_eq!(core.system.read().tree.len(), 0);
    }

    #[test]
    fn orphan_filters_go_with_their_chain_and_connected_ones_stay() {
        let core = temp_core("orphans");
        let mut s = open(&core);
        s.add_element(None, "A");
        let element = s.tree.live_nodes().next().unwrap().1.element_id.unwrap();
        let a = s.flow.find(&element).unwrap();
        // Connected, stays.
        let kept = add_breakdown(&mut s, a, 1);
        // A chain with no element above it goes.
        let orphan = add_breakdown(&mut s, a, 1);
        let child = add_breakdown(&mut s, orphan, 1);
        s.flow.reset_source(orphan);
        // And a filter added on the canvas and never connected.
        let unconnected = s.graph_add_filter(vixen_output::ids::DIMMING_CURVE, Default::default()).unwrap();
        s.dirty.filters = true;
        s.patching_changed();
        s.commit(&core, |_, _| {}).unwrap();

        let loaded = load_system(&core.paths).unwrap();
        let ids: HashSet<Uuid> = loaded.filters.iter().map(|f| f.instance_id).collect();
        assert_eq!(ids.len(), 1);
        assert!(ids.contains(&s.flow.comp(kept).id));
        assert!(!s.flow.is_live(child) && !s.flow.is_live(unconnected));
        assert!(loaded.module_store.instance(vixen_output::ids::COLOR_BREAKDOWN, s.flow.comp(kept).id).is_some());
        assert!(loaded.module_store.instance(vixen_output::ids::COLOR_BREAKDOWN, s.flow.comp(orphan).id).is_none());
    }

    #[test]
    fn deleted_property_data_leaves_the_store_and_new_data_reaches_it() {
        let core = temp_core("props");
        let mut s = open(&core);
        s.add_element(None, "A");
        let a = row(&s, "A");
        s.add_property(&[a.clone()], property_ids::ORDER);
        s.add_property(&[a.clone()], property_ids::LOCATION);
        s.commit(&core, |_, _| {}).unwrap();
        let loaded = load_system(&core.paths).unwrap();
        let order = loaded.tree.node(loaded.tree.roots[0]).properties[0].clone();
        assert_eq!(order.data, PropertyData::Order { order: 0 });
        assert!(loaded.module_store.instance(order.type_id, order.instance_id).is_some());
        assert_eq!(loaded.module_store.instance(order.type_id, order.instance_id).unwrap().data_model_type(), Some("VixenModules.Property.Order.OrderData, Order"));

        let mut s = open(&core);
        let a = s.row_path_of(s.tree.roots[0]);
        s.remove_properties(&[a.clone()], &[property_ids::ORDER]);
        s.commit(&core, |_, _| {}).unwrap();
        let loaded = load_system(&core.paths).unwrap();
        assert!(loaded.module_store.instance(order.type_id, order.instance_id).is_none());
        assert_eq!(loaded.tree.node(loaded.tree.roots[0]).properties.len(), 1);
    }

    #[test]
    fn orphaned_colour_data_goes_but_only_on_a_real_save() {
        let core = temp_core("orphan-data");
        let mut s = open(&core);
        s.add_element(None, "A");
        let a = row(&s, "A");
        s.add_property(&[a.clone()], property_ids::COLOR);
        s.commit(&core, |_, _| {}).unwrap();
        let instance = core.system.read().tree.node(core.system.read().tree.roots[0]).properties[0].instance_id;

        // Node loses the property on disk by being a group: a group's Color data isn't on any leaf.
        let mut s = open(&core);
        let a = s.row_path_of(s.tree.roots[0]);
        s.remove_properties(&[a], &[property_ids::COLOR]);
        s.commit(&core, |_, _| {}).unwrap();
        assert!(core.system.read().module_store.instance(property_ids::COLOR, instance).is_none());
    }

    #[test]
    fn patches_controllers_and_run_state_round_trip() {
        let core = temp_core("patches");
        let mut s = open(&core);
        s.add_element(None, "A");
        s.add_element(None, "B");
        let slot = s.add_controller(vixen_output::ids::DUMMY_LIGHTING, "Dummy", 3).unwrap();
        let other = s.add_controller(vixen_output::ids::DEBUG_CONTROLLER, "Debug", 1).unwrap();
        let rows = [row(&s, "A"), row(&s, "B")];
        s.patch_elements(&rows, &s.all_outputs_selection(), &PatchOptions::default(), false);
        s.stop_controllers(None, &[slot]);
        s.move_controllers(&[other], true);
        s.commit(&core, |_, _| {}).unwrap();

        let loaded = load_system(&core.paths).unwrap();
        let names: Vec<&str> = loaded.controllers.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Debug", "Dummy"]);
        assert_eq!(loaded.patches.len(), 2);
        assert_eq!(loaded.disabled_devices, [s.controllers[slot as usize].id]);
        assert!(loaded.module_store.instance(vixen_output::ids::DUMMY_LIGHTING, s.controllers[slot as usize].instance_id).is_some());
        // The debug controller has no data to store.
        assert!(loaded.module_store.instance(vixen_output::ids::DEBUG_CONTROLLER, s.controllers[other as usize].instance_id).is_none());

        // Reopened, it's all there, and with nothing changed, nothing is written.
        let mut s = open(&core);
        assert_eq!(s.controller_rows().len(), 2);
        assert_eq!(s.flow.to_patches().len(), 2);
        assert!(!s.commit(&core, |_, _| {}).unwrap());

        // Deleting a disabled controller takes it off the list.
        let mut s = open(&core);
        let dummy = s.controllers.iter().position(|c| c.name == "Dummy").unwrap() as u32;
        s.delete_controllers(None, &[dummy]);
        s.commit(&core, |_, _| {}).unwrap();
        let loaded = load_system(&core.paths).unwrap();
        assert!(loaded.disabled_devices.is_empty());
        assert_eq!(loaded.patches.len(), 0);
    }

    #[test]
    fn every_element_is_saved_under_its_nodes_name() {
        let core = temp_core("names");
        let mut s = open(&core);
        s.add_element(None, "A");
        s.add_element(None, "B");
        s.commit(&core, |_, _| {}).unwrap();
        // A channel name the file holds that differs from its node's: loading it upstream renames the element after the node.
        core.system.write().channels[1].name = "Different".to_string();

        let mut s = open(&core);
        let a = s.row_path_of(s.tree.roots[0]);
        s.rename_rows(&[a], &["A2".to_string()]);
        s.commit(&core, |_, _| {}).unwrap();
        let names: Vec<String> = load_system(&core.paths).unwrap().channels.into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["A2", "B"]);

        // And when the elements themselves weren't touched.
        core.system.write().channels[1].name = "Different".to_string();
        let mut s = open(&core);
        s.add_controller(vixen_output::ids::DUMMY_LIGHTING, "C", 2).unwrap();
        s.commit(&core, |_, _| {}).unwrap();
        let names: Vec<String> = load_system(&core.paths).unwrap().channels.into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["A2", "B"]);
    }

    #[test]
    fn built_in_tags_reach_the_file_with_the_first_real_save() {
        let core = temp_core("tags");
        let mut s = open(&core);
        s.add_element(None, "A");
        s.commit(&core, |_, _| {}).unwrap();
        assert_eq!(load_system(&core.paths).unwrap().tags.len(), 3);
    }

    #[test]
    fn a_node_at_the_top_and_in_a_group_stays_in_both_after_a_reload() {
        let core = temp_core("links");
        let mut s = open(&core);
        s.add_element(None, "A");
        let a = row(&s, "A");
        s.create_group(&[a], "G");
        s.commit(&core, |_, _| {}).unwrap();

        let back = load_system(&core.paths).unwrap();
        let names: Vec<&str> = back.tree.roots.iter().map(|r| back.tree.node(*r).name.as_str()).collect();
        assert_eq!(names, ["A", "G"]);
        let group = back.tree.roots[1];
        assert_eq!(back.tree.node(group).children.len(), 1);

        // And a later edit doesn't lose the link.
        let mut s = open(&core);
        s.add_element(None, "B");
        s.commit(&core, |_, _| {}).unwrap();
        let back = load_system(&core.paths).unwrap();
        let names: Vec<&str> = back.tree.roots.iter().map(|r| back.tree.node(*r).name.as_str()).collect();
        assert_eq!(names, ["A", "G", "B"]);
    }

    #[test]
    fn an_element_no_node_refers_to_stays() {
        let core = temp_core("nodeless");
        let mut s = open(&core);
        s.add_element(None, "A");
        s.commit(&core, |_, _| {}).unwrap();
        // What upstream's F44 bug leaves: an element in the list with no node.
        let ghost = Uuid::new_v4();
        core.system.write().channels.push(Channel { id: ghost, name: "Ghost".to_string() });

        let mut s = open(&core);
        let a = row(&s, "A");
        s.delete_rows(&[a]);
        s.add_element(None, "B");
        s.commit(&core, |_, _| {}).unwrap();
        let ids: Vec<Uuid> = load_system(&core.paths).unwrap().channels.iter().map(|c| c.id).collect();
        assert!(ids.contains(&ghost));
        assert_eq!(ids.len(), 2, "A's element went with A");
    }

    #[test]
    fn deleting_a_group_with_its_child_selected_leaves_nothing_behind() {
        let core = temp_core("group-child");
        let mut s = open(&core);
        s.add_element(None, "L");
        s.add_element(None, "M");
        let root_row = |s: &SetupSession, name: &str| s.element_rows(None).into_iter().find(|r| r.name == name).unwrap().path;
        let (l, m) = (root_row(&s, "L"), root_row(&s, "M"));
        s.create_group(&[l.clone(), m.clone()], "G");
        // Now G alone holds L and M.
        s.delete_rows(&[l, m]);
        let g = root_row(&s, "G");
        let child = s.element_rows(Some(&g))[0].path.clone();
        s.delete_rows(&[g, child]);
        assert_eq!(s.tree.live_nodes().count(), 0);
        s.commit(&core, |_, _| {}).unwrap();
        let back = load_system(&core.paths).unwrap();
        assert!(back.channels.is_empty(), "{:?}", back.channels);
        assert_eq!(back.tree.len(), 0);

        // A child's row alone, in a group with others: that link goes and nothing else.
        let mut s = open(&core);
        s.add_element(None, "A");
        s.add_element(None, "B");
        let (a, b) = (root_row(&s, "A"), root_row(&s, "B"));
        s.create_group(&[a.clone(), b.clone()], "H");
        let h = root_row(&s, "H");
        let under = s.element_rows(Some(&h))[0].path.clone();
        s.delete_rows(&[under]);
        let names: Vec<String> = s.element_rows(Some(&h)).into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["B"]);
        assert_eq!(s.tree.live_nodes().count(), 3);
    }

    #[test]
    fn color_data_on_a_group_goes_at_ok_and_comes_back_as_the_default() {
        let core = temp_core("group-color");
        let mut s = open(&core);
        s.add_element(None, "L");
        let l = row(&s, "L");
        s.create_group(&[l.clone()], "G");
        s.delete_rows(&[l]);
        let g = row(&s, "G");
        s.add_property(&[g.clone()], property_ids::LOCATION);
        let node = s.tree.live_nodes().find(|(_, n)| n.name == "G").unwrap().0;
        assert!(s.set_property_data(node, &property_ids::LOCATION, PropertyData::Location { x: 5, y: 6, z: 7 }));
        s.commit(&core, |_, _| {}).unwrap();

        let back = load_system(&core.paths).unwrap();
        let group = back.tree.roots.iter().map(|r| back.tree.node(*r)).find(|n| n.name == "G").unwrap();
        let p = group.properties.iter().find(|p| p.type_id == property_ids::LOCATION).expect("the group keeps the property");
        assert_eq!(p.data, vixen_io::system::decode_property(&property_ids::LOCATION, None));
        assert!(back.module_store.instance(p.type_id, p.instance_id).is_none());
    }

    #[test]
    fn renaming_two_rows_in_turn_counts_the_later_ones_old_name() {
        let core = temp_core("rename-order");
        let mut s = open(&core);
        s.add_element(None, "A");
        s.add_element(None, "B");
        let rows = vec![row(&s, "A"), row(&s, "B")];
        s.rename_rows(&rows, &["B".to_string(), "C".to_string()]);
        let names: Vec<String> = s.element_rows(None).into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["B - 2", "C"]);
    }

    #[test]
    fn a_copied_row_deleted_before_the_paste_pastes_nothing() {
        let core = temp_core("copy-delete");
        let mut s = open(&core);
        s.add_element(None, "A");
        s.add_element(None, "G");
        let a = row(&s, "A");
        s.copy_rows(&[a.clone()]);
        s.delete_rows(&[a]);
        let g = row(&s, "G");
        let result = s.paste_links(Some(&g));
        assert_eq!(result, super::super::elements::EditResult::default());
        let result = s.paste_as_new(Some(&g));
        assert_eq!(result, super::super::elements::EditResult::default());
        assert!(s.element_rows(Some(&g)).is_empty());
    }

    /// Writes a display of 50,000 elements for measuring the window: `VIXEN_SYNTHETIC_OUT=<folder> cargo test -p vixen-session --lib make_synthetic_display -- --ignored`.
    /// A wide group of 20,000 leaves patched straight to a controller, and 30 strips of 1,000 leaves each with a three-colour breakdown.
    #[test]
    #[ignore]
    fn make_synthetic_display() {
        let Some(out) = std::env::var_os("VIXEN_SYNTHETIC_OUT") else { return };
        let root = std::path::PathBuf::from(out);
        let _ = std::fs::remove_dir_all(&root);
        let core = AppCore::open(root.clone(), true).unwrap();
        let mut s = open(&core);
        let started = std::time::Instant::now();

        s.add_element(None, "Wide");
        let wide = row(&s, "Wide");
        let names: Vec<String> = (1..=20_000).map(|i| format!("W{i}")).collect();
        s.add_elements(Some(&wide), &names);
        let mut strips = Vec::new();
        for n in 1..=30 {
            let name = format!("Strip {n}");
            s.add_element(None, &name);
            let strip = row(&s, &name);
            let names: Vec<String> = (1..=1_000).map(|i| format!("S{n}-{i}")).collect();
            s.add_elements(Some(&strip), &names);
            strips.push(strip);
        }
        eprintln!("tree {:?}", started.elapsed());

        // A breakdown on every strip leaf.
        let leaves: Vec<super::super::flow::Cid> = s
            .tree
            .live_nodes()
            .filter(|(_, n)| n.name.starts_with('S') && n.children.is_empty())
            .filter_map(|(_, n)| n.element_id.and_then(|e| s.flow.find(&e)))
            .collect();
        for c in leaves {
            add_breakdown(&mut s, c, 3);
        }
        s.dirty.filters = true;
        s.patching_changed();
        eprintln!("filters {:?}", started.elapsed());

        let ddp = vixen_output::ids::DDP;
        let a = s.add_controller(ddp, "Wide Controller", 20_000).unwrap();
        let b = s.add_controller(ddp, "Strip Controller", 90_000).unwrap();
        let options = PatchOptions::default();
        let only = |slot: u32, count: u32| super::super::controllers::OutputSelection { items: vec![(slot, vec![0..count])] };
        assert_eq!(s.patch_elements(&[wide], &only(a, 20_000), &options, false), 20_000);
        assert_eq!(s.patch_elements(&strips, &only(b, 90_000), &options, false), 90_000);
        eprintln!("patched {:?}", started.elapsed());
        assert!(s.commit(&core, |_, _| {}).unwrap());
        eprintln!("saved {:?} to {}", started.elapsed(), root.display());

        let back = load_system(&core.paths).unwrap();
        assert_eq!(back.channels.len(), 50_000);
        // The wide group's outputs, the breakdowns' sources, and the strips' outputs.
        assert_eq!(back.patches.len(), 20_000 + 30_000 + 90_000);
    }

    #[test]
    fn an_open_sequence_follows_the_display() {
        use crate::sequence::SequenceSession;
        const SET_LEVEL: Uuid = uuid::uuid!("32cff8e0-5b10-4466-a093-0d232c55aac0");
        let core = temp_core("sequence");
        let mut s = open(&core);
        s.add_element(None, "A");
        s.add_element(None, "Gone");
        s.commit(&core, |_, _| {}).unwrap();

        let env = core.session_environment();
        let mut sequence = SequenceSession::new_untitled(&env);
        let (keep, gone) = {
            let system = core.system.read();
            (system.tree.node(system.tree.roots[0]).id, system.tree.node(system.tree.roots[1]).id)
        };
        let effect = sequence.add_new_effect(SET_LEVEL, keep, "A", vixen_core::TimeSpan::ZERO, vixen_core::TimeSpan::from_ms_i(100)).unwrap();
        let orphan = sequence.add_new_effect(SET_LEVEL, gone, "Gone", vixen_core::TimeSpan::ZERO, vixen_core::TimeSpan::from_ms_i(100)).unwrap();
        sequence.wait_for_renders();
        let shared = Arc::new(Mutex::new(sequence));
        core.register_sequence_session(&shared);

        // A new element goes in front of what the effect targets, by deleting one and adding another.
        let mut s = open(&core);
        s.delete_rows(&[row(&s, "Gone")]);
        s.add_element(None, "New");
        s.commit(&core, |_, _| {}).unwrap();

        let mut sequence = shared.lock();
        assert_eq!(sequence.tree().len(), 2);
        sequence.wait_for_renders();
        assert_eq!(sequence.effect(effect).unwrap().targets.len(), 1);
        assert_eq!(sequence.tree().node(sequence.effect(effect).unwrap().targets[0]).id, keep);
        assert!(sequence.effect(orphan).unwrap().targets.is_empty());
        assert_eq!(sequence.elements().len(), 2);
        assert!(sequence.effect(effect).unwrap().is_rendered());
    }
}
