//! Port of Vixen.Modules/Controller/DDP: one UDP packet per 1440 channels to port 4048, with the push flag on the last.

use super::{
    ConfigMismatch, ControllerConfig, ControllerNetworkConfiguration, ModuleShared, ModuleStatus, NetworkProtocol, OutputModule,
    TransmissionMethod, UniverseConfiguration,
};
use crate::command::OutputCommand;
use crate::ids;
use crate::xmlutil::{self, data_root, opt_text_el, ModuleIds};
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use uuid::Uuid;
use vixen_xml::{contract_ns, XElement};

const CLR_NS: &str = "VixenModules.Output.DDP";

pub const DDP_PORT: u16 = 4048;
pub const DDP_HEADER_LEN: usize = 10;
pub const DDP_CHANNELS_PER_PACKET: usize = 1440;
const DDP_FLAGS1_VER1: u8 = 0x40;
const DDP_FLAGS1_PUSH: u8 = 0x01;
const DDP_ID_DISPLAY: u8 = 1;

/// `DDPData`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DdpConfig {
    pub ids: ModuleIds,
    /// Always an IP literal. Set to the loopback address in a new data object.
    pub address: Option<String>,
    /// The host name the user entered, when they entered one.
    pub host_name: Option<String>,
}

impl Default for DdpConfig {
    fn default() -> Self {
        DdpConfig { ids: ModuleIds::new(ids::DDP, Uuid::nil()), address: Some(Ipv4Addr::LOCALHOST.to_string()), host_name: None }
    }
}

impl DdpConfig {
    pub fn from_xml(e: &XElement) -> DdpConfig {
        DdpConfig {
            ids: ModuleIds::read(e),
            address: xmlutil::get_opt_string(e, "Address"),
            host_name: xmlutil::get_opt_string(e, "HostName"),
        }
    }

    pub fn to_xml(&self) -> XElement {
        let ns = contract_ns(CLR_NS);
        let mut e = data_root(&ns, "DDPData", &self.ids);
        e.push(opt_text_el(&ns, "Address", self.address.as_deref()));
        e.push(opt_text_el(&ns, "HostName", self.host_name.as_deref()));
        e
    }
}

/// `SetupPackets`: the size of each packet. A count that's an exact multiple of 1440 still ends in a packet with no data,
/// which is the one that carries the push flag.
pub fn packet_sizes(output_count: usize) -> Vec<usize> {
    let num = output_count / DDP_CHANNELS_PER_PACKET;
    let last = output_count % DDP_CHANNELS_PER_PACKET;
    let mut sizes = vec![DDP_CHANNELS_PER_PACKET; num];
    sizes.push(last);
    sizes
}

/// `SendPacket`'s header for one packet.
pub fn write_header(packet: &mut [u8], packet_number: usize, size: usize, last: bool) {
    let offset = (packet_number * DDP_CHANNELS_PER_PACKET) as u32;
    packet[0] = if last { DDP_FLAGS1_VER1 | DDP_FLAGS1_PUSH } else { DDP_FLAGS1_VER1 };
    packet[1] = 0; // Reserved.
    packet[2] = 0; // Undefined data type (VIX-4029).
    packet[3] = DDP_ID_DISPLAY;
    packet[4..8].copy_from_slice(&offset.to_be_bytes());
    packet[8] = (size >> 8) as u8;
    packet[9] = size as u8;
}

/// `HostNameResolver.TryResolveToIPv4`: the first IPv4 address. The setup dialog uses this too.
pub fn resolve_to_ipv4(host_name: &str) -> Option<Ipv4Addr> {
    if host_name.trim().is_empty() {
        return None;
    }
    (host_name, 0u16).to_socket_addrs().ok()?.find_map(|a| match a.ip() {
        IpAddr::V4(v4) => Some(v4),
        IpAddr::V6(_) => None,
    })
}

