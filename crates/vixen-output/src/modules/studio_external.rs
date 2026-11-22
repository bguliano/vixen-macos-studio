//! Studio: controller types that live outside this crate. Vixen Studio's plugin host (`crates/studio-plugins`) registers
//! one provider at startup, and its controller types then load, save, run and report like built-in ones. Without a
//! provider (vanilla, tests) nothing here claims anything, so module data of an unknown type behaves exactly as before.
//!
//! An external controller's module data is kept as the element `ModuleStore.xml` holds, so it saves back unchanged
//! whether or not the plugin that owns it is installed. See `studio/PLUGINS-DESIGN.md`.

use super::{ConfigMismatch, ControllerConfig, OutputModule};
use crate::policy::DataPolicy;
use crate::xmlutil::ModuleIds;
use parking_lot::{Mutex, RwLock};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;
use vixen_xml::XElement;

/// The `<Module>` attributes external module data is written with. Windows Vixen can't resolve the type, so it skips
/// the entry while loading (and leaves it out of its own next save); this port keeps it.
pub const DATA_MODEL_TYPE: &str = "VixenStudio.Plugins.PluginControllerData, StudioPlugins";
pub const ASSEMBLY_NAME: &str = "Module.Controller.StudioPlugins";

/// An external controller's module data: its ids and the data element, kept whole.
#[derive(Clone, Debug, PartialEq)]
pub struct ExternalConfig {
    pub ids: ModuleIds,
    pub data: XElement,
}

impl ExternalConfig {
    /// The data element with the current ids written into its `ModuleInstanceId` / `ModuleTypeId` members.
    pub fn to_xml(&self) -> XElement {
        let mut data = self.data.clone();
        for (name, id) in [("ModuleInstanceId", self.ids.instance_id), ("ModuleTypeId", self.ids.type_id)] {
            if let Some(member) = data.child_mut(name) {
                *member = XElement::with_text("", name, id.to_string());
            }
        }
        data
    }
}

/// A controller type a provider offers, for the Add Controller list and `controller_types`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExternalType {
    pub type_id: Uuid,
    pub name: String,
    pub default_outputs: u32,
    pub max_outputs: u32,
}

/// What Studio's plugin host implements.
pub trait ExternalControllers: Send + Sync {
    /// Whether module data of `type_id` belongs to the provider. `data` is the stored data element, when there is one:
    /// a provider also claims data it wrote for a type whose plugin isn't installed now.
    fn claims(&self, type_id: &Uuid, data: Option<&XElement>) -> bool;
    /// The data a new instance of an installed type gets.
    fn new_default(&self, type_id: &Uuid, ids: &ModuleIds) -> Option<XElement>;
    /// The module for a configuration. Must be cheap: nothing may start until `OutputModule::start`.
    fn create(&self, config: ExternalConfig) -> Box<dyn OutputModule>;
    fn data_policy(&self, config: &ExternalConfig) -> DataPolicy;
    /// Installed types, in no particular order.
    fn types(&self) -> Vec<ExternalType>;
    /// Why a controller with this data can't send right now (its plugin missing, disabled, too old), if it can't.
    fn problem(&self, config: &ExternalConfig) -> Option<String>;
    /// The display name for a type, also for data whose plugin isn't installed.
    fn type_name(&self, type_id: &Uuid, data: Option<&XElement>) -> Option<String>;
    /// Called after the profile's system config was saved, with every external controller's data.
    fn system_saved(&self, profile: &Path, configs: &[ExternalConfig]);
    /// What the inspector and `controller_network` show for the controller, as a JSON object.
    fn describe(&self, config: &ExternalConfig) -> String;
    /// New data with the configuration replaced by `settings` (JSON text), or the field errors as JSON text
    /// (`[{field, message}]`) when it doesn't fit the type's schema.
    fn reconfigure(&self, config: &ExternalConfig, settings: &str) -> Result<XElement, String>;
}

static PROVIDER: RwLock<Option<Arc<dyn ExternalControllers>>> = RwLock::new(None);

/// Installs (or with `None` removes) the provider.
pub fn register(provider: Option<Arc<dyn ExternalControllers>>) {
    *PROVIDER.write() = provider;
}

pub fn provider() -> Option<Arc<dyn ExternalControllers>> {
    PROVIDER.read().clone()
}

pub(crate) fn new_default(type_id: &Uuid, instance_id: Uuid) -> Option<ControllerConfig> {
    let ids = ModuleIds::new(*type_id, instance_id);
    let data = provider()?.new_default(type_id, &ids)?;
    Some(ControllerConfig::External(ExternalConfig { ids, data }))
}

