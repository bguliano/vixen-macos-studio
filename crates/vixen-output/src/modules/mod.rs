//! The controller modules: `OutputModule` is `IControllerModuleInstance` without the Windows forms. Each one
//! takes the commands for a frame and puts them on the wire, and each has a typed configuration that reads
//! and writes its part of `ModuleStore.xml` exactly.

pub mod ddp;
pub mod debug;
pub mod dmx_usb_pro;
pub mod dummy;
pub mod e131;
pub mod elexol;
pub mod generic_serial;
pub mod launcher;
pub mod open_dmx;
pub mod rds;
pub mod renard;
pub mod studio_external; // STUDIO: plugin controller types (studio/PLUGINS-DESIGN.md)

pub use ddp::{DdpConfig, DdpModule};
pub use debug::{DebugConfig, DebugModule};
pub use dmx_usb_pro::{DmxUsbProConfig, DmxUsbProModule};
pub use dummy::{DummyLightingConfig, DummyLightingModule, DummyLightingView, RenderStyle};
pub use e131::{E131Config, E131Counters, E131Module, E131Stats, E131UniverseStats, UniverseEntry};
pub use elexol::{ElexolConfig, ElexolModule};
pub use generic_serial::{GenericSerialConfig, GenericSerialModule};
pub use launcher::{LauncherConfig, LauncherModule};
pub use open_dmx::{OpenDmxConfig, OpenDmxDevice, OpenDmxModule};
pub use rds::{RdsConfig, RdsHardware, RdsModule};
pub use renard::{RenardConfig, RenardModule};

use crate::command::OutputCommand;
use crate::ids;
use crate::policy::DataPolicy;
use crate::xmlutil::{unwrap_module, ModuleIds};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;
use vixen_xml::XElement;

/// `IController` plus `IOutputter`: what the runner needs from a controller.
///
/// Every method but `shared` runs on the controller's own thread once it has been handed to a `ControllerRunner`.
pub trait OutputModule: Send {
    /// The module type's id.
    fn type_id(&self) -> Uuid;

    /// `Start()`. Failure doesn't return an error: as in the C#, a controller that can't reach its hardware stays
    /// in the show, stays quiet, and says why through `status`.
    fn start(&mut self);

    /// `Stop()`.
    fn stop(&mut self);

    /// `Pause()`. Most modules ignore it, as updates simply stop arriving.
    fn pause(&mut self) {}

    /// `Resume()`.
    fn resume(&mut self) {}

    /// `UpdateState(0, outputStates)`: one frame's commands, one per output.
    fn update(&mut self, commands: &[OutputCommand]);

    /// `OutputCount`.
    fn output_count(&self) -> usize;

    /// `OutputCount = value`, which the output mediator sets as outputs are added.
    fn set_output_count(&mut self, count: usize);

    /// `OutputLimit`.
    fn output_limit(&self) -> usize {
        i32::MAX as usize
    }

    /// A typed copy of the module's data, with whatever the module has changed in it since it was set.
    fn config(&self) -> ControllerConfig;

    /// `ModuleData = value`. Fails when the configuration belongs to another module type.
    fn set_config(&mut self, config: ControllerConfig) -> Result<(), ConfigMismatch>;

    /// The evaluator and combinator this module wants, `DataPolicyFactory.CreateDataPolicy()`.
    fn data_policy(&self) -> DataPolicy {
        DataPolicy::HIGHEST_WINS_8BIT
    }

    /// State other threads may read while the module runs.
    fn shared(&self) -> &Arc<ModuleShared>;

    /// The E1.31 statistics, on the module that has them.
    fn e131_counters(&self) -> Option<Arc<E131Counters>> {
        None
    }

    /// What the dummy controller's window shows, on the module that has one.
    fn dummy_view(&self) -> Option<Arc<Mutex<DummyLightingView>>> {
        None
    }

    /// The latest frame, on the debug controller.
    fn debug_frame(&self) -> Option<Arc<Mutex<Vec<OutputCommand>>>> {
        None
    }

    /// `shared().status()`.
    fn status(&self) -> ModuleStatus {
        self.shared().status()
    }
}

