//! The controllers panel: `SetupControllersSimple`, `Common.Controls.ControllerTree` and `OutputController`'s output list rules.

use super::flow::{Cid, CompKind, DataFlow};
use super::SetupSession;
use crate::app::AppCore;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use uuid::Uuid;
use vixen_io::{Controller, ControllerOutput, ModuleEntry, SystemData};
use vixen_output::ids::{controller_descriptor, CONTROLLER_DESCRIPTORS, DEBUG_CONTROLLER};
use vixen_output::modules::default_output_name;
use vixen_output::{create_module, ControllerConfig};

/// `OutputPageSize`: above this many outputs a controller's outputs sit under page rows.
pub const OUTPUT_PAGE_SIZE: u32 = 5000;

#[derive(Clone, Debug)]
pub struct OutputEntry {
    pub name: String,
    pub id: Uuid,
    pub cid: Cid,
}

/// A controller in the working copy.
#[derive(Clone, Debug)]
pub struct ControllerEntry {
    pub id: Uuid,
    pub name: String,
    pub type_id: Uuid,
    pub instance_id: Uuid,
    pub outputs: Vec<OutputEntry>,
    pub alive: bool,
    pub running: bool,
    pub running_at_open: bool,
    /// Added this session, so its module data has to be written at OK.
    pub is_new: bool,
    /// The module's data, for the output names E1.31 gives itself and for the controller dialogs.
    pub config: Option<ControllerConfig>,
    /// Edited by its dialog this session.
    pub config_changed: bool,
}

impl ControllerEntry {
    pub(crate) fn from_system(c: &Controller, _slot: u32, running: bool, system: &SystemData, flow: &mut DataFlow) -> ControllerEntry {
        let outputs = c
            .outputs
            .iter()
            .map(|o| OutputEntry { name: o.name.clone(), id: o.id, cid: flow.add_component(o.id, CompKind::Output, 0) })
            .collect();
        let config = match system.module_store.instance(c.hardware_type_id, c.hardware_instance_id).and_then(|e| e.data()) {
            Some(data) => ControllerConfig::from_xml(&c.hardware_type_id, data),
            None => ControllerConfig::new_default(&c.hardware_type_id, c.hardware_instance_id),
        };
        ControllerEntry {
            id: c.id,
            name: c.name.clone(),
            type_id: c.hardware_type_id,
            instance_id: c.hardware_instance_id,
            outputs,
            alive: true,
            running,
            running_at_open: running,
            is_new: false,
            config,
            config_changed: false,
        }
    }

    pub fn type_name(&self) -> &'static str {
        controller_descriptor(&self.type_id).map_or("Unknown Controller", |d| d.type_name)
    }

    /// `HasSetup`. Only the debug controller has none.
    pub fn has_setup(&self) -> bool {
        controller_descriptor(&self.type_id).is_some() && self.type_id != DEBUG_CONTROLLER
    }
}

/// Which controllers count as running when the window opens: those actually running, when execution is open. Otherwise, as on Windows where the system starts
/// with the app, every controller not on the disabled list. Nothing here starts anything.
pub(crate) fn running_controllers(core: &AppCore, system: &SystemData) -> HashSet<Uuid> {
    match core.playback.running_controllers() {
        Some(running) => running,
        None => system.controllers.iter().map(|c| c.id).filter(|id| !system.disabled_devices.contains(id)).collect(),
    }
}

/// Selected outputs per controller, in pane order, as ranges: a whole 170,000-output controller is one entry, where upstream builds a `HashSet` of indexes several
/// times per click. Index ranges in a controller's entry are ascending and don't overlap.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputSelection {
    pub items: Vec<(u32, Vec<Range<u32>>)>,
}

impl OutputSelection {
    /// One range of one controller.
    pub fn single(slot: u32, range: Range<u32>) -> OutputSelection {
        OutputSelection { items: vec![(slot, vec![range])] }
    }

    /// `found[slot]` holds that controller's indexes in any order, with repeats allowed. `order` is pane order.
    pub fn from_indexes(order: &[u32], mut found: Vec<Vec<u32>>) -> OutputSelection {
        let mut items = Vec::new();
        for &slot in order {
            let Some(list) = found.get_mut(slot as usize) else { continue };
            if list.is_empty() {
                continue;
            }
            list.sort_unstable();
            list.dedup();
            let mut ranges: Vec<Range<u32>> = Vec::new();
            for &i in list.iter() {
                match ranges.last_mut() {
                    Some(r) if r.end == i => r.end = i + 1,
                    _ => ranges.push(i..i + 1),
                }
            }
            items.push((slot, ranges));
        }
        OutputSelection { items }
    }

    pub fn is_empty(&self) -> bool {
        self.items.iter().all(|(_, r)| r.iter().all(|x| x.is_empty()))
    }

    pub fn output_count(&self) -> u32 {
        self.items.iter().flat_map(|(_, r)| r.iter()).map(|r| r.end - r.start).sum()
    }
}

/// F80's row icons.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ControllerIcon {
    /// The "Group" icon, on a running controller and on a page row.
    Group,
    /// Red ball: a controller that isn't running.
    Stopped,
    /// White ball.
    OutputUnpatched,
    /// Grey ball. A source that is there but has no component, which the working graph can't hold after load, kept for its picture.
    OutputBroken,
    /// Green ball.
    OutputPatched,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ControllerRowKind {
    Controller,
    Page,
    Output,
}

/// One row of the controller tree. Ids are `c{slot}`, `p{slot}:{start}` and `o{slot}:{index}`: identity by controller, page start and output index, as upstream keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerRow {
    pub id: String,
    pub title: String,
    pub kind: ControllerRowKind,
    pub icon: ControllerIcon,
    pub has_children: bool,
    pub child_count: u32,
}