/// `DDP`.
pub struct DdpModule {
    config: DdpConfig,
    shared: Arc<ModuleShared>,
    output_count: usize,
    sizes: Vec<usize>,
    packet: Box<[u8; DDP_HEADER_LEN + DDP_CHANNELS_PER_PACKET]>,
    is_running: bool,
    socket: Option<UdpSocket>,
    port: u16,
}

impl DdpModule {
    pub fn new(config: DdpConfig) -> DdpModule {
        DdpModule::with_port(config, DDP_PORT)
    }

    /// A module sending to another port, for a controller on an unusual one and for tests.
    pub fn with_port(config: DdpConfig, port: u16) -> DdpModule {
        DdpModule {
            config,
            shared: ModuleShared::new(),
            output_count: 0,
            sizes: Vec::new(),
            packet: Box::new([0; DDP_HEADER_LEN + DDP_CHANNELS_PER_PACKET]),
            is_running: false,
            socket: None,
            port,
        }
    }

    fn setup_packets(&mut self) {
        if self.output_count == 0 {
            return;
        }
        self.sizes = packet_sizes(self.output_count);
    }

    /// `OpenConnection`.
    fn open_connection(&mut self) -> bool {
        self.socket = None;
        if self.config.address.is_none() {
            self.shared.record_error("DDP: Trying to connect with a null IP address.");
            return false;
        }
        if let Some(host) = self.config.host_name.as_deref().filter(|h| !h.is_empty()) {
            match resolve_to_ipv4(host) {
                Some(resolved) => {
                    let text = resolved.to_string();
                    if self.config.address.as_deref() != Some(text.as_str()) {
                        self.config.address = Some(text);
                    }
                }
                None => self
                    .shared
                    .record_error(format!("DDP: Could not resolve host name '{host}'; continuing with last known address.")),
            }
        }
        let address = self.config.address.clone().unwrap_or_default();
        let connected =
            UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).and_then(|s| s.connect((address.as_str(), self.port)).map(|()| s));
        match connected {
            Ok(s) => {
                self.socket = Some(s);
                true
            }
            Err(e) => {
                self.shared.record_error(format!("DDP: Failed connect to host {address}: {e}"));
                false
            }
        }
    }

    /// `GetNetworkConfiguration`, for the FPP export.
    pub fn network_configuration(&self) -> ControllerNetworkConfiguration {
        let ip_address = self.config.address.as_deref().and_then(|a| a.parse().ok()).unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        ControllerNetworkConfiguration {
            protocol: NetworkProtocol::Ddp,
            transmission: TransmissionMethod::Unicast,
            ip_address: Some(ip_address),
            supports_universes: true,
            throttling_enabled: false,
            universes: vec![UniverseConfiguration { universe_number: 1, active: true, start: 1, size: self.output_count as i32 }],
        }
    }
}

impl OutputModule for DdpModule {
    fn type_id(&self) -> Uuid {
        ids::DDP
    }

    fn start(&mut self) {
        self.setup_packets();
        let connected = self.open_connection();
        self.is_running = true;
        self.shared.set_status(if connected {
            ModuleStatus::Running
        } else {
            ModuleStatus::Unavailable(self.shared.last_error().unwrap_or_else(|| "DDP: not connected".to_string()))
        });
    }

    fn stop(&mut self) {
        self.is_running = false;
        self.socket = None;
        self.shared.set_status(ModuleStatus::Stopped);
    }

    fn pause(&mut self) {
        self.is_running = false;
        self.socket = None;
    }

    fn resume(&mut self) {
        self.open_connection();
        self.is_running = true;
    }

