//! Every wire format, byte for byte. Network tests only use 127.0.0.1 and serial tests a pseudo terminal, so no real hardware
//! or network is touched, and nothing here sends multicast.

mod common;

use common::{Pty, UdpReceiver};
use std::sync::Arc;
use std::time::Duration;
use uuid::uuid;
use vixen_output::modules::ddp::{DdpConfig, DdpModule};
use vixen_output::modules::dmx_usb_pro::{DmxUsbProConfig, DmxUsbProModule};
use vixen_output::modules::e131::{multicast_address, packet, E131Config, E131Module, UniverseEntry};
use vixen_output::modules::elexol::{ElexolConfig, ElexolModule, INIT_PACKET, OFF_PACKET};
use vixen_output::modules::generic_serial::{GenericSerialConfig, GenericSerialModule};
use vixen_output::modules::rds::{RdsConfig, RdsHardware, RdsModule};
use vixen_output::modules::renard::{RenardConfig, RenardModule};
use vixen_output::serial::{Parity, SerialSettings, StopBits};
use vixen_output::{ids, ModuleIds, ModuleStatus, OutputCommand, OutputModule};

const WAIT: Duration = Duration::from_secs(2);

fn u8s(values: &[u8]) -> Vec<OutputCommand> {
    values.iter().map(|v| OutputCommand::U8(*v)).collect()
}

fn serial_settings(path: &str, baud: i32) -> SerialSettings {
    SerialSettings {
        port_name: Some(path.to_string()),
        baud_rate: baud,
        parity: Parity::None,
        data_bits: 8,
        stop_bits: StopBits::One,
    }
}

// ---------------------------------------------------------------- E1.31

const CID: uuid::Uuid = uuid!("00112233-4455-6677-8899-aabbccddeeff");

fn e131_config(universes: Vec<UniverseEntry>) -> E131Config {
    E131Config {
        ids: ModuleIds::new(ids::E131, CID),
        unicast: Some("127.0.0.1".to_string()),
        universes: Some(universes),
        ..Default::default()
    }
}

fn e131_module(config: E131Config, outputs: usize, rx: &UdpReceiver) -> E131Module {
    let mut m = E131Module::with_port(config, rx.port);
    m.set_output_count(outputs);
    m.start();
    assert_eq!(m.status(), ModuleStatus::Running);
    m
}

