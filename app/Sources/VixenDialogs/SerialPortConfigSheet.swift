import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Serial Port Config (F7 to F25), used by Generic Serial, Renard and DMX Pro. `onFinish` gets the chosen settings, or nil for Cancel.
/// Nothing's written here: the caller does that.
public struct SerialPortConfigSheet: View {
    let context: DialogContext
    let current: DialogSerialSettings?
    let onFinish: (DialogSerialSettings?) -> Void
    @State private var config: DialogSerialConfig
    @State private var alert: DialogAlert?

    private static let unsupportedHelp = "macOS serial ports don't support this setting"

    public init(context: DialogContext, current: DialogSerialSettings?, onFinish: @escaping (DialogSerialSettings?) -> Void) {
        self.context = context
        self.current = current
        self.onFinish = onFinish
        _config = State(initialValue: context.serialConfig(current: current, ports: context.serialPorts()))
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Serial Port Config")
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text("Current Configured Port:")
                    Text(config.currentLabel).fontWeight(.medium)
                }
                if let note = config.currentNote {
                    Label(note, systemImage: "exclamationmark.triangle")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            GroupBox("Port Details") {
                if config.groupEnabled {
                    details
                } else {
                    Text("No serial ports found")
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, minHeight: 100)
                }
            }
            StudioVerbButtons(verb: "Use Port", disabled: !config.groupEnabled, onConfirm: ok, onCancel: { onFinish(nil) }) // STUDIO: a verb, not OK (B9)
        }
        .padding(20)
        .frame(width: 460)
        .dialogAlert($alert)
        .task { await probe() }
    }

    private var details: some View {
        Grid(alignment: .leading, verticalSpacing: 8) {
            GridRow {
                Text("Port name")
                HStack {
                    Picker("Port name", selection: $config.selectedPath) {
                        if config.selectedPath == nil {
                            Text("Choose a Port").tag(String?.none)
                        }
                        ForEach(config.ports, id: \.path) { port in
                            Text(port.label).tag(String?.some(port.path))
                        }
                    }
                    .labelsHidden()
                    Button("Refresh", systemImage: "arrow.clockwise") { refresh() }
                        .labelStyle(.iconOnly)
                        .help("Look for serial ports again")
                }
            }
            GridRow {
                Text("Baud rate")
                Picker("Baud rate", selection: $config.baud) {
                    if config.baud == -1 {
                        Text("").tag(Int32(-1))
                    }
                    ForEach(context.baudRates(), id: \.self) { rate in
                        Text(String(rate)).tag(rate)
                    }
                }
                .labelsHidden()
            }
            GridRow {
                Text("Parity")
                optionPicker("Parity", selection: $config.parity, options: context.parityOptions())
            }
            GridRow {
                Text("Data bits")
                Picker("Data bits", selection: $config.dataBits) {
                    if !(5...8).contains(config.dataBits) {
                        Text("").tag(config.dataBits)
                    }
                    ForEach(Int32(5)...Int32(8), id: \.self) { bits in
                        Text(String(bits)).tag(bits)
                    }
                }
                .labelsHidden()
            }
            GridRow {
                Text("Stop bits")
                optionPicker("Stop bits", selection: $config.stopBits, options: context.stopBitOptions())
            }
        }
        .padding(6)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func optionPicker(_ title: String, selection: Binding<String>, options: [DialogSerialOption]) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Picker(title, selection: selection) {
                ForEach(options, id: \.name) { option in
                    Text(option.name)
                        .tag(option.name)
                        .disabled(!option.supported)
                        .help(option.supported ? "" : Self.unsupportedHelp)
                }
            }
            .labelsHidden()
            if options.contains(where: { $0.name == selection.wrappedValue && !$0.supported }) {
                Text(Self.unsupportedHelp).font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private func refresh() {
        config.ports = context.serialPorts()
        if let selected = config.selectedPath, !config.ports.contains(where: { $0.path == selected }) {
            config.selectedPath = nil
        }
        Task { await probe() }
    }

    /// F8. Opening a port can reset an Arduino, so it only happens as this dialog opens, and never under the test harness.
    private func probe() async {
        guard !AppModel.isMuted else { return }
        let context = context
        let ports = config.ports
        let probed = await offMain { context.probeSerialPorts(ports: ports) }
        if config.ports.map(\.path) == probed.map(\.path) {
            config.ports = probed
        }
    }

    private func ok() {
        let result = context.validateSerialConfig(config: config)
        if let error = result.error {
            alert = .notice("Serial Port", error)
        } else if result.inUse {
            alert = DialogAlert(
                title: "Warning",
                message: "Serial Port may be in use, do you wish to continue?",
                choices: [.init("Use Port") { onFinish(result.settings) }, .init("Choose Another", role: .cancel)]
            )
        } else {
            onFinish(result.settings)
        }
    }
}