/// `set_config` was given the configuration of another module type.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ConfigMismatch;

impl std::fmt::Display for ConfigMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the configuration belongs to a different module type")
    }
}

impl std::error::Error for ConfigMismatch {}

/// Where a module stands.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum ModuleStatus {
    #[default]
    Stopped,
    Running,
    /// Started, but its hardware can't be reached: for example a saved `COM4`, or a port in use. The module sends
    /// nothing, and may retry.
    Unavailable(String),
    /// The module stays in the model, but what it needs doesn't exist on this platform.
    UnsupportedOnMacOs(String),
    /// A configuration the module can't work with.
    Error(String),
}

/// Something a controller wants the user to see. The C# shows these as message boxes on the thread that's starting the controller. Here
/// they wait for the UI to collect them, so the engine never waits on an alert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerMessage {
    pub title: String,
    /// The text above a scrolling box of `text`, as J1MsgBox shows. `None` is a plain alert.
    pub heading: Option<String>,
    pub text: String,
}

/// A module's state that outlives a lock on the module: readable from any thread.
#[derive(Default)]
pub struct ModuleShared {
    status: Mutex<ModuleStatus>,
    last_error: Mutex<Option<String>>,
    error_sink: Mutex<Option<Arc<dyn Fn(&str) + Send + Sync>>>,
    messages: Mutex<Vec<ControllerMessage>>,
    /// Set while stopping, so that a wait on a jammed port can give up.
    stop: AtomicBool,
    errors: AtomicU64,
    packets_sent: AtomicU64,
    bytes_sent: AtomicU64,
}

impl std::fmt::Debug for ModuleShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModuleShared").field("status", &self.status).field("last_error", &self.last_error)
            .field("errors", &self.errors).finish_non_exhaustive()
    }
}

impl ModuleShared {
    pub fn new() -> Arc<ModuleShared> {
        Arc::new(ModuleShared::default())
    }

    pub fn status(&self) -> ModuleStatus {
        self.status.lock().clone()
    }

    pub fn set_status(&self, status: ModuleStatus) {
        *self.status.lock() = status;
    }