/// The packet the plugin builds, written out from `E131Root`, `E131Framing` and `E131DeviceManagementProtocol`.
fn expected_e131(universe: u16, sequence: u8, priority: u8, options: u8, slots: &[u8]) -> Vec<u8> {
    let n = slots.len();
    let dmp = 10 + 1 + n;
    let framing = 77 + dmp;
    let root = 22 + framing;
    let mut p = Vec::new();
    p.extend_from_slice(&[0x00, 0x10, 0x00, 0x00]);
    p.extend_from_slice(b"ASC-E1.17\0\0\0");
    p.extend_from_slice(&(0x7000u16 | root as u16).to_be_bytes());
    p.extend_from_slice(&[0, 0, 0, 4]);
    // The CID, in `Guid.ToByteArray()` order.
    p.extend_from_slice(&[0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    p.extend_from_slice(&(0x7000u16 | framing as u16).to_be_bytes());
    p.extend_from_slice(&[0, 0, 0, 2]);
    let mut source = [0u8; 64];
    source[..7].copy_from_slice(b"Vixen 3");
    p.extend_from_slice(&source);
    p.push(priority);
    p.extend_from_slice(&[0, 0]);
    p.push(sequence);
    p.push(options);
    p.extend_from_slice(&universe.to_be_bytes());
    p.extend_from_slice(&(0x7000u16 | dmp as u16).to_be_bytes());
    p.extend_from_slice(&[0x02, 0xa1, 0x00, 0x00, 0x00, 0x01]);
    p.extend_from_slice(&((n + 1) as u16).to_be_bytes());
    p.push(0);
    p.extend_from_slice(slots);
    p
}

#[test]
fn e131_packet_is_exact_with_fixed_bytes_written_out_by_hand() {
    let rx = UdpReceiver::new();
    let mut m = e131_module(e131_config(vec![UniverseEntry::new(true, 1, 0, 3)]), 3, &rx);
    m.update(&u8s(&[1, 2, 3]));
    let got = rx.recv();
    #[rustfmt::skip]
    let mut want: Vec<u8> = vec![
        // Root layer.
        0x00, 0x10, 0x00, 0x00,
        0x41, 0x53, 0x43, 0x2d, 0x45, 0x31, 0x2e, 0x31, 0x37, 0x00, 0x00, 0x00,
        0x70, 0x71,
        0x00, 0x00, 0x00, 0x04,
        0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
        // Framing layer.
        0x70, 0x5b,
        0x00, 0x00, 0x00, 0x02,
        0x56, 0x69, 0x78, 0x65, 0x6e, 0x20, 0x33,
    ];
    want.extend_from_slice(&[0u8; 57]);
    #[rustfmt::skip]
    want.extend_from_slice(&[
        100,
        0x00, 0x00,
        0x00,
        0x00,
        0x00, 0x01,
        // Device management protocol layer.
        0x70, 0x0e,
        0x02,
        0xa1,
        0x00, 0x00,
        0x00, 0x01,
        0x00, 0x04,
        0x00,
        1, 2, 3,
    ]);
    assert_eq!(got, want);
    assert_eq!(got.len(), 129);
    assert_eq!(got, expected_e131(1, 0, 100, 0, &[1, 2, 3]));
    m.stop();
}

#[test]
fn e131_sends_universes_from_their_start_with_their_own_sequence_numbers() {
    let rx = UdpReceiver::new();
    let mut m = e131_module(e131_config(vec![UniverseEntry::new(true, 5, 0, 2), UniverseEntry::new(true, 300, 2, 3)]), 5, &rx);
    m.update(&u8s(&[1, 2, 3, 4, 5]));
    assert_eq!(rx.recv(), expected_e131(5, 0, 100, 0, &[1, 2]));
    assert_eq!(rx.recv(), expected_e131(300, 0, 100, 0, &[3, 4, 5]));
    m.update(&u8s(&[9, 8, 7, 6, 5]));
    assert_eq!(rx.recv(), expected_e131(5, 1, 100, 0, &[9, 8]));
    assert_eq!(rx.recv(), expected_e131(300, 1, 100, 0, &[7, 6, 5]));
    m.stop();
}

#[test]
fn e131_sequence_starts_from_the_saved_value_and_wraps() {
    let rx = UdpReceiver::new();
    let mut u = UniverseEntry::new(true, 1, 0, 1);
    u.seq_num = 254;
    let mut m = e131_module(e131_config(vec![u]), 1, &rx);
    for expected in [254u8, 255, 0, 1] {
        m.update(&u8s(&[7]));
        assert_eq!(rx.recv(), expected_e131(1, expected, 100, 0, &[7]));
    }
    let vixen_output::ControllerConfig::E131(saved) = m.config() else { panic!() };
    assert_eq!(saved.universes.unwrap()[0].seq_num, 2);
    m.stop();
}

#[test]
fn e131_priority_and_blind_are_in_the_framing_layer() {
    let rx = UdpReceiver::new();
    let mut c = e131_config(vec![UniverseEntry::new(true, 1, 0, 1)]);
    c.priority = 200;
    c.blind = true;
    let mut m = e131_module(c, 1, &rx);
    m.update(&u8s(&[7]));
    assert_eq!(rx.recv(), expected_e131(1, 0, 200, 128, &[7]));
    m.stop();
}

#[test]
fn e131_null_and_16_bit_commands_are_zero_slots() {
    let rx = UdpReceiver::new();
    let mut m = e131_module(e131_config(vec![UniverseEntry::new(true, 1, 0, 3)]), 3, &rx);
    m.update(&[OutputCommand::None, OutputCommand::U16(65535), OutputCommand::U8(5)]);
    assert_eq!(rx.recv(), expected_e131(1, 0, 100, 0, &[0, 0, 5]));
    m.stop();
}

#[test]
fn e131_universe_past_the_outputs_keeps_its_size_and_zero_slots() {
    // `universeSize` is cut to what the controller has, but the packet was built at full size.
    let rx = UdpReceiver::new();
    let mut m = e131_module(e131_config(vec![UniverseEntry::new(true, 1, 2, 5)]), 4, &rx);
    m.update(&u8s(&[1, 2, 3, 4]));
    assert_eq!(rx.recv(), expected_e131(1, 0, 100, 0, &[3, 4, 0, 0, 0]));
    let stats = m.counters().snapshot();
    // `SlotCount` adds the full size anyway.
    assert_eq!(stats.universes[0].slots, 5);
    m.stop();
}

#[test]
fn e131_skips_an_inactive_universe_and_one_starting_past_the_outputs() {
    let rx = UdpReceiver::new();
    let mut m = e131_module(
        e131_config(vec![
            UniverseEntry::new(false, 1, 0, 2),
            UniverseEntry::new(true, 2, 10, 2),
            UniverseEntry::new(true, 3, 0, 2),
        ]),
        2,
        &rx,
    );
    m.update(&u8s(&[1, 2]));
    assert_eq!(rx.recv(), expected_e131(3, 0, 100, 0, &[1, 2]));
    assert_eq!(rx.poll(Duration::from_millis(30)), None);
    m.stop();
}

#[test]
fn e131_sends_every_frame_by_default_even_when_nothing_changes() {
    let rx = UdpReceiver::new();
    let mut m = e131_module(e131_config(vec![UniverseEntry::new(true, 1, 0, 1)]), 1, &rx);
    for seq in 0..4 {
        m.update(&u8s(&[9]));
        assert_eq!(rx.recv(), expected_e131(1, seq, 100, 0, &[9]));
    }
    m.stop();
}

#[test]
fn e131_repeat_and_suppress_counts_follow_the_plugin() {
    let rx = UdpReceiver::new();
    let mut c = e131_config(vec![UniverseEntry::new(true, 1, 0, 1)]);
    c.event_repeat_count = 2;
    c.event_suppress_count = 3;
    let mut m = e131_module(c, 1, &rx);
    let frame = u8s(&[9]);

    let mut sent = Vec::new();
    let mut step = |m: &mut E131Module, frame: &[OutputCommand]| {
        m.update(frame);
        sent.push(rx.poll(Duration::from_millis(20)).map(|p| (p[packet::SEQUENCE_OFFSET], p[packet::SLOTS_OFFSET])));
    };
    // New data goes out, then one repeat. Past that, only every third repeat is forced out.
    step(&mut m, &frame);
    step(&mut m, &frame);
    step(&mut m, &frame);
    step(&mut m, &frame);
    step(&mut m, &frame);
    step(&mut m, &frame);
    step(&mut m, &frame);
    step(&mut m, &frame);
    // A change goes out at once and resets everything.
    let changed = u8s(&[10]);
    step(&mut m, &changed);
    step(&mut m, &changed);
    step(&mut m, &changed);
    assert_eq!(
        sent,
        vec![Some((0, 9)), Some((1, 9)), None, None, Some((2, 9)), None, None, Some((3, 9)), Some((4, 10)), Some((5, 10)), None]
    );
    m.stop();
}

#[test]
fn e131_repeats_of_the_empty_packet_count_as_the_same() {
    // The buffer starts all zero, so a first frame of zeros is already a repeat.
    let rx = UdpReceiver::new();
    let mut c = e131_config(vec![UniverseEntry::new(true, 1, 0, 1)]);
    c.event_repeat_count = 1;
    let mut m = e131_module(c, 1, &rx);
    m.update(&u8s(&[0]));
    assert_eq!(rx.poll(Duration::from_millis(30)), None);
    m.update(&u8s(&[1]));
    assert_eq!(rx.recv(), expected_e131(1, 0, 100, 0, &[1]));
    m.stop();
}

#[test]
fn e131_counters_match_what_the_statistics_box_shows() {
    let rx = UdpReceiver::new();
    let mut m = e131_module(e131_config(vec![UniverseEntry::new(true, 7, 0, 4), UniverseEntry::new(false, 8, 0, 4)]), 4, &rx);
    let counters = m.counters();
    for _ in 0..3 {
        m.update(&u8s(&[1, 2, 3, 4]));
        rx.recv();
    }
    let stats = counters.snapshot();
    assert_eq!(stats.events, 3);
    assert_eq!(stats.universes[0].universe, 7);
    assert_eq!(stats.universes[0].packets, 3);
    assert_eq!(stats.universes[0].slots, 12);
    assert_eq!(stats.universes[1].packets, 0);
    assert!(stats.update_nanos > 0);
    assert!(stats.to_text().starts_with("Events: 3\nTotal Time: "));
    assert!(stats.to_text().contains("Univ=7  Packets=3  Slots=12"));
    assert!(!stats.to_text().contains("Univ=8"));
    assert_eq!(m.shared().packets_sent(), 3);
    m.stop();
    // A restart clears the plugin-wide stats, but a universe's counts live on its entry.
    m.start();
    let stats = counters.snapshot();
    assert_eq!(stats.events, 0);
    assert_eq!(stats.universes[0].packets, 3);
    m.stop();
}

#[test]
fn e131_stops_sending_after_stop_and_rebuilds_its_buffer_on_restart() {
    let rx = UdpReceiver::new();
    let mut m = e131_module(e131_config(vec![UniverseEntry::new(true, 1, 0, 2)]), 2, &rx);
    m.update(&u8s(&[1, 2]));
    rx.recv();
    m.stop();
    assert_eq!(m.status(), ModuleStatus::Stopped);
    m.update(&u8s(&[3, 4]));
    assert_eq!(rx.poll(Duration::from_millis(30)), None);
    m.start();
    // The sequence number is on the entry, so it carries on.
    m.update(&u8s(&[5, 6]));
    assert_eq!(rx.recv(), expected_e131(1, 1, 100, 0, &[5, 6]));
    m.stop();
}

#[test]
fn e131_a_failed_later_universe_leaves_earlier_ones_running() {
    // `running` is set inside the loop. Nothing after the failing universe gets a packet.
    let rx = UdpReceiver::new();
    let big = UniverseEntry::new(true, 2, 0, 5000);
    let mut m = E131Module::with_port(
        e131_config(vec![UniverseEntry::new(true, 1, 0, 1), big, UniverseEntry::new(true, 3, 0, 1)]),
        rx.port,
    );
    m.set_output_count(1);
    m.start();
    assert!(matches!(m.status(), ModuleStatus::Unavailable(_)));
    m.update(&u8s(&[9]));
    assert_eq!(rx.recv(), expected_e131(1, 0, 100, 0, &[9]));
    assert_eq!(rx.poll(Duration::from_millis(30)), None);
    m.stop();
}

#[test]
fn e131_unicast_takes_a_host_name() {
    let rx = UdpReceiver::new();
    let mut c = e131_config(vec![UniverseEntry::new(true, 1, 0, 1)]);
    c.unicast = Some("localhost".to_string());
    let mut m = e131_module(c, 1, &rx);
    m.update(&u8s(&[3]));
    assert_eq!(rx.recv(), expected_e131(1, 0, 100, 0, &[3]));
    m.stop();
}

#[test]
fn e131_multicast_sets_up_on_a_real_interface_without_sending() {
    // Only the socket options are set. `update` is never called, so nothing leaves the machine.
    let Some(nic) = vixen_output::net::list_multicast_interfaces().into_iter().find(|n| n.is_up && !n.ipv4_addresses.is_empty())
    else {
        eprintln!("skipping: no interface to set up on");
        return;
    };
    let mut c = e131_config(vec![UniverseEntry::new(true, 1, 0, 1), UniverseEntry::new(true, 2, 1, 1)]);
    c.unicast = None;
    c.multicast = Some(nic.id.clone());
    let mut m = E131Module::with_port(c, 1);
    m.set_output_count(2);
    m.start();
    assert_eq!(m.status(), ModuleStatus::Running, "{:?}", m.shared().last_error());
    let n = m.network_configuration();
    assert_eq!(n.transmission, vixen_output::modules::TransmissionMethod::Multicast);
    assert_eq!(n.ip_address, None);
    m.stop();
    assert_eq!(m.shared().packets_sent(), 0);
}

#[test]
fn e131_multicast_groups_are_computed_but_never_used_here() {
    assert_eq!(multicast_address(1).octets(), [239, 255, 0, 1]);
    assert_eq!(multicast_address(0x0102).octets(), [239, 255, 1, 2]);
}

// ---------------------------------------------------------------- DDP

fn ddp_module(outputs: usize, rx: &UdpReceiver) -> DdpModule {
    let mut m = DdpModule::with_port(DdpConfig { address: Some("127.0.0.1".to_string()), ..Default::default() }, rx.port);
    m.set_output_count(outputs);
    m.start();
    assert_eq!(m.status(), ModuleStatus::Running);
    m
}

#[test]
fn ddp_single_packet_has_the_push_flag() {
    let rx = UdpReceiver::new();
    let mut m = ddp_module(3, &rx);
    m.update(&[OutputCommand::U8(10), OutputCommand::None, OutputCommand::U16(700)]);
    assert_eq!(rx.recv(), vec![0x41, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0x00, 0x03, 10, 0, 0]);
    m.stop();
}

#[test]
fn ddp_splits_at_1440_channels_with_the_push_flag_last() {
    let rx = UdpReceiver::new();
    let mut m = ddp_module(1441, &rx);
    let frame: Vec<OutputCommand> = (0..1441).map(|i| OutputCommand::U8((i % 251) as u8)).collect();
    m.update(&frame);
    let first = rx.recv();
    assert_eq!(first.len(), 1450);
    assert_eq!(&first[..10], &[0x40, 0, 0, 1, 0, 0, 0, 0, 0x05, 0xA0]);
    assert_eq!(first[10], 0);
    assert_eq!(first[1449], (1439 % 251) as u8);
    let second = rx.recv();
    assert_eq!(second, vec![0x41, 0, 0, 1, 0x00, 0x00, 0x05, 0xA0, 0x00, 0x01, (1440 % 251) as u8]);
    m.stop();
}

#[test]
fn ddp_an_exact_multiple_ends_in_an_empty_push_packet() {
    let rx = UdpReceiver::new();
    let mut m = ddp_module(1440, &rx);
    m.update(&vec![OutputCommand::U8(1); 1440]);
    assert_eq!(rx.recv().len(), 1450);
    assert_eq!(rx.recv(), vec![0x41, 0, 0, 1, 0x00, 0x00, 0x05, 0xA0, 0x00, 0x00]);
    m.stop();
}

#[test]
fn ddp_offsets_keep_growing_by_1440() {
    let rx = UdpReceiver::new();
    let mut m = ddp_module(3000, &rx);
    m.update(&vec![OutputCommand::U8(2); 3000]);
    let first = rx.recv();
    let second = rx.recv();
    let third = rx.recv();
    assert_eq!(&first[..10], &[0x40, 0, 0, 1, 0, 0, 0, 0, 0x05, 0xA0]);
    assert_eq!(&second[..10], &[0x40, 0, 0, 1, 0, 0, 0x05, 0xA0, 0x05, 0xA0]);
    assert_eq!(&third[..10], &[0x41, 0, 0, 1, 0, 0, 0x0B, 0x40, 0x00, 0x78]);
    assert_eq!(third.len(), 130);
    m.stop();
}

#[test]
fn ddp_host_name_replaces_the_saved_address() {
    let rx = UdpReceiver::new();
    let mut m = DdpModule::with_port(
        DdpConfig { address: Some("10.255.255.1".to_string()), host_name: Some("localhost".to_string()), ..Default::default() },
        rx.port,
    );
    m.set_output_count(1);
    m.start();
    m.update(&u8s(&[4]));
    assert_eq!(rx.recv(), vec![0x41, 0, 0, 1, 0, 0, 0, 0, 0, 1, 4]);
    let vixen_output::ControllerConfig::Ddp(saved) = m.config() else { panic!() };
    assert_eq!(saved.address.as_deref(), Some("127.0.0.1"));
    assert_eq!(saved.host_name.as_deref(), Some("localhost"));
    m.stop();
}

#[test]
fn ddp_is_quiet_when_stopped_paused_or_empty() {
    let rx = UdpReceiver::new();
    let mut m = ddp_module(1, &rx);
    m.pause();
    m.update(&u8s(&[1]));
    assert_eq!(rx.poll(Duration::from_millis(30)), None);
    m.resume();
    m.update(&u8s(&[2]));
    assert_eq!(rx.recv()[10], 2);
    m.stop();
    m.update(&u8s(&[3]));
    assert_eq!(rx.poll(Duration::from_millis(30)), None);

    let mut none = ddp_module(0, &rx);
    none.update(&[]);
    assert_eq!(rx.poll(Duration::from_millis(30)), None);
    none.stop();
}

// ---------------------------------------------------------------- Elexol

#[test]
fn elexol_initialises_once_then_sends_the_state_every_frame() {
    let rx = UdpReceiver::new();
    let mut m = ElexolModule::new(ElexolConfig {
        address: Some("127.0.0.1".to_string()),
        port: rx.port as i32,
        minimum_intensity: 1,
        ..Default::default()
    });
    m.set_output_count(3);
    m.start();
    assert_eq!(m.status(), ModuleStatus::Running);
    assert_eq!(rx.recv(), INIT_PACKET.to_vec());
    assert_eq!(INIT_PACKET, [b'!', b'A', 0, b'A', 0, b'!', b'B', 0, b'B', 0, b'!', b'C', 0, b'C', 0]);
    m.update(&u8s(&[255, 1, 2]));
    assert_eq!(rx.recv(), vec![b'A', 0b101, b'B', 0, b'C', 0]);
    m.update(&u8s(&[0, 0, 0]));
    assert_eq!(rx.recv(), vec![b'A', 0, b'B', 0, b'C', 0]);
    m.stop();
    assert_eq!(rx.recv(), OFF_PACKET.to_vec());
}

// ---------------------------------------------------------------- Serial, through a pseudo terminal

#[test]
fn generic_serial_frames_are_header_values_footer() {
    let pty = Pty::new();
    let mut m = GenericSerialModule::new(GenericSerialConfig {
        serial: serial_settings(&pty.path, 9600),
        header: Some("AB".to_string()),
        footer: Some("Z\u{e9}".to_string()),
        ..Default::default()
    });
    m.set_output_count(3);
    m.start();
    assert_eq!(m.status(), ModuleStatus::Running, "{:?}", m.shared().last_error());
    m.update(&[OutputCommand::U8(1), OutputCommand::None, OutputCommand::U16(9)]);
    // Non-ASCII in the footer becomes '?', and the value bytes are raw.
    assert_eq!(pty.read_exactly(7, WAIT), vec![b'A', b'B', 1, 0, 0, b'Z', b'?']);
    m.update(&u8s(&[255, 0x0D, 0x0A]));
    // Line endings aren't translated on a port in raw mode.
    assert_eq!(pty.read_exactly(7, WAIT), vec![b'A', b'B', 255, 0x0D, 0x0A, b'Z', b'?']);
    assert_eq!(m.shared().packets_sent(), 2);
    assert_eq!(m.shared().bytes_sent(), 14);
    m.stop();
}

#[test]
fn generic_serial_with_no_header_is_just_the_values() {
    let pty = Pty::new();
    let mut m = GenericSerialModule::new(GenericSerialConfig {
        serial: serial_settings(&pty.path, 9600),
        header: Some(String::new()),
        ..Default::default()
    });
    m.set_output_count(4);
    m.start();
    m.update(&u8s(&[9, 8, 7, 6]));
    assert_eq!(pty.read_exactly(4, WAIT), vec![9, 8, 7, 6]);
    m.stop();
}

#[test]
fn generic_serial_with_nothing_to_send_writes_nothing() {
    let pty = Pty::new();
    let mut m = GenericSerialModule::new(GenericSerialConfig { serial: serial_settings(&pty.path, 9600), ..Default::default() });
    m.start();
    m.update(&[]);
    assert_eq!(pty.drain(Duration::from_millis(20)), Vec::<u8>::new());
    assert_eq!(m.shared().packets_sent(), 0);
    m.stop();
}

#[test]
fn renard_frames_escape_and_pad() {
    let pty = Pty::new();
    let mut m = RenardModule::new(RenardConfig { serial: serial_settings(&pty.path, 57600), ..Default::default() });
    m.set_output_count(6);
    m.start();
    assert_eq!(m.status(), ModuleStatus::Running, "{:?}", m.shared().last_error());
    m.update(&u8s(&[0x7C, 0x7D, 0x7E, 0x7F, 0x80, 0xFF]));
    assert_eq!(pty.read_exactly(8, WAIT), vec![0x7E, 0x80, 124, 124, 124, 128, 0x80, 0xFF]);
    m.update(&[
        OutputCommand::None,
        OutputCommand::U16(1),
        OutputCommand::U8(1),
        OutputCommand::U8(2),
        OutputCommand::U8(3),
        OutputCommand::U8(4),
    ]);
    assert_eq!(pty.read_exactly(8, WAIT), vec![0x7E, 0x80, 0, 0, 1, 2, 3, 4]);
    m.stop();
}

#[test]
fn renard_inserts_a_pad_byte_every_100_bytes() {
    let pty = Pty::new();
    let mut m = RenardModule::new(RenardConfig { serial: serial_settings(&pty.path, 57600), ..Default::default() });
    m.set_output_count(250);
    m.start();
    let frame = vec![OutputCommand::U8(0x11); 250];
    m.update(&frame);
    // 2 + 250 bytes with pads after index 99 and index 200.
    let got = pty.read_exactly(254, WAIT);
    assert_eq!(got.len(), 254);
    assert_eq!(&got[..2], &[0x7E, 0x80]);
    assert_eq!(got[100], 0x7D);
    assert_eq!(got[200], 0x7D);
    assert_eq!(got.iter().filter(|b| **b == 0x7D).count(), 2);
    assert_eq!(got.iter().filter(|b| **b == 0x11).count(), 250);
    m.stop();
}

#[test]
fn renard_at_the_output_count_that_overflows_loses_every_frame() {
    let pty = Pty::new();
    let mut m = RenardModule::new(RenardConfig { serial: serial_settings(&pty.path, 57600), ..Default::default() });
    m.set_output_count(197);
    m.start();
    m.update(&vec![OutputCommand::U8(1); 197]);
    assert_eq!(pty.drain(Duration::from_millis(20)), Vec::<u8>::new());
    assert_eq!(m.shared().error_count(), 1);
    m.stop();
}

#[test]
fn dmx_usb_pro_frames_are_exact() {
    let pty = Pty::new();
    let mut m = DmxUsbProModule::new(DmxUsbProConfig { serial: serial_settings(&pty.path, 115200), ..Default::default() });
    m.set_output_count(4);
    m.start();
    assert_eq!(m.status(), ModuleStatus::Running, "{:?}", m.shared().last_error());
    m.update(&[OutputCommand::U8(1), OutputCommand::None, OutputCommand::U16(3), OutputCommand::U8(255)]);
    let got = pty.read_exactly(518, WAIT);
    assert_eq!(got.len(), 518);
    let mut want = vec![0x7E, 6, 0x01, 0x02, 0, 1, 0, 0, 255];
    want.resize(517, 0);
    want.push(0xE7);
    assert_eq!(got, want);
    m.stop();
}

#[test]
fn dmx_usb_pro_only_sends_the_first_512_channels() {
    let pty = Pty::new();
    let mut m = DmxUsbProModule::new(DmxUsbProConfig { serial: serial_settings(&pty.path, 115200), ..Default::default() });
    m.set_output_count(600);
    m.start();
    m.update(&vec![OutputCommand::U8(7); 600]);
    let got = pty.read_exactly(518, WAIT);
    assert_eq!(got.len(), 518);
    assert_eq!(got[4], 0);
    assert!(got[5..517].iter().all(|b| *b == 7));
    assert_eq!(got[517], 0xE7);
    assert_eq!(pty.drain(Duration::from_millis(20)), Vec::<u8>::new());
    m.stop();
}

/// `lastRdsText` is static in the C#, and so one per process here, so the two RDS tests take turns.
static RDS_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn rds_serial_sends_the_setup_then_the_padded_text() {
    let _turn = RDS_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut pty = Pty::new();
    pty.hold_slave_open();
    let mut m =
        RdsModule::new(RdsConfig { hardware_id: RdsHardware::Mrds1322, port_name: Some(pty.path.clone()), ..Default::default() });
    m.start();
    let text: Arc<str> = Arc::from("RDS|Silent Night (wire test)");
    let mut want = vec![0xFE, 0x1F, 0xFF, 0xFE, 0x20];
    want.extend_from_slice(b"Silent Night (wire test)");
    want.resize(5 + 64, 0x20);
    want.push(0xFF);
    // Each write waits to drain, so this needs something reading the other end at the same time.
    let got = std::thread::scope(|scope| {
        let reader = scope.spawn(|| pty.read_exactly(want.len(), WAIT));
        m.update(&[OutputCommand::Str(text.clone())]);
        reader.join().unwrap()
    });
    assert_eq!(got, want, "{:?}", m.shared().last_error());
    // The same text isn't sent again, even from another frame.
    m.update(&[OutputCommand::Str(text)]);
    assert_eq!(pty.drain(Duration::from_millis(20)), Vec::<u8>::new());
    m.stop();
}

#[test]
fn rds_http_sends_the_encoded_text_and_answers_a_challenge() {
    use std::io::{Read, Write};
    let _turn = RDS_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for reply in [
            "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"rds\"\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n",
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).unwrap();
            requests.push(String::from_utf8_lossy(&buf[..n]).into_owned());
            stream.write_all(reply.as_bytes()).unwrap();
        }
        requests
    });
    let mut m = RdsModule::new(RdsConfig {
        hardware_id: RdsHardware::Http,
        http_url: Some(format!("http://127.0.0.1:{port}/?action=update_rt&update_rt={{text}}")),
        require_http_authentication: true,
        http_username: Some("user".to_string()),
        http_password: Some("pass".to_string()),
        ..Default::default()
    });
    m.start();
    m.update(&[OutputCommand::Str(Arc::from("RDS|Jingle Bells & more"))]);
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /?action=update_rt&update_rt=Jingle+Bells+%26+more HTTP/1.1\r\n"), "{}", requests[0]);
    assert!(!requests[0].contains("Authorization"));
    assert!(requests[1].contains("Authorization: Basic dXNlcjpwYXNz\r\n"), "{}", requests[1]);
    m.stop();
}

