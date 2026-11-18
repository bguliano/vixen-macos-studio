//! What the Display Setup window does, without the window: the port of `VixenApplication`'s Setup forms and the system managers
//! they edit (`NodeManager`, `DataFlowManager`, `FilterManager`, `OutputControllerManager`, `ElementTagService`).
//!
//! Upstream edits the live system and reloads it all on Cancel. Here a [`SetupSession`] is a working copy, so Cancel is dropping
//! it, and OK is [`SetupSession::commit`]. All calls are safe off the main thread: the FFI wraps a session in a lock and returns
//! plain records.

pub mod commit;
pub mod controllers;
pub mod elements;
pub mod export;
pub mod fixture;
pub mod fixture_editor;
pub mod flow;
pub mod graph;
pub mod naming;
pub mod patching;
pub mod properties;
pub mod selection;
pub mod tags;
pub mod templates;
pub mod tree;
pub mod wire_diagram;

use crate::app::AppCore;
use controllers::ControllerEntry;
use flow::{Cid, CompKind, DataFlow, FlowWarning};
use parking_lot::Mutex;
use patching::LeafTable;
use std::collections::{HashMap, HashSet};
use tree::{Nid, SetupTree};
use uuid::Uuid;
use vixen_io::{SystemData, TagDefinition};
use vixen_output::FilterConfig;

/// An output filter instance in the working copy.
#[derive(Clone, Debug)]
pub struct FilterEntry {
    pub cid: Cid,
    pub type_id: Uuid,
    pub instance_id: Uuid,
    /// `None` for a module type this port doesn't have. Its data stays in the store untouched.
    pub config: Option<FilterConfig>,
    /// New or edited, so its module data has to be written at OK.
    pub changed: bool,
    pub alive: bool,
}

impl FilterEntry {
    /// `IDataFlowComponent.Name`.
    pub fn name(&self) -> String {
        match &self.config {
            Some(c) => c.display_name(),
            None => "Unknown filter".to_string(),
        }
    }
}

/// Which parts of the system have changed since the window opened. A clean session writes nothing at OK, so an OK with no edits
/// leaves both files byte-identical and makes no backups.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Dirty {
    pub studio_module: bool, // STUDIO: module-only show/schedule transactions without rebuilding unrelated setup data
    pub tree: bool,
    pub flow: bool,
    pub filters: bool,
    pub controllers: bool,
    pub tags: bool,
    /// Controllers started or stopped.
    pub run_state: bool,
}

impl Dirty {
    pub fn any(&self) -> bool {
        self.studio_module || self.tree || self.flow || self.filters || self.controllers || self.tags || self.run_state // STUDIO: count module-only commands as edits
    }
}

/// A working copy of the display.
pub struct SetupSession {
    pub tree: SetupTree,
    pub flow: DataFlow,
    /// The rest of the system. Its `tree`, `channels`, `patches`, `filters`, `controllers`, `tags` and `disabled_devices` go stale during the session
    /// and are rebuilt from the working parts at OK, only for the parts that changed.
    pub system: SystemData,
    pub filters: Vec<FilterEntry>,
    pub filter_lookup: HashMap<Cid, usize>,
    /// In slots that never move, so a row id stays good.
    pub controllers: Vec<ControllerEntry>,
    /// Slots in pane order.
    pub controller_order: Vec<u32>,
    pub tags: Vec<TagDefinition>,
    pub dirty: Dirty,
    tree_revision: u64,
    patch_revision: u64,
    /// Property data to drop from the module store at OK.
    pub(crate) removed_properties: Vec<(Uuid, Uuid)>,
    pub(crate) removed_filter_data: Vec<(Uuid, Uuid)>,
    pub(crate) removed_controller_data: Vec<(Uuid, Uuid)>,
    pub(crate) clipboard: Option<Vec<Nid>>,
    pub(crate) property_clipboard: Option<elements::PropertyClipboard>,
    pub(crate) leaf_table: Mutex<LeafTable>,
    pub(crate) tag_table: tags::TagTable,
    pub graph: graph::GraphState,
    /// Patches the file had that couldn't be used.
    pub warnings: Vec<FlowWarning>,
    /// Whether controllers can really start: false in tests, and whenever the app is muted.
    pub(crate) hardware_allowed: bool,
    /// Elements the file lists that no node refers to, which upstream's F44 bug leaves behind. They stay, so their ids do.
    pub(crate) nodeless_channels: HashSet<Uuid>,
    /// Color and Location data on groups that OK drops from the store, so nothing puts it back.
    pub(crate) group_properties_dropped: HashSet<Uuid>,
}

