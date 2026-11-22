//! Reading and writing module data in the profile, keeping whatever the file says that typed data doesn't.

use super::world::WorldChanges;
use crate::app::AppCore;
use std::collections::HashSet;
use uuid::Uuid;
use vixen_io::{Controller, ModuleEntry, ModuleStore, SystemData};
use vixen_output::filters::FilterConfig;
use vixen_output::ids;
use vixen_output::modules::ControllerConfig;
use vixen_xml::XElement;

pub fn find_controller<'a>(system: &'a SystemData, controller: &Uuid) -> Option<&'a Controller> {
    system.controllers.iter().find(|c| c.id == *controller)
}

/// A controller's data as it stands. A controller with no entry in the store has what a new instance gets.
pub fn controller_config(system: &SystemData, controller: &Uuid) -> Option<ControllerConfig> {
    let c = find_controller(system, controller)?;
    match system.module_store.instance(c.hardware_type_id, c.hardware_instance_id) {
        Some(entry) => ControllerConfig::from_xml(&c.hardware_type_id, &entry.element),
        None => ControllerConfig::new_default(&c.hardware_type_id, c.hardware_instance_id),
    }
}

/// Writes a controller's data into the store. The debug controller has none.
pub fn store_controller_config(store: &mut ModuleStore, config: &ControllerConfig) {
    let Some(new_plain) = config.to_xml() else {
        return;
    };
    let module_ids = *config.ids();
    match store.instance_mut(module_ids.type_id, module_ids.instance_id) {
        Some(entry) => {
            let old_plain = ControllerConfig::from_xml(&module_ids.type_id, &entry.element).and_then(|c| c.to_xml());
            merge_entry(entry, old_plain, new_plain);
        }
        None => {
            if let Some(descriptor) = ids::controller_descriptor(&module_ids.type_id) {
                store.set_instance(ModuleEntry { element: ids::module_element(descriptor, &module_ids, new_plain) });
            // STUDIO-BEGIN plugins-store-config: plugin controller data is written under the plugin's type id
            } else if matches!(config, ControllerConfig::External(_)) {
                use vixen_output::modules::studio_external::{ASSEMBLY_NAME, DATA_MODEL_TYPE};
                store.set_instance(ModuleEntry::new(DATA_MODEL_TYPE, &module_ids.type_id, &module_ids.instance_id, ASSEMBLY_NAME, new_plain));
            // STUDIO-END plugins-store-config
            }
        }
    }
}

/// A filter's data in the store. A filter with no entry has what a new instance gets.
pub fn filter_config_in(store: &ModuleStore, type_id: &Uuid, instance: &Uuid) -> Option<FilterConfig> {
    match store.instance(*type_id, *instance) {
        Some(entry) => FilterConfig::from_xml(type_id, &entry.element),
        None => FilterConfig::new_default(type_id, *instance),
    }
}

/// A filter's data as it stands.
pub fn filter_config(system: &SystemData, instance: &Uuid) -> Option<FilterConfig> {
    let filter = system.filters.iter().find(|f| f.instance_id == *instance)?;
    filter_config_in(&system.module_store, &filter.type_id, instance)
}

/// Writes a filter's data into the store.
pub fn store_filter_config(store: &mut ModuleStore, config: &FilterConfig) {
    let module_ids = *config.ids();
    let new_plain = config.to_xml();
    match store.instance_mut(module_ids.type_id, module_ids.instance_id) {
        Some(entry) => {
            let old_plain = FilterConfig::from_xml(&module_ids.type_id, &entry.element).map(|c| c.to_xml());
            merge_entry(entry, old_plain, new_plain);
        }
        None => {
            if let Some(descriptor) = ids::filter_descriptor(&module_ids.type_id) {
                store.set_instance(ModuleEntry { element: ids::module_element(descriptor, &module_ids, new_plain) });
            }
        }
    }
}

fn merge_entry(entry: &mut ModuleEntry, old_plain: Option<XElement>, new_plain: XElement) {
    match old_plain {
        Some(old_plain) => entry.merge_data(&old_plain, new_plain),
        None => entry.set_data(new_plain),
    }
}

/// Which controllers are running now.
pub fn running_controllers(core: &AppCore) -> HashSet<Uuid> {
    core.playback.with_output(|_, runner| {
        (0..runner.controller_count())
            .filter(|i| runner.is_running(*i))
            .filter_map(|i| runner.controller_id(i))
            .collect()
    })
}

/// Hands what a call changed in the profile to the running graph and controllers. A running controller stops, takes its data and
/// starts again between frames, and a stopped one stays stopped (F88, F95). A filter that keeps its output count swaps atomically,
/// so the frame thread never sees half of it and nothing rebuilds (F150). Anything else builds the graph again.
pub fn follow_changes(core: &AppCore, changes: &WorldChanges) {
    let (controllers, filters): (Vec<(Uuid, ControllerConfig)>, Vec<FilterConfig>) = {
        let system = core.system.read();
        (
            changes.controllers.iter().filter_map(|id| Some((*id, controller_config(&system, id)?))).collect(),
            changes.filters.iter().filter_map(|id| filter_config(&system, id)).collect(),
        )
    };
    let mut rebuild = changes.structure;
    core.playback.with_output(|graph, runner| {
        for (id, config) in controllers {
            if let Some(i) = graph.controller_index(&id) {
                graph.set_controller_config(i, config.clone());
            }
            if let Some(i) = runner.controller_index(&id) {
                let _ = runner.set_config(i, config);
            }
        }
        for config in filters {
            let instance = config.ids().instance_id;
            rebuild |= !graph.set_filter_config(&instance, config);
        }
    });
    if rebuild {
        rebuild_output_graph(core);
    }
}

/// Builds the running graph again from the profile, after patches or filters changed. Controllers keep their threads.
pub fn rebuild_output_graph(core: &AppCore) {
    let graph = {
        let system = core.system.read();
        crate::show::build_output_graph(&system, &core.playback.elements().ids)
    };
    core.playback.with_output(|current, _| *current = graph);
}

/// What a new element property of each type is stored as.
pub fn property_module_names(type_id: &Uuid) -> Option<(&'static str, &'static str)> {
    use vixen_core::element::property_ids as p;
    Some(match *type_id {
        p::COLOR => ("VixenModules.Property.Color.ColorData, Color", "Module.Property.Color"),
        p::LOCATION => ("VixenModules.Property.Location.LocationData, Location", "Module.Property.Location"),
        p::ORIENTATION => ("VixenModules.Property.Orientation.OrientationData, Orientation", "Module.Property.Orientation"),
        p::ORDER => ("VixenModules.Property.Order.OrderData, Order", "Module.Property.Order"),
        p::GRID => ("VixenModules.Property.Grid.Data, Grid", "Module.Property.Grid"),
        p::FACE => ("VixenModules.Property.Face.FaceData, Face", "Module.Property.Face"),
        p::STATE => ("VixenModules.Property.State.StateData, State", "Module.Property.State"),
        p::INTELLIGENT_FIXTURE => ("VixenModules.Property.IntelligentFixture.IntelligentFixtureData, IntelligentFixture", "Module.Property.IntelligentFixture"),
        _ => return None,
    })
}