// Upstream `DDPPacketTests` (VIX-4029): header, payload, offsets and push flag against the 3waylabs spec.

#[test]
fn ddp_update_state_single_packet_encodes_spec_header_and_payload() {
    let rx = UdpReceiver::new();
    let mut m = ddp_module(3, &rx);
    m.update(&u8s(&[0x12, 0x34, 0x56]));
    assert_eq!(rx.recv(), vec![0x41, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x12, 0x34, 0x56]);
    m.stop();
}

#[test]
fn ddp_update_state_multiple_packets_offsets_and_push_flag_match_spec() {
    let rx = UdpReceiver::new();
    let mut m = ddp_module(1441, &rx);
    let states: Vec<OutputCommand> = (0..1441).map(|i| OutputCommand::U8((i & 0xff) as u8)).collect();
    m.update(&states);
    let first = rx.recv();
    let second = rx.recv();
    assert_eq!(first.len(), 1450);
    assert_eq!(first[0], 0x40);
    assert_eq!(&first[4..10], &[0x00, 0x00, 0x00, 0x00, 0x05, 0xa0]);
    assert_eq!(second[0], 0x41);
    assert_eq!(&second[4..10], &[0x00, 0x00, 0x05, 0xa0, 0x00, 0x01]);
    assert_eq!(first[first.len() - 1], 0x9f);
    assert_eq!(second[10], 0xa0);
    m.stop();
}