impl SetupSession {
    /// Opens a working copy of the system as it stands.
    pub fn open(core: &AppCore) -> SetupSession {
        let system = core.system.read().clone();
        let running = controllers::running_controllers(core, &system);
        let session = SetupSession::from_system(system, &running, core.playback.hardware_allowed());
        for warning in &session.warnings {
            crate::logging::warn("Vixen.Setup", &format!("Patch not used: {warning:?}"));
        }
        session
    }

    /// `running` says which controllers are running now. Tests pass whatever suits.
    pub fn from_system(system: SystemData, running: &HashSet<Uuid>, hardware_allowed: bool) -> SetupSession {
        let tree = SetupTree::from_element_tree(&system.tree);
        let mut flow = DataFlow::new();

        // `_componentLookup` in the order `VixenSystem.LoadSystemConfig` fills it, which is the order patches are written in.
        for channel in &system.channels {
            flow.add_component(channel.id, CompKind::Element, 1);
        }
        // A leaf can name a channel the file doesn't have. It stays a working element here, after the listed ones.
        for (_, n) in tree.live_nodes() {
            if let Some(e) = n.element_id {
                flow.add_component(e, CompKind::Element, 1);
            }
        }
        let mut controllers = Vec::with_capacity(system.controllers.len());
        for (slot, c) in system.controllers.iter().enumerate() {
            controllers.push(ControllerEntry::from_system(c, slot as u32, running.contains(&c.id), &system, &mut flow));
        }
        let controller_order: Vec<u32> = (0..controllers.len() as u32).collect();

        let mut filters = Vec::with_capacity(system.filters.len());
        let mut filter_lookup = HashMap::with_capacity(system.filters.len());
        // A filter of a type this port doesn't have keeps its patches: its output count comes from the highest one in use.
        let mut highest_output: HashMap<Uuid, i32> = HashMap::new();
        for p in &system.patches {
            if let Some(source) = p.source_id {
                let e = highest_output.entry(source).or_insert(-1);
                *e = (*e).max(p.output);
            }
        }
        for f in &system.filters {
            let config = match system.module_store.instance(f.type_id, f.instance_id).and_then(|e| e.data()) {
                Some(data) => FilterConfig::from_xml(&f.type_id, data),
                None => FilterConfig::new_default(&f.type_id, f.instance_id),
            };
            let count = match &config {
                Some(c) => c.output_names().len() as u32,
                None => (highest_output.get(&f.instance_id).copied().unwrap_or(-1) + 1) as u32,
            };
            let cid = flow.add_component(f.instance_id, CompKind::Filter, count);
            filter_lookup.insert(cid, filters.len());
            filters.push(FilterEntry { cid, type_id: f.type_id, instance_id: f.instance_id, config, changed: false, alive: true });
        }
        let warnings = flow.apply_patches(&system.patches);
        let tags = tags::seeded_tags(&system.tags);
        let tag_table = tags::TagTable::new(&tags);
        let nodeless_channels = system.channels.iter().filter(|c| tree.find_by_element(&c.id).is_none()).map(|c| c.id).collect();

        SetupSession {
            tree,
            flow,
            system,
            filters,
            filter_lookup,
            controllers,
            controller_order,
            tags,
            dirty: Dirty::default(),
            tree_revision: 1,
            patch_revision: 1,
            removed_properties: Vec::new(),
            removed_filter_data: Vec::new(),
            removed_controller_data: Vec::new(),
            clipboard: None,
            property_clipboard: None,
            leaf_table: Mutex::new(LeafTable::default()),
            tag_table,
            graph: graph::GraphState::default(),
            warnings,
            hardware_allowed,
            nodeless_channels,
            group_properties_dropped: HashSet::new(),
        }
    }

    /// Whether Cancel or the close box would lose anything.
    pub fn is_dirty(&self) -> bool {
        self.dirty.any()
    }