pub(crate) fn from_xml(type_id: &Uuid, data: &XElement) -> Option<ControllerConfig> {
    let p = provider()?;
    if !p.claims(type_id, Some(data)) {
        return None;
    }
    let ids = ModuleIds::new(*type_id, crate::xmlutil::ModuleIds::read(data).instance_id);
    Some(ControllerConfig::External(ExternalConfig { ids, data: data.clone() }))
}

pub(crate) fn data_policy(config: &ExternalConfig) -> DataPolicy {
    provider().map_or(DataPolicy::HIGHEST_WINS_8BIT, |p| p.data_policy(config))
}

pub(crate) fn create(config: ExternalConfig) -> Box<dyn OutputModule> {
    match provider() {
        Some(p) => p.create(config),
        None => Box::new(Silent::new(config)),
    }
}

/// Installed external types as `(type id, name)`. Names are interned, as built-in type names are `'static`.
pub fn types() -> Vec<(Uuid, &'static str)> {
    provider().map_or_else(Vec::new, |p| p.types().into_iter().map(|t| (t.type_id, intern(t.name))).collect())
}

/// An external type's name, for a type the provider knows or data it wrote.
pub fn type_name(type_id: &Uuid, config: Option<&ControllerConfig>) -> Option<&'static str> {
    let data = match config {
        Some(ControllerConfig::External(c)) => Some(&c.data),
        _ => None,
    };
    let p = provider()?;
    (data.is_some() || p.claims(type_id, None)).then(|| p.type_name(type_id, data)).flatten().map(intern)
}

/// Whether the provider owns this type (installed now).
pub fn is_external_type(type_id: &Uuid) -> bool {
    provider().is_some_and(|p| p.claims(type_id, None))
}

/// Why an external controller can't send, if it can't. `None` for built-in types.
pub fn problem(config: &ControllerConfig) -> Option<String> {
    match config {
        ControllerConfig::External(c) => provider().and_then(|p| p.problem(c)),
        _ => None,
    }
}

/// Tells the provider the profile was saved, with the external controllers' data in `configs`.
pub fn system_saved(profile: &Path, configs: &[ExternalConfig]) {
    if let Some(p) = provider() {
        p.system_saved(profile, configs);
    }
}

/// [`ExternalControllers::describe`], `None` without a provider.
pub fn describe(config: &ExternalConfig) -> Option<String> {
    provider().map(|p| p.describe(config))
}

/// [`ExternalControllers::reconfigure`].
pub fn reconfigure(config: &ExternalConfig, settings: &str) -> Result<XElement, String> {
    provider().ok_or_else(|| "[{\"field\":\"/\",\"message\":\"no plugin host is running\"}]".to_string())?.reconfigure(config, settings)
}

fn intern(name: String) -> &'static str {
    static NAMES: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut guard = NAMES.lock();
    let names = guard.get_or_insert_with(HashSet::new);
    if let Some(known) = names.get(name.as_str()) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.into_boxed_str());
    names.insert(leaked);
    leaked
}

/// What runs for external data when the provider has gone away: nothing is sent, and the status says why.
struct Silent {
    config: ExternalConfig,
    outputs: usize,
    shared: Arc<super::ModuleShared>,
}

impl Silent {
    fn new(config: ExternalConfig) -> Silent {
        Silent { config, outputs: 0, shared: super::ModuleShared::new() }
    }
}

impl OutputModule for Silent {
    fn type_id(&self) -> Uuid {
        self.config.ids.type_id
    }
    fn start(&mut self) {
        self.shared.set_status(super::ModuleStatus::Error("No plugin host is running".into()));
    }
    fn stop(&mut self) {
        self.shared.set_status(super::ModuleStatus::Stopped);
    }
    fn update(&mut self, _commands: &[crate::command::OutputCommand]) {}
    fn output_count(&self) -> usize {
        self.outputs
    }
    fn set_output_count(&mut self, count: usize) {
        self.outputs = count;
    }
    fn config(&self) -> ControllerConfig {
        ControllerConfig::External(self.config.clone())
    }
    fn set_config(&mut self, config: ControllerConfig) -> Result<(), ConfigMismatch> {
        match config {
            ControllerConfig::External(c) if c.ids.type_id == self.config.ids.type_id => {
                self.config = c;
                Ok(())
            }
            _ => Err(ConfigMismatch),
        }
    }
    fn shared(&self) -> &Arc<super::ModuleShared> {
        &self.shared
    }
}