    /// The most recent error, where the C# would have logged it.
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().clone()
    }

    /// Records an error. Errors are counted rather than thrown: like `OutputController.Update`'s catch, one bad
    /// frame loses that frame and nothing else.
    pub fn record_error(&self, message: impl Into<String>) {
        let message = message.into();
        self.record_error_without_log(message.clone());
        let sink = self.error_sink.lock().clone();
        if let Some(sink) = sink {
            // Driver-owned HTTP/DMX/launcher workers can fail between updates or after Stop. Deliver at the source,
            // without holding shared-state locks or allowing a diagnostic callback to kill that worker.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(&message)));
        }
    }

    pub(crate) fn set_error_sink(&self, sink: Option<Arc<dyn Fn(&str) + Send + Sync>>) {
        *self.error_sink.lock() = sink;
    }

    /// Runner exceptions have their own upstream lifecycle logger/message, so they are reported separately.
    pub(crate) fn record_error_without_log(&self, message: impl Into<String>) {
        self.errors.fetch_add(1, Ordering::Relaxed);
        *self.last_error.lock() = Some(message.into());
    }

    /// Leaves a message for the UI.
    pub fn post_message(&self, message: ControllerMessage) {
        self.messages.lock().push(message);
    }

    /// What's waiting to be shown. Taking them clears them.
    pub fn take_messages(&self) -> Vec<ControllerMessage> {
        std::mem::take(&mut *self.messages.lock())
    }

    pub fn error_count(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn count_packet(&self, bytes: usize) {
        self.packets_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// Packets (or serial frames) put on the wire.
    pub fn packets_sent(&self) -> u64 {
        self.packets_sent.load(Ordering::Relaxed)
    }

    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent.load(Ordering::Relaxed)
    }

    // STUDIO-BEGIN plugins-module-counters: a plugin controller reports its own packet, byte and error totals
    /// Raises the counters to totals a controller counted elsewhere (a plugin process). Never lowers them.
    pub fn raise_counters(&self, packets: u64, bytes: u64, errors: u64) {
        self.packets_sent.fetch_max(packets, Ordering::Relaxed);
        self.bytes_sent.fetch_max(bytes, Ordering::Relaxed);
        self.errors.fetch_max(errors, Ordering::Relaxed);
    }
    // STUDIO-END plugins-module-counters

    pub(crate) fn request_stop(&self, stop: bool) {
        self.stop.store(stop, Ordering::Relaxed);
    }

    pub(crate) fn stop_flag(&self) -> &AtomicBool {
        &self.stop
    }

    pub(crate) fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// The typed data of any controller module.
#[derive(Clone, Debug, PartialEq)]
pub enum ControllerConfig {
    GenericSerial(GenericSerialConfig),
    DummyLighting(DummyLightingConfig),
    E131(E131Config),
    Ddp(DdpConfig),
    Renard(RenardConfig),
    DmxUsbPro(DmxUsbProConfig),
    OpenDmx(OpenDmxConfig),
    Elexol(ElexolConfig),
    Rds(RdsConfig),
    Launcher(LauncherConfig),
    Debug(DebugConfig),
    External(studio_external::ExternalConfig), // STUDIO: a plugin controller's data, kept whole
}

impl ControllerConfig {
    /// The data a new module instance gets, from each data class's constructor. `None` for an unknown type.
    pub fn new_default(type_id: &Uuid, instance_id: Uuid) -> Option<ControllerConfig> {
        let m = ModuleIds::new(*type_id, instance_id);
        Some(match *type_id {
            ids::GENERIC_SERIAL => ControllerConfig::GenericSerial(GenericSerialConfig { ids: m, ..Default::default() }),
            ids::DUMMY_LIGHTING => ControllerConfig::DummyLighting(DummyLightingConfig { ids: m, ..Default::default() }),
            ids::E131 => ControllerConfig::E131(E131Config { ids: m, ..Default::default() }),
            ids::DDP => ControllerConfig::Ddp(DdpConfig { ids: m, ..Default::default() }),
            ids::RENARD => ControllerConfig::Renard(RenardConfig { ids: m, ..Default::default() }),
            ids::DMX_USB_PRO => ControllerConfig::DmxUsbPro(DmxUsbProConfig { ids: m, ..Default::default() }),
            ids::OPEN_DMX => ControllerConfig::OpenDmx(OpenDmxConfig { ids: m, ..Default::default() }),
            ids::ELEXOL_ETHER_IO => ControllerConfig::Elexol(ElexolConfig { ids: m, ..Default::default() }),
            ids::RDS => ControllerConfig::Rds(RdsConfig { ids: m, ..Default::default() }),
            ids::LAUNCHER => ControllerConfig::Launcher(LauncherConfig { ids: m, ..Default::default() }),
            ids::DEBUG_CONTROLLER => ControllerConfig::Debug(DebugConfig { ids: m }),
            _ => return studio_external::new_default(type_id, instance_id), // STUDIO: plugin controller types
        })
    }

    /// Reads a controller's module data, given as the `<Module>` element or the data inside it. The ids on a
    /// `<Module>` element win, as they do in `XmlModuleDataModelSerializer.ReadObject`.
    pub fn from_xml(type_id: &Uuid, e: &XElement) -> Option<ControllerConfig> {
        let (data, wrapper_ids) = unwrap_module(e);
        let mut config = match *type_id {
            ids::GENERIC_SERIAL => ControllerConfig::GenericSerial(GenericSerialConfig::from_xml(data)),
            ids::DUMMY_LIGHTING => ControllerConfig::DummyLighting(DummyLightingConfig::from_xml(data)),
            ids::E131 => ControllerConfig::E131(E131Config::from_xml(data)),
            ids::DDP => ControllerConfig::Ddp(DdpConfig::from_xml(data)),
            ids::RENARD => ControllerConfig::Renard(RenardConfig::from_xml(data)),
            ids::DMX_USB_PRO => ControllerConfig::DmxUsbPro(DmxUsbProConfig::from_xml(data)),
            ids::OPEN_DMX => ControllerConfig::OpenDmx(OpenDmxConfig::from_xml(data)),
            ids::ELEXOL_ETHER_IO => ControllerConfig::Elexol(ElexolConfig::from_xml(data)),
            ids::RDS => ControllerConfig::Rds(RdsConfig::from_xml(data)),
            ids::LAUNCHER => ControllerConfig::Launcher(LauncherConfig::from_xml(data)),
            ids::DEBUG_CONTROLLER => ControllerConfig::Debug(DebugConfig::from_xml(data)),
            _ => studio_external::from_xml(type_id, data)?, // STUDIO: plugin controller types
        };
        if let Some(w) = wrapper_ids {
            *config.ids_mut() = w;
        }
        Some(config)
    }

    /// The data element, for inside the `<Module>` element. `None` for the debug controller, which has no data.
    pub fn to_xml(&self) -> Option<XElement> {
        Some(match self {
            ControllerConfig::GenericSerial(c) => c.to_xml(),
            ControllerConfig::DummyLighting(c) => c.to_xml(),
            ControllerConfig::E131(c) => c.to_xml(),
            ControllerConfig::Ddp(c) => c.to_xml(),
            ControllerConfig::Renard(c) => c.to_xml(),
            ControllerConfig::DmxUsbPro(c) => c.to_xml(),
            ControllerConfig::OpenDmx(c) => c.to_xml(),
            ControllerConfig::Elexol(c) => c.to_xml(),
            ControllerConfig::Rds(c) => c.to_xml(),
            ControllerConfig::Launcher(c) => c.to_xml(),
            ControllerConfig::Debug(_) => return None,
            ControllerConfig::External(c) => c.to_xml(), // STUDIO: plugin controller data
        })
    }

    pub fn ids(&self) -> &ModuleIds {
        match self {
            ControllerConfig::GenericSerial(c) => &c.ids,
            ControllerConfig::DummyLighting(c) => &c.ids,
            ControllerConfig::E131(c) => &c.ids,
            ControllerConfig::Ddp(c) => &c.ids,
            ControllerConfig::Renard(c) => &c.ids,
            ControllerConfig::DmxUsbPro(c) => &c.ids,
            ControllerConfig::OpenDmx(c) => &c.ids,
            ControllerConfig::Elexol(c) => &c.ids,
            ControllerConfig::Rds(c) => &c.ids,
            ControllerConfig::Launcher(c) => &c.ids,
            ControllerConfig::Debug(c) => &c.ids,
            ControllerConfig::External(c) => &c.ids, // STUDIO: plugin controller data
        }
    }

    pub fn ids_mut(&mut self) -> &mut ModuleIds {
        match self {
            ControllerConfig::GenericSerial(c) => &mut c.ids,
            ControllerConfig::DummyLighting(c) => &mut c.ids,
            ControllerConfig::E131(c) => &mut c.ids,
            ControllerConfig::Ddp(c) => &mut c.ids,
            ControllerConfig::Renard(c) => &mut c.ids,
            ControllerConfig::DmxUsbPro(c) => &mut c.ids,
            ControllerConfig::OpenDmx(c) => &mut c.ids,
            ControllerConfig::Elexol(c) => &mut c.ids,
            ControllerConfig::Rds(c) => &mut c.ids,
            ControllerConfig::Launcher(c) => &mut c.ids,
            ControllerConfig::Debug(c) => &mut c.ids,
            ControllerConfig::External(c) => &mut c.ids, // STUDIO: plugin controller data
        }
    }

    /// The module's `DataPolicyFactory`.
    pub fn data_policy(&self) -> DataPolicy {
        match self {
            ControllerConfig::DummyLighting(c) => c.render_style.data_policy(),
            ControllerConfig::Rds(_) | ControllerConfig::Launcher(_) | ControllerConfig::Debug(_) => {
                DataPolicy::DISCARD_EXCESS_8BIT
            }
            ControllerConfig::External(c) => studio_external::data_policy(c), // STUDIO: the plugin's dataPolicy
            _ => DataPolicy::HIGHEST_WINS_8BIT,
        }
    }
}

/// Creates the module for a configuration. The module owns the configuration from here on.
pub fn create_module(config: ControllerConfig) -> Box<dyn OutputModule> {
    match config {
        ControllerConfig::GenericSerial(c) => Box::new(GenericSerialModule::new(c)),
        ControllerConfig::DummyLighting(c) => Box::new(DummyLightingModule::new(c)),
        ControllerConfig::E131(c) => Box::new(E131Module::new(c)),
        ControllerConfig::Ddp(c) => Box::new(DdpModule::new(c)),
        ControllerConfig::Renard(c) => Box::new(RenardModule::new(c)),
        ControllerConfig::DmxUsbPro(c) => Box::new(DmxUsbProModule::new(c)),
        ControllerConfig::OpenDmx(c) => Box::new(OpenDmxModule::new(c)),
        ControllerConfig::Elexol(c) => Box::new(ElexolModule::new(c)),
        ControllerConfig::Rds(c) => Box::new(RdsModule::new(c)),
        ControllerConfig::Launcher(c) => Box::new(LauncherModule::new(c)),
        ControllerConfig::Debug(c) => Box::new(DebugModule::new(c)),
        ControllerConfig::External(c) => studio_external::create(c), // STUDIO: plugin controllers
    }
}

/// `OutputController.UpdateOutputNames` for a module without named outputs: `Output 1`, `Output 2`, ...
pub fn default_output_name(index: usize) -> String {
    format!("Output {}", index + 1)
}

/// The FPP export's view of a network controller: `ControllerNetworkConfiguration`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerNetworkConfiguration {
    pub protocol: NetworkProtocol,
    pub transmission: TransmissionMethod,
    pub ip_address: Option<std::net::IpAddr>,
    pub supports_universes: bool,
    pub throttling_enabled: bool,
    pub universes: Vec<UniverseConfiguration>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NetworkProtocol {
    E131,
    Ddp,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TransmissionMethod {
    Unicast,
    Multicast,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct UniverseConfiguration {
    pub universe_number: i32,
    pub active: bool,
    pub start: i32,
    pub size: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_descriptor_has_a_default_config_and_module() {
        for d in &ids::CONTROLLER_DESCRIPTORS {
            let instance = uuid::uuid!("11111111-2222-3333-4444-555555555555");
            let config = ControllerConfig::new_default(&d.type_id, instance).expect(d.type_name);
            assert_eq!(config.ids().type_id, d.type_id);
            assert_eq!(config.ids().instance_id, instance);
            let module = create_module(config);
            assert_eq!(module.type_id(), d.type_id, "{}", d.type_name);
            assert_eq!(module.status(), ModuleStatus::Stopped, "{}", d.type_name);
        }
    }

    #[test]
    fn policies_match_each_modules_policy_class() {
        let policy = |t: &Uuid| ControllerConfig::new_default(t, Uuid::nil()).unwrap().data_policy();
        for t in [
            ids::GENERIC_SERIAL,
            ids::RENARD,
            ids::DMX_USB_PRO,
            ids::OPEN_DMX,
            ids::E131,
            ids::DDP,
            ids::ELEXOL_ETHER_IO,
            ids::DUMMY_LIGHTING,
        ] {
            assert_eq!(policy(&t), DataPolicy::HIGHEST_WINS_8BIT);
        }
        for t in [ids::RDS, ids::LAUNCHER, ids::DEBUG_CONTROLLER] {
            assert_eq!(policy(&t), DataPolicy::DISCARD_EXCESS_8BIT);
        }
        let dummy = |style| {
            ControllerConfig::DummyLighting(DummyLightingConfig { render_style: style, ..Default::default() }).data_policy()
        };
        assert_eq!(dummy(RenderStyle::Monochrome), DataPolicy::HIGHEST_WINS_8BIT);
        assert_eq!(dummy(RenderStyle::RGBMultiChannel), DataPolicy::AVERAGE_8BIT);
        assert_eq!(dummy(RenderStyle::RGBSingleChannel), DataPolicy::ONE_CHANNEL_COLOR);
    }

    #[test]
    fn output_names_default_to_one_based() {
        assert_eq!(default_output_name(0), "Output 1");
        assert_eq!(default_output_name(15), "Output 16");
    }
}