    pub fn tree_revision(&self) -> u64 {
        self.tree_revision
    }

    pub fn patch_revision(&self) -> u64 {
        self.patch_revision
    }

    pub(crate) fn tree_changed(&mut self) {
        self.tree_revision += 1;
        self.dirty.tree = true;
    }

    pub(crate) fn patching_changed(&mut self) {
        self.patch_revision += 1;
        self.dirty.flow = true;
    }

    /// Applies what a tree edit means for data flow and the module store.
    pub(crate) fn apply_effects(&mut self, effects: tree::TreeEffects) {
        let mut flow_changed = false;
        for element in &effects.removed_elements {
            if let Some(c) = self.flow.find(element) {
                flow_changed |= self.flow.comp(c).source.is_some() || !self.flow.destinations(c).is_empty();
                self.flow.remove_component(c);
            }
        }
        for (element, _) in &effects.added_elements {
            self.flow.add_component(*element, CompKind::Element, 1);
        }
        self.removed_properties.extend(effects.removed_properties);
        if flow_changed || !effects.removed_elements.is_empty() || !effects.added_elements.is_empty() {
            self.patching_changed();
        }
    }

    /// Takes in what a template generated: its elements join data flow, and the tree counts as edited.
    pub fn apply_template_result(&mut self, result: templates::TemplateResult) {
        self.apply_effects(result.effects);
        self.tree_changed();
        self.bump_graph();
    }

    pub fn filter_for(&self, cid: Cid) -> Option<&FilterEntry> {
        self.filter_lookup.get(&cid).map(|i| &self.filters[*i])
    }
}

/// The two trees' ball colours, for rows and the graphical view's status. Rust names them, and the window maps them to pictures.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum RowIcon {
    Group,
    Patched,
    PatchedMasked,
    Unpatched,
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use vixen_core::element::ElementTree;
    use vixen_io::{Channel, Controller, ControllerOutput};

    /// A small system: leaves `count` per name, one Dummy Lighting controller with `outputs` outputs, nothing patched.
    pub fn system(leaves: &[&str], outputs: usize) -> SystemData {
        let mut data = SystemData::default();
        let mut tree = ElementTree::new();
        for name in leaves {
            let n = tree.add_element(name);
            let element = tree.node(n).element_id.unwrap();
            data.channels.push(Channel { id: element, name: (*name).to_string() });
        }
        data.tree = tree;
        if outputs > 0 {
            data.controllers.push(controller("Main", outputs));
        }
        data
    }

    pub fn controller(name: &str, outputs: usize) -> Controller {
        Controller {
            name: name.to_string(),
            hardware_type_id: vixen_output::ids::DUMMY_LIGHTING,
            hardware_instance_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            outputs: (0..outputs).map(|i| ControllerOutput { name: format!("Output {}", i + 1), id: Uuid::new_v4() }).collect(),
        }
    }

    pub fn session(leaves: &[&str], outputs: usize) -> SetupSession {
        SetupSession::from_system(system(leaves, outputs), &HashSet::new(), false)
    }

    /// Adds a colour breakdown with `outputs` outputs under `element`, returning its component.
    pub fn add_breakdown(s: &mut SetupSession, source: Cid, outputs: usize) -> Cid {
        let instance = Uuid::new_v4();
        let mut config = vixen_output::filters::ColorBreakdownConfig {
            ids: vixen_output::ModuleIds::new(vixen_output::ids::COLOR_BREAKDOWN, instance),
            ..Default::default()
        };
        for i in 0..outputs {
            config.breakdown_items.push(vixen_output::filters::ColorBreakdownItem {
                color: vixen_output::DotNetColor::from_color(vixen_core::Color::RED),
                name: format!("C{i}"),
            });
        }
        let cid = s.flow.add_component(instance, CompKind::Filter, outputs as u32);
        s.filter_lookup.insert(cid, s.filters.len());
        s.filters.push(FilterEntry {
            cid,
            type_id: vixen_output::ids::COLOR_BREAKDOWN,
            instance_id: instance,
            config: Some(FilterConfig::ColorBreakdown(config)),
            changed: true,
            alive: true,
        });
        assert!(s.flow.set_source(cid, flow::SourceRef { component: source, output: 0 }));
        cid
    }

}