    fn update(&mut self, commands: &[OutputCommand]) {
        if !self.is_running || self.output_count == 0 {
            return;
        }
        if commands.len() != self.output_count {
            self.setup_packets();
        }
        let mut channel = 0;
        let last = self.sizes.len().saturating_sub(1);
        for (number, &size) in self.sizes.iter().enumerate() {
            for slot in &mut self.packet[DDP_HEADER_LEN..DDP_HEADER_LEN + size] {
                *slot = commands.get(channel).map_or(0, OutputCommand::byte);
                channel += 1;
            }
            write_header(&mut self.packet[..], number, size, number == last);
            let bytes = &self.packet[..size + DDP_HEADER_LEN];
            let sent = match &self.socket {
                Some(s) => s.send(bytes).map(|_| ()),
                None => Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "DDP: not connected")),
            };
            match sent {
                Ok(()) => self.shared.count_packet(bytes.len()),
                // `OutputController.Update` catches this, so the rest of the frame is lost.
                Err(e) => {
                    self.shared.record_error(format!("An error occurred outputting data: {e}"));
                    return;
                }
            }
        }
    }

    fn output_count(&self) -> usize {
        self.output_count
    }

    fn set_output_count(&mut self, count: usize) {
        self.output_count = count;
    }

    fn config(&self) -> ControllerConfig {
        ControllerConfig::Ddp(self.config.clone())
    }

    fn set_config(&mut self, config: ControllerConfig) -> Result<(), ConfigMismatch> {
        let ControllerConfig::Ddp(config) = config else {
            return Err(ConfigMismatch);
        };
        self.config = config;
        self.socket = None;
        Ok(())
    }

    fn shared(&self) -> &Arc<ModuleShared> {
        &self.shared
    }
}

/// The expected clone values come from upstream `DDPDataTests`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_copies_host_name_alongside_address() {
        let original = DdpConfig {
            address: Some("192.168.1.50".to_string()),
            host_name: Some("wled-porch".to_string()),
            ..Default::default()
        };
        let clone = original.clone();
        assert_eq!(original.address, clone.address);
        assert_eq!(original.host_name, clone.host_name);
    }

    #[test]
    fn clone_host_name_null_clone_preserves_null() {
        let original = DdpConfig { address: Some("192.168.1.50".to_string()), host_name: None, ..Default::default() };
        assert_eq!(original.clone().host_name, None);
    }

    #[test]
    fn a_new_config_points_at_loopback() {
        assert_eq!(DdpConfig::default().address.as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn packets_split_at_1440_with_the_remainder_last() {
        assert_eq!(packet_sizes(3), vec![3]);
        assert_eq!(packet_sizes(1439), vec![1439]);
        assert_eq!(packet_sizes(1441), vec![1440, 1]);
        assert_eq!(packet_sizes(3000), vec![1440, 1440, 120]);
    }

    #[test]
    fn an_exact_multiple_ends_in_an_empty_packet() {
        assert_eq!(packet_sizes(1440), vec![1440, 0]);
        assert_eq!(packet_sizes(2880), vec![1440, 1440, 0]);
    }

    #[test]
    fn header_bytes() {
        let mut p = [0u8; 10];
        write_header(&mut p, 0, 3, true);
        assert_eq!(p, [0x41, 0, 0, 1, 0, 0, 0, 0, 0, 3]);
        write_header(&mut p, 2, 1440, false);
        assert_eq!(p, [0x40, 0, 0, 1, 0, 0, 0x0B, 0x40, 0x05, 0xA0]);
    }

    #[test]
    fn network_configuration_reports_one_universe() {
        let mut m = DdpModule::new(DdpConfig { address: Some("10.0.0.5".to_string()), ..Default::default() });
        m.set_output_count(300);
        let n = m.network_configuration();
        assert_eq!(n.ip_address, Some("10.0.0.5".parse().unwrap()));
        assert_eq!(n.universes[0].size, 300);
    }

    #[test]
    fn a_null_address_stays_quiet() {
        let mut m = DdpModule::new(DdpConfig { address: None, ..Default::default() });
        m.set_output_count(2);
        m.start();
        assert!(matches!(m.status(), ModuleStatus::Unavailable(_)));
        m.update(&[OutputCommand::U8(1), OutputCommand::U8(2)]);
        assert_eq!(m.shared().packets_sent(), 0);
        assert!(m.shared().error_count() > 0);
        m.stop();
    }
}