/// The module-level facts about a selected row set: F81.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeSelection {
    /// Selected controllers, in pane order.
    pub controllers: Vec<u32>,
    /// Outputs selected individually, per slot.
    outputs: Vec<(u32, Vec<u32>)>,
    /// Selected rows that aren't outputs and aren't controllers.
    pub pages: u32,
    /// All rows, of any kind.
    pub rows: u32,
}

/// F85, F86, for the panel's labels and buttons.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControllerPanelState {
    pub type_label: String,
    pub outputs_label: String,
    pub one_controller: bool,
    pub any_controller: bool,
    pub can_start: bool,
    pub can_stop: bool,
    pub can_find_elements: bool,
    pub has_setup: bool,
}

/// F92, F93. `show_*` says which items appear at all, with their wording.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControllerMenuState {
    /// False means no menu appears.
    pub show: bool,
    pub controller_items: bool,
    pub configure: bool,
    pub channel_count: bool,
    pub rename: bool,
    pub delete: bool,
    pub start: bool,
    pub stop: bool,
    pub unpatch_controllers: bool,
    pub unpatch_controllers_title: String,
    pub insert_channels: bool,
    pub unpatch_channels: bool,
    pub unpatch_channels_title: String,
    pub find_elements: bool,
    pub find_elements_title: String,
}

/// The rows to expand and select for a found set of outputs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControllerReveal {
    pub expand: Vec<String>,
    pub select: Vec<String>,
}

fn controller_row_id(slot: u32) -> String {
    format!("c{slot}")
}

fn page_row_id(slot: u32, start: u32) -> String {
    format!("p{slot}:{start}")
}

fn output_row_id(slot: u32, index: u32) -> String {
    format!("o{slot}:{index}")
}

/// `(kind, slot, value)` for a row id.
fn parse_row(id: &str) -> Option<(ControllerRowKind, u32, u32)> {
    let (kind, rest) = id.split_at_checked(1)?;
    match kind {
        "c" => Some((ControllerRowKind::Controller, rest.parse().ok()?, 0)),
        "p" | "o" => {
            let (slot, value) = rest.split_once(':')?;
            let kind = if kind == "p" { ControllerRowKind::Page } else { ControllerRowKind::Output };
            Some((kind, slot.parse().ok()?, value.parse().ok()?))
        }
        _ => None,
    }
}

/// The controller types for the "Add:" list, sorted by name (F76).
pub fn controller_types() -> Vec<(Uuid, &'static str)> {
    let mut list: Vec<(Uuid, &'static str)> = CONTROLLER_DESCRIPTORS.iter().map(|d| (d.type_id, d.type_name)).collect();
    // A sorted `ComboBox` compares by culture, which puts "Debugging Values" before "DMX Open".
    list.sort_by_cached_key(|t| t.1.to_lowercase());
    list
}

/// `OutputLimit` for a new module of this type.
pub fn output_limit(type_id: &Uuid) -> u32 {
    let Some(config) = ControllerConfig::new_default(type_id, Uuid::nil()) else { return i32::MAX as u32 };
    create_module(config).output_limit().min(i32::MAX as usize) as u32
}

/// `ReorderSelectedNodes` (F78) over a list of slots. `selected` needn't be in order.
pub fn move_in_order(order: &[u32], selected: &HashSet<u32>, up: bool) -> Vec<u32> {
    let delta: i64 = if up { -1 } else { 1 };
    // Selected roots leave bottom to top, each remembering where it was, then go back top to bottom.
    let held: Vec<(u32, i64)> = order
        .iter()
        .enumerate()
        .filter(|(_, s)| selected.contains(s))
        .map(|(i, s)| (*s, i as i64))
        .collect();
    let mut list: Vec<u32> = order.iter().copied().filter(|s| !selected.contains(s)).collect();
    let mut insertion: i64 = -1;
    for (slot, original) in held {
        let target = original + delta;
        // At or above the previous insertion point it goes to its own place, which keeps a block at the top from reordering.
        insertion = if target <= insertion { original } else { target };
        let at = (insertion.max(0) as usize).min(list.len());
        list.insert(at, slot);
    }
    list
}

impl SetupSession {
    fn live_controller(&self, slot: u32) -> Option<&ControllerEntry> {
        self.controllers.get(slot as usize).filter(|c| c.alive)
    }

    /// Output component to `(slot, index)`, the port of `getOutputDetailsForDataFlowComponent`. Built where it's needed, so an output edit has nothing to keep in sync.
    pub(crate) fn output_lookup(&self) -> HashMap<Cid, (u32, u32)> {
        let total: usize = self.controllers.iter().map(|c| c.outputs.len()).sum();
        let mut map = HashMap::with_capacity(total);
        for &slot in &self.controller_order {
            let Some(c) = self.live_controller(slot) else { continue };
            for (i, o) in c.outputs.iter().enumerate() {
                map.insert(o.cid, (slot, i as u32));
            }
        }
        map
    }

    // ---- Rows ---------------------------------------------------------------------------------------------------------

    /// The root rows.
    pub fn controller_rows(&self) -> Vec<ControllerRow> {
        self.controller_order
            .iter()
            .filter_map(|s| self.live_controller(*s).map(|c| (*s, c)))
            .map(|(slot, c)| ControllerRow {
                id: controller_row_id(slot),
                title: c.name.clone(),
                kind: ControllerRowKind::Controller,
                icon: if c.running { ControllerIcon::Group } else { ControllerIcon::Stopped },
                has_children: !c.outputs.is_empty(),
                child_count: Self::top_child_count(c.outputs.len() as u32),
            })
            .collect()
    }

    fn top_child_count(outputs: u32) -> u32 {
        if outputs <= OUTPUT_PAGE_SIZE {
            outputs
        } else {
            outputs.div_ceil(OUTPUT_PAGE_SIZE)
        }
    }

    /// The rows under a controller or page row. F80: one row per output up to 5000, page rows above that.
    pub fn controller_children(&self, parent: &str) -> Vec<ControllerRow> {
        let Some((kind, slot, value)) = parse_row(parent) else { return Vec::new() };
        let Some(c) = self.live_controller(slot) else { return Vec::new() };
        let count = c.outputs.len() as u32;
        match kind {
            ControllerRowKind::Output => Vec::new(),
            ControllerRowKind::Controller if count > OUTPUT_PAGE_SIZE => (0..count.div_ceil(OUTPUT_PAGE_SIZE))
                .map(|p| {
                    let start = p * OUTPUT_PAGE_SIZE;
                    let n = OUTPUT_PAGE_SIZE.min(count - start);
                    ControllerRow {
                        id: page_row_id(slot, start),
                        title: format!("Outputs {}-{}", start + 1, start + n),
                        kind: ControllerRowKind::Page,
                        icon: ControllerIcon::Group,
                        has_children: n > 0,
                        child_count: n,
                    }
                })
                .collect(),
            ControllerRowKind::Controller => self.output_rows(slot, c, 0, count),
            ControllerRowKind::Page => {
                let end = value.saturating_add(OUTPUT_PAGE_SIZE).min(count);
                self.output_rows(slot, c, value.min(count), end)
            }
        }
    }

    fn output_rows(&self, slot: u32, c: &ControllerEntry, start: u32, end: u32) -> Vec<ControllerRow> {
        (start..end)
            .map(|i| {
                let output = &c.outputs[i as usize];
                ControllerRow {
                    id: output_row_id(slot, i),
                    title: output.name.clone(),
                    kind: ControllerRowKind::Output,
                    icon: self.output_icon(output.cid),
                    has_children: false,
                    child_count: 0,
                }
            })
            .collect()
    }

    fn output_icon(&self, cid: Cid) -> ControllerIcon {
        match self.flow.source(cid) {
            None => ControllerIcon::OutputUnpatched,
            Some(s) if s.output < 0 || !self.flow.is_live(s.component) => ControllerIcon::OutputBroken,
            Some(_) => ControllerIcon::OutputPatched,
        }
    }

    /// Icons for visible rows only, after a patching or run state change. A row gone stale reads as an unpatched output.
    pub fn controller_icons(&self, rows: &[String]) -> Vec<ControllerIcon> {
        rows.iter()
            .map(|id| match parse_row(id) {
                Some((ControllerRowKind::Controller, slot, _)) => match self.live_controller(slot) {
                    Some(c) if c.running => ControllerIcon::Group,
                    _ => ControllerIcon::Stopped,
                },
                Some((ControllerRowKind::Page, ..)) => ControllerIcon::Group,
                Some((ControllerRowKind::Output, slot, index)) => self
                    .live_controller(slot)
                    .and_then(|c| c.outputs.get(index as usize))
                    .map_or(ControllerIcon::OutputUnpatched, |o| self.output_icon(o.cid)),
                None => ControllerIcon::OutputUnpatched,
            })
            .collect()
    }

    // ---- Selection -----------------------------------------------------------------------------------------------------

    /// What a set of selected rows means (F81).
    pub fn tree_selection(&self, rows: &[String]) -> TreeSelection {
        let mut selected: HashSet<u32> = HashSet::new();
        let mut outputs: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut result = TreeSelection { rows: rows.len() as u32, ..TreeSelection::default() };
        for id in rows {
            match parse_row(id) {
                Some((ControllerRowKind::Controller, slot, _)) if self.live_controller(slot).is_some() => {
                    selected.insert(slot);
                }
                Some((ControllerRowKind::Output, slot, index)) if self.live_controller(slot).is_some() => {
                    outputs.entry(slot).or_default().push(index);
                }
                Some((ControllerRowKind::Page, ..)) => result.pages += 1,
                _ => {}
            }
        }
        result.controllers = self.controller_order.iter().copied().filter(|s| selected.contains(s)).collect();
        result.outputs = self
            .controller_order
            .iter()
            .filter_map(|s| outputs.remove(s).map(|list| (*s, list)))
            .collect();
        result
    }

    /// `GetSelectedControllerOutputs`: root rows in pane order. A selected controller gives all its outputs, even if some were also selected on their own. Page rows give nothing.
    pub fn output_selection(&self, rows: &[String]) -> OutputSelection {
        let selection = self.tree_selection(rows);
        let mut items = Vec::new();
        for &slot in &self.controller_order {
            let Some(c) = self.live_controller(slot) else { continue };
            if selection.controllers.contains(&slot) {
                if !c.outputs.is_empty() {
                    items.push((slot, vec![0..c.outputs.len() as u32]));
                }
            } else if let Some((_, list)) = selection.outputs.iter().find(|(s, _)| *s == slot) {
                let count = c.outputs.len() as u32;
                let mut list: Vec<u32> = list.iter().copied().filter(|i| *i < count).collect();
                list.sort_unstable();
                list.dedup();
                let mut ranges: Vec<Range<u32>> = Vec::new();
                for i in list {
                    match ranges.last_mut() {
                        Some(r) if r.end == i => r.end = i + 1,
                        _ => ranges.push(i..i + 1),
                    }
                }
                if !ranges.is_empty() {
                    items.push((slot, ranges));
                }
            }
        }
        OutputSelection { items }
    }

    /// Every output of every controller, in pane order.
    pub fn all_outputs_selection(&self) -> OutputSelection {
        OutputSelection {
            items: self
                .controller_order
                .iter()
                .filter_map(|s| self.live_controller(*s).map(|c| (*s, c)))
                .filter(|(_, c)| !c.outputs.is_empty())
                .map(|(s, c)| (s, vec![0..c.outputs.len() as u32]))
                .collect(),
        }
    }

    /// `PopulateControllerTree(set)`: the rows for a set of outputs, with each controller and page that holds one expanded.
    pub fn reveal_outputs(&self, selection: &OutputSelection) -> ControllerReveal {
        let mut reveal = ControllerReveal::default();
        for (slot, ranges) in &selection.items {
            let Some(c) = self.live_controller(*slot) else { continue };
            reveal.expand.push(controller_row_id(*slot));
            let paged = c.outputs.len() as u32 > OUTPUT_PAGE_SIZE;
            let mut last_page = None;
            for range in ranges {
                for i in range.clone() {
                    if paged {
                        let page = i / OUTPUT_PAGE_SIZE;
                        if last_page != Some(page) {
                            last_page = Some(page);
                            reveal.expand.push(page_row_id(*slot, page * OUTPUT_PAGE_SIZE));
                        }
                    }
                    reveal.select.push(output_row_id(*slot, i));
                }
            }
        }
        reveal
    }

    // ---- Rules --------------------------------------------------------------------------------------------------------

    /// `UpdateForm` (F85, F86).
    pub fn controller_panel_state(&self, rows: &[String]) -> ControllerPanelState {
        let selection = self.tree_selection(rows);
        let live: Vec<&ControllerEntry> = selection.controllers.iter().filter_map(|s| self.live_controller(*s)).collect();
        let mut state = ControllerPanelState {
            one_controller: live.len() == 1,
            any_controller: !live.is_empty(),
            can_start: live.iter().any(|c| !c.running),
            can_stop: live.iter().any(|c| c.running),
            can_find_elements: !self.output_selection(rows).is_empty(),
            has_setup: live.len() == 1 && live[0].has_setup(),
            ..ControllerPanelState::default()
        };
        match live.len() {
            0 => {}
            1 => {
                state.type_label = live[0].type_name().to_string();
                state.outputs_label = live[0].outputs.len().to_string();
            }
            n => {
                state.type_label = format!("{n} controllers selected");
                state.outputs_label = live.iter().map(|c| c.outputs.len()).sum::<usize>().to_string();
            }
        }
        state
    }

    /// `contextMenuStripTreeView_Opening` (F92, F93).
    pub fn controller_menu_state(&self, rows: &[String]) -> ControllerMenuState {
        let selection = self.tree_selection(rows);
        let live: Vec<&ControllerEntry> = selection.controllers.iter().filter_map(|s| self.live_controller(*s)).collect();
        if !live.is_empty() {
            let one = live.len() == 1;
            return ControllerMenuState {
                show: true,
                controller_items: true,
                configure: one,
                channel_count: one,
                rename: one,
                delete: true,
                start: live.iter().any(|c| !c.running),
                stop: live.iter().any(|c| c.running),
                unpatch_controllers: live.iter().any(|c| c.outputs.iter().any(|o| self.flow.source(o.cid).is_some())),
                unpatch_controllers_title: if one { "Unpatch Controller" } else { "Unpatch Controllers" }.to_string(),
                ..ControllerMenuState::default()
            };
        }
        // Anything selected that isn't an output, such as a page row, suppresses the menu, as does nothing at all.
        let output_count: u32 = selection.outputs.iter().map(|(_, l)| l.len() as u32).sum();
        if output_count == 0 || selection.pages > 0 || output_count != selection.rows {
            return ControllerMenuState::default();
        }
        let any_patched = selection.outputs.iter().any(|(slot, list)| {
            let outputs = &self.controllers[*slot as usize].outputs;
            list.iter().any(|i| outputs.get(*i as usize).is_some_and(|o| self.output_icon(o.cid) == ControllerIcon::OutputPatched))
        });
        let one = output_count == 1;
        ControllerMenuState {
            show: true,
            insert_channels: one,
            unpatch_channels: any_patched,
            unpatch_channels_title: if one { "Unpatch Channel" } else { "Unpatch Channels" }.to_string(),
            find_elements: any_patched,
            find_elements_title: if one { "Find Patched Element" } else { "Find Patched Elements" }.to_string(),
            ..ControllerMenuState::default()
        }
    }

    // ---- Editing --------------------------------------------------------------------------------------------------------

    pub(crate) fn controllers_changed(&mut self) { // STUDIO: the controller inspector's network edits (studio_controller_network.rs)
        self.dirty.controllers = true;
        self.bump_graph();
    }

    /// F77: `AddNewControllerOfTypeWithPrompts` once its two prompts are answered. An empty name is the type's name.
    pub fn add_controller(&mut self, type_id: Uuid, name: &str, outputs: u32) -> Result<u32, String> {
        let descriptor = controller_descriptor(&type_id).ok_or_else(|| "That controller type isn't available.".to_string())?;
        let name = if name.is_empty() { descriptor.type_name } else { name };
        let instance_id = Uuid::new_v4();
        let config = ControllerConfig::new_default(&type_id, instance_id);
        let slot = self.controllers.len() as u32;
        self.controllers.push(ControllerEntry {
            id: Uuid::new_v4(),
            name: name.to_string(),
            type_id,
            instance_id,
            outputs: Vec::new(),
            alive: true,
            // `Add` matches the execution state, and it starts for real at OK, when the working copy reaches the running system.
            running: true,
            running_at_open: true,
            is_new: true,
            config,
            config_changed: false,
        });
        self.controller_order.push(slot);
        self.grow_outputs(slot, outputs);
        self.controllers_changed();
        Ok(slot)
    }

    /// `OutputCount`'s growing half: new outputs named by position, with identity and a data flow component each.
    fn grow_outputs(&mut self, slot: u32, to: u32) {
        let c = &mut self.controllers[slot as usize];
        c.outputs.reserve((to as usize).saturating_sub(c.outputs.len()));
        while (c.outputs.len() as u32) < to {
            let id = Uuid::new_v4();
            let cid = self.flow.add_component(id, CompKind::Output, 0);
            let name = default_output_name(c.outputs.len());
            c.outputs.push(OutputEntry { name, id, cid });
        }
    }

    pub fn slot_of(&self, id: Uuid) -> Option<u32> {
        self.controller_order.iter().copied().find(|s| self.controllers[*s as usize].id == id)
    }

    /// F88: `OutputCount = value`. Growing names the new outputs; shrinking drops the last ones and their patches, with no warning. The rest keep their names.
    pub fn set_output_count(&mut self, id: Uuid, count: u32) {
        let Some(slot) = self.slot_of(id) else { return };
        let current = self.controllers[slot as usize].outputs.len() as u32;
        if count == current {
            return;
        }
        if count > current {
            self.grow_outputs(slot, count);
        } else {
            let removed = self.controllers[slot as usize].outputs.split_off(count as usize);
            let any_patched = removed.iter().any(|o| self.flow.source(o.cid).is_some());
            for o in removed {
                self.flow.remove_component(o.cid);
            }
            if any_patched {
                self.patching_changed();
            }
        }
        self.controllers_changed();
    }

    /// F94: `InsertOutputsAt`. Existing outputs from `at` on keep their identity and patches and move down. All outputs are renamed, unless the module names them.
    pub fn insert_outputs(&mut self, slot: u32, at: u32, count: u32) {
        let Some(c) = self.controllers.get(slot as usize).filter(|c| c.alive) else { return };
        if count == 0 || at as usize > c.outputs.len() {
            return;
        }
        let mut fresh = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let id = Uuid::new_v4();
            let cid = self.flow.add_component(id, CompKind::Output, 0);
            fresh.push(OutputEntry { name: String::new(), id, cid });
        }
        let c = &mut self.controllers[slot as usize];
        // One splice instead of removing and re-adding everything from the insertion point on.
        c.outputs.splice(at as usize..at as usize, fresh);
        self.update_output_names(slot);
        self.controllers_changed();
    }

    /// F95: `RemoveOutputs`, every listed output, in one pass. The rest are re-indexed by position, keeping their identity and patches, and renamed unless the module names them.
    pub fn remove_outputs(&mut self, slot: u32, indexes: &[u32]) {
        let Some(c) = self.controllers.get(slot as usize).filter(|c| c.alive) else { return };
        let remove: HashSet<u32> = indexes.iter().copied().filter(|i| (*i as usize) < c.outputs.len()).collect();
        if remove.is_empty() {
            return;
        }
        let old = std::mem::take(&mut self.controllers[slot as usize].outputs);
        let mut kept = Vec::with_capacity(old.len() - remove.len());
        let mut any_patched = false;
        for (i, o) in old.into_iter().enumerate() {
            if remove.contains(&(i as u32)) {
                any_patched |= self.flow.source(o.cid).is_some();
                self.flow.remove_component(o.cid);
            } else {
                kept.push(o);
            }
        }
        self.controllers[slot as usize].outputs = kept;
        self.update_output_names(slot);
        if any_patched {
            self.patching_changed();
        }
        self.controllers_changed();
    }

    /// `Remove Channels` from the context menu: every selected output row, grouped by controller. Nothing happens if a controller is selected too.
    pub fn remove_selected_outputs(&mut self, rows: &[String]) -> bool {
        let selection = self.tree_selection(rows);
        if !selection.controllers.is_empty() {
            return false;
        }
        for (slot, list) in selection.outputs {
            self.remove_outputs(slot, &list);
        }
        true
    }

    /// `UpdateOutputNames`: `Output {n}` by position, or what E1.31 calls them.
    pub(crate) fn update_output_names(&mut self, slot: u32) {
        let c = &mut self.controllers[slot as usize];
        if let Some(ControllerConfig::E131(config)) = &c.config {
            let Some(universes) = &config.universes else { return };
            for (i, o) in c.outputs.iter_mut().enumerate() {
                o.name = format!("Output #{}", i + 1);
            }
            let target = config.unicast.as_deref().filter(|u| !u.is_empty()).unwrap_or("Multicast");
            for u in universes {
                for x in u.start..u.start.saturating_add(u.size) {
                    if let Some(o) = usize::try_from(x).ok().and_then(|x| c.outputs.get_mut(x)) {
                        o.name = format!("#{} {}-{}: {target}", x + 1, u.universe, x - u.start + 2);
                    }
                }
            }
            return;
        }
        for (i, o) in c.outputs.iter_mut().enumerate() {
            o.name = default_output_name(i);
        }
    }

    /// F89: no uniqueness rule. An empty name is ignored.
    pub fn rename_controller(&mut self, id: Uuid, name: &str) {
        let Some(slot) = self.slot_of(id) else { return };
        if name.is_empty() || self.controllers[slot as usize].name == name {
            return;
        }
        self.controllers[slot as usize].name = name.to_string();
        self.controllers_changed();
    }

    /// F90: `OutputControllers.Remove`: `Stop`, all outputs and their patches go, then the controller.
    pub fn delete_controllers(&mut self, core: Option<&AppCore>, slots: &[u32]) {
        let mut any = false;
        let mut any_patched = false;
        for &slot in slots {
            let Some(c) = self.controllers.get_mut(slot as usize).filter(|c| c.alive) else { continue };
            c.alive = false;
            any = true;
            let was_new = c.is_new;
            let (id, type_id, instance_id) = (c.id, c.type_id, c.instance_id);
            let outputs = std::mem::take(&mut c.outputs);
            for o in outputs {
                any_patched |= self.flow.source(o.cid).is_some();
                self.flow.remove_component(o.cid);
            }
            if !was_new {
                if let Some(core) = core {
                    if self.hardware_allowed {
                        core.playback.set_controller_running(&id, false);
                    }
                }
                self.removed_controller_data.push((type_id, instance_id));
            }
        }
        if any {
            self.controller_order.retain(|s| self.controllers[*s as usize].alive);
            self.controllers_changed();
        }
        if any_patched {
            self.patching_changed();
        }
    }

    /// F78, applied to the working copy at once, so a later repopulate can't lose it and Cancel still reverts it.
    pub fn move_controllers(&mut self, slots: &[u32], up: bool) {
        let selected: HashSet<u32> = slots.iter().copied().collect();
        let moved = move_in_order(&self.controller_order, &selected, up);
        if moved != self.controller_order {
            self.controller_order = moved;
            self.dirty.controllers = true;
        }
    }

    /// F79: root rows dragged among root rows. `index` is the proposed position in the pane.
    pub fn drop_controllers(&mut self, slots: &[u32], index: u32) {
        let selected: HashSet<u32> = slots.iter().copied().collect();
        let before = self.controller_order[..(index as usize).min(self.controller_order.len())]
            .iter()
            .filter(|s| !selected.contains(s))
            .count();
        let moving: Vec<u32> = self.controller_order.iter().copied().filter(|s| selected.contains(s)).collect();
        let mut rest: Vec<u32> = self.controller_order.iter().copied().filter(|s| !selected.contains(s)).collect();
        rest.splice(before..before, moving);
        if rest != self.controller_order {
            self.controller_order = rest;
            self.dirty.controllers = true;
        }
    }

    /// F91's Start: each selected controller not running. Hardware only moves when the app isn't muted. `core` is `None` in tests.
    pub fn start_controllers(&mut self, core: Option<&AppCore>, slots: &[u32]) {
        self.set_running(core, slots, true);
    }

    /// F91's Stop: each selected controller running.
    pub fn stop_controllers(&mut self, core: Option<&AppCore>, slots: &[u32]) {
        self.set_running(core, slots, false);
    }

    fn set_running(&mut self, core: Option<&AppCore>, slots: &[u32], running: bool) {
        for &slot in slots {
            let Some(c) = self.controllers.get_mut(slot as usize).filter(|c| c.alive && c.running != running) else { continue };
            c.running = running;
            let (id, is_new) = (c.id, c.is_new);
            self.dirty.run_state = true;
            // A controller added this session isn't in the running system yet, so it takes this state at OK.
            if let (Some(core), false, true) = (core, is_new, self.hardware_allowed) {
                core.playback.set_controller_running(&id, running);
            }
        }
    }

    /// The working list for saving: controllers in pane order.
    pub(crate) fn controller_list(&self) -> Vec<Controller> {
        self.controller_order
            .iter()
            .filter_map(|s| self.live_controller(*s))
            .map(|c| Controller {
                name: c.name.clone(),
                hardware_type_id: c.type_id,
                hardware_instance_id: c.instance_id,
                id: c.id,
                outputs: c.outputs.iter().map(|o| ControllerOutput { name: o.name.clone(), id: o.id }).collect(),
            })
            .collect()
    }

    /// Module data for controllers new or edited this session.
    pub(crate) fn controller_store_entries(&self) -> Vec<ModuleEntry> {
        let mut out = Vec::new();
        for c in self.controllers.iter().filter(|c| c.alive && (c.is_new || c.config_changed)) {
            let Some(descriptor) = controller_descriptor(&c.type_id) else { continue };
            let Some(data) = c.config.as_ref().and_then(ControllerConfig::to_xml) else { continue };
            out.push(ModuleEntry::new(descriptor.data_model_type.unwrap_or_default(), &c.type_id, &c.instance_id, descriptor.assembly_name, data));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::patching::PatchOptions;
    use super::super::test_support::*;
    use super::*;

    fn order_names(s: &SetupSession) -> Vec<String> {
        s.controller_rows().into_iter().map(|r| r.title).collect()
    }

    fn four() -> SetupSession {
        let mut data = system(&[], 0);
        for name in ["A", "B", "C", "D"] {
            data.controllers.push(controller(name, 1));
        }
        SetupSession::from_system(data, &HashSet::new(), false)
    }

    #[test]
    fn moving_follows_upstreams_expected_results() {
        let run = |selected: &[u32], up: bool| {
            let mut s = four();
            s.move_controllers(selected, up);
            order_names(&s).concat()
        };
        assert_eq!(run(&[1, 2], true), "BCAD");
        assert_eq!(run(&[1, 2], false), "ADBC");
        assert_eq!(run(&[0, 2], true), "ACBD");
        assert_eq!(run(&[0, 1], true), "ABCD");
        assert_eq!(run(&[2, 3], false), "ABCD");
    }

    #[test]
    fn order_survives_a_later_delete() {
        let mut s = four();
        s.move_controllers(&[1], true);
        s.delete_controllers(None, &[3]);
        assert_eq!(order_names(&s).concat(), "BAC");
        assert_eq!(s.controller_list().iter().map(|c| c.name.clone()).collect::<Vec<_>>().concat(), "BAC");
    }

    #[test]
    fn dragging_roots_among_roots() {
        let mut s = four();
        s.drop_controllers(&[0, 1], 4);
        assert_eq!(order_names(&s).concat(), "CDAB");
        let mut s = four();
        s.drop_controllers(&[3], 0);
        assert_eq!(order_names(&s).concat(), "DABC");
    }

    #[test]
    fn page_rows_follow_the_pinned_values() {
        // ControllerTreeVirtualizationTests.
        let cases: [(usize, usize, &str); 6] = [
            (1, 1, "Output 1"),
            (4999, 4999, "Output 4999"),
            (5000, 5000, "Output 5000"),
            (5001, 2, "Outputs 5001-5001"),
            (10000, 2, "Outputs 5001-10000"),
            (15000, 3, "Outputs 10001-15000"),
        ];
        for (outputs, rows, last) in cases {
            let s = session(&[], outputs);
            let root = &s.controller_rows()[0];
            let children = s.controller_children(&root.id);
            assert_eq!(children.len(), rows, "{outputs}");
            assert_eq!(root.child_count as usize, rows, "{outputs}");
            assert_eq!(children.last().unwrap().title, last, "{outputs}");
        }
        let s = session(&[], 5001);
        let pages = s.controller_children("c0");
        assert_eq!(s.controller_children(&pages[0].id).len(), 5000);
        let tail = s.controller_children(&pages[1].id);
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].id, "o0:5000");
    }

    #[test]
    fn a_selected_controller_means_all_its_outputs_and_pages_mean_nothing() {
        let s = session(&[], 6000);
        let all = s.output_selection(&["c0".to_string(), "o0:3".to_string()]);
        assert_eq!(all.items, vec![(0, vec![0..6000])]);
        assert!(s.output_selection(&["p0:0".to_string()]).is_empty());
        let some = s.output_selection(&["o0:5".to_string(), "o0:4".to_string(), "o0:9".to_string(), "o0:99999".to_string()]);
        assert_eq!(some.items, vec![(0, vec![4..6, 9..10])]);
        assert_eq!(some.output_count(), 3);
    }

    #[test]
    fn stats_for_a_whole_controller_dont_need_a_set_of_indexes() {
        let s = session(&[], 170_000);
        let stats = s.controller_stats(&s.output_selection(&["c0".to_string()]));
        assert_eq!((stats.outputs, stats.unpatched), (170_000, 170_000));
        assert_eq!((stats.first_label.as_str(), stats.last_label.as_str()), ("Main #1", "Main #170000"));
    }

    #[test]
    fn panel_labels_and_rules() {
        let mut s = four();
        let none = s.controller_panel_state(&[]);
        assert_eq!((none.type_label.as_str(), none.outputs_label.as_str()), ("", ""));
        assert!(!none.any_controller && !none.can_find_elements);

        let one = s.controller_panel_state(&["c1".to_string()]);
        assert_eq!((one.type_label.as_str(), one.outputs_label.as_str()), ("Dummy Lighting (for testing)", "1"));
        assert!(one.one_controller && one.can_start && !one.can_stop && one.can_find_elements && one.has_setup);

        let two = s.controller_panel_state(&["c1".to_string(), "c2".to_string()]);
        assert_eq!((two.type_label.as_str(), two.outputs_label.as_str()), ("2 controllers selected", "2"));
        assert!(!two.one_controller && two.any_controller);

        s.start_controllers(None, &[1]);
        let mixed = s.controller_panel_state(&["c1".to_string(), "c2".to_string()]);
        assert!(mixed.can_start && mixed.can_stop);
        assert!(s.dirty.run_state);
        assert_eq!(s.controller_rows()[1].icon, ControllerIcon::Group);
        assert_eq!(s.controller_rows()[2].icon, ControllerIcon::Stopped);

        // Only outputs selected: blank labels, but Find still works.
        let outputs = s.controller_panel_state(&["o1:0".to_string()]);
        assert_eq!(outputs.type_label, "");
        assert!(outputs.can_find_elements && !outputs.any_controller);
    }

    #[test]
    fn menu_for_controllers_with_wording() {
        let mut s = session(&["L"], 2);
        let leaf = s.row_path_of(s.tree.live_nodes().next().unwrap().0);
        let one = s.controller_menu_state(&["c0".to_string()]);
        assert!(one.show && one.controller_items && one.configure && one.delete && one.start && !one.stop && !one.unpatch_controllers);
        assert_eq!(one.unpatch_controllers_title, "Unpatch Controller");
        s.patch_elements(&[leaf], &s.all_outputs_selection(), &PatchOptions::default(), false);
        assert!(s.controller_menu_state(&["c0".to_string()]).unpatch_controllers);

        let mut s = four();
        s.start_controllers(None, &[0]);
        let two = s.controller_menu_state(&["c0".to_string(), "c1".to_string(), "o0:0".to_string()]);
        assert!(two.controller_items && !two.configure && !two.rename && two.delete && two.start && two.stop);
        assert_eq!(two.unpatch_controllers_title, "Unpatch Controllers");
    }

    #[test]
    fn menu_for_outputs_with_wording() {
        let mut s = session(&["L"], 3);
        let leaf = s.row_path_of(s.tree.live_nodes().next().unwrap().0);
        s.patch_elements(&[leaf], &OutputSelection::single(0, 0..1), &PatchOptions::default(), false);

        let one = s.controller_menu_state(&["o0:0".to_string()]);
        assert!(one.show && !one.controller_items && one.insert_channels && one.unpatch_channels && one.find_elements);
        assert_eq!((one.unpatch_channels_title.as_str(), one.find_elements_title.as_str()), ("Unpatch Channel", "Find Patched Element"));

        let unpatched = s.controller_menu_state(&["o0:1".to_string()]);
        assert!(unpatched.show && !unpatched.unpatch_channels && !unpatched.find_elements);

        let two = s.controller_menu_state(&["o0:0".to_string(), "o0:1".to_string()]);
        assert!(!two.insert_channels && two.unpatch_channels);
        assert_eq!((two.unpatch_channels_title.as_str(), two.find_elements_title.as_str()), ("Unpatch Channels", "Find Patched Elements"));

        // A page row or nothing at all suppresses the menu.
        assert!(!s.controller_menu_state(&["p0:0".to_string(), "o0:0".to_string()]).show);
        assert!(!s.controller_menu_state(&[]).show);
    }

    #[test]
    fn shrinking_drops_patches_and_growing_names_by_position() {
        let mut s = session(&["A", "B", "C"], 3);
        let leaves: Vec<String> = s.tree.roots.iter().map(|n| s.row_path_of(*n)).collect();
        s.patch_elements(&leaves, &s.all_outputs_selection(), &PatchOptions::default(), false);
        let id = s.controllers[0].id;
        s.set_output_count(id, 1);
        assert_eq!(s.flow.to_patches().len(), 1);
        assert_eq!(s.controllers[0].outputs.len(), 1);
        s.set_output_count(id, 3);
        assert_eq!(s.controllers[0].outputs[2].name, "Output 3");
        assert_eq!(s.flow.to_patches().len(), 1);
    }

    #[test]
    fn inserting_keeps_identity_and_patches_and_renumbers() {
        let mut s = session(&["A", "B", "C"], 3);
        let leaves: Vec<String> = s.tree.roots.iter().map(|n| s.row_path_of(*n)).collect();
        s.patch_elements(&leaves, &s.all_outputs_selection(), &PatchOptions::default(), false);
        let old_second = s.controllers[0].outputs[1].id;
        let old_cid = s.controllers[0].outputs[1].cid;
        s.insert_outputs(0, 1, 2);
        let outputs = &s.controllers[0].outputs;
        assert_eq!(outputs.len(), 5);
        assert_eq!(outputs[3].id, old_second);
        assert!(s.flow.source(old_cid).is_some());
        assert_eq!(s.output_lookup().get(&old_cid), Some(&(0, 3)));
        assert!(s.flow.source(outputs[1].cid).is_none());
        let names: Vec<&str> = outputs.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["Output 1", "Output 2", "Output 3", "Output 4", "Output 5"]);
    }

    #[test]
    fn removing_reindexes_and_keeps_survivors() {
        let mut s = session(&["A", "B", "C"], 3);
        let leaves: Vec<String> = s.tree.roots.iter().map(|n| s.row_path_of(*n)).collect();
        s.patch_elements(&leaves, &s.all_outputs_selection(), &PatchOptions::default(), false);
        let last = s.controllers[0].outputs[2].cid;
        assert!(s.remove_selected_outputs(&["o0:0".to_string(), "o0:1".to_string()]));
        assert_eq!(s.controllers[0].outputs.len(), 1);
        assert_eq!(s.controllers[0].outputs[0].name, "Output 1");
        assert!(s.flow.source(last).is_some());
        assert_eq!(s.output_lookup().get(&last), Some(&(0, 0)));
        assert_eq!(s.flow.to_patches().len(), 1);
        assert!(!s.remove_selected_outputs(&["c0".to_string()]));
    }

    #[test]
    fn e131_names_its_own_outputs() {
        let mut s = session(&[], 0);
        let slot = s.add_controller(vixen_output::ids::E131, "E", 3).unwrap();
        s.insert_outputs(slot, 0, 1);
        let names: Vec<String> = s.controllers[slot as usize].outputs.iter().map(|o| o.name.clone()).collect();
        // A new module has one universe, starting at output 1 (the constructor's offset) with 510 outputs.
        assert_eq!(names[0], "Output #1");
        assert_eq!(names[1], "#2 1-2: Multicast");
    }

    #[test]
    fn adding_uses_the_type_name_and_makes_outputs_and_an_entry() {
        let mut s = session(&[], 0);
        let slot = s.add_controller(vixen_output::ids::DUMMY_LIGHTING, "", 2).unwrap();
        let rows = s.controller_rows();
        assert_eq!(rows[0].title, "Dummy Lighting (for testing)");
        assert_eq!(rows[0].child_count, 2);
        assert_eq!(s.controllers[slot as usize].outputs[1].name, "Output 2");
        assert_eq!(s.controller_store_entries().len(), 1);
        assert!(s.is_dirty());
        assert!(s.add_controller(Uuid::new_v4(), "x", 1).is_err());
        assert_eq!(output_limit(&vixen_output::ids::OPEN_DMX), 512);
        assert_eq!(output_limit(&vixen_output::ids::DUMMY_LIGHTING), i32::MAX as u32);
    }

    #[test]
    fn deleting_takes_patches_with_it() {
        let mut s = session(&["A"], 1);
        let leaf = s.row_path_of(s.tree.roots[0]);
        s.patch_elements(&[leaf], &s.all_outputs_selection(), &PatchOptions::default(), false);
        s.delete_controllers(None, &[0]);
        assert!(s.controller_rows().is_empty());
        assert!(s.flow.to_patches().is_empty());
        assert_eq!(s.removed_controller_data.len(), 1);
    }

    #[test]
    fn reveal_expands_controllers_and_pages() {
        let s = session(&[], 6000);
        let reveal = s.reveal_outputs(&OutputSelection::from_indexes(&s.controller_order, vec![vec![0, 5500]]));
        assert_eq!(reveal.expand, ["c0", "p0:0", "p0:5000"]);
        assert_eq!(reveal.select, ["o0:0", "o0:5500"]);
    }

    #[test]
    fn type_list_is_sorted() {
        let names: Vec<&str> = controller_types().into_iter().map(|t| t.1).collect();
        assert_eq!(
            names,
            [
                "DDP",
                "Debugging Values",
                "DMX Open",
                "DMX Pro",
                "Dummy Lighting (for testing)",
                "Elexol Ethernet I/O 24",
                "Generic Serial",
                "Launcher",
                "RDS",
                "Renard",
                "Streaming ACN (E1.31)"
            ]
        );
    }

    #[test]
    fn stale_rows_are_harmless() {
        let s = session(&[], 1);
        assert!(s.controller_children("c9").is_empty());
        assert!(s.controller_children("nonsense").is_empty());
        assert!(s.output_selection(&["c9".to_string(), "o9:0".to_string(), "x".to_string()]).is_empty());
        assert_eq!(s.controller_icons(&["o9:9".to_string()]), [ControllerIcon::OutputUnpatched]);
    }

    #[test]
    fn running_state_at_open_comes_from_what_is_passed() {
        let mut data = system(&[], 0);
        let c = controller("X", 1);
        let id = c.id;
        data.controllers.push(c);
        let s = SetupSession::from_system(data, &[id].into_iter().collect(), false);
        assert_eq!(s.controller_rows()[0].icon, ControllerIcon::Group);
        assert!(!s.is_dirty());
    }
}
