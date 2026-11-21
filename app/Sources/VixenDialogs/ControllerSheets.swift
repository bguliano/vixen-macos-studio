import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// Generic Serial Configuration (F26 to F35).
struct GenericSerialSheet: View {
    let context: DialogContext
    let controller: String
    let onFinish: (Bool) -> Void
    @State private var draft: DialogGenericSerial
    @State private var choosingPort = false
    @State private var alert: DialogAlert?
    @State private var live = StudioLiveSheet(step: "Configure Controller") // STUDIO: live sheet, Undo while it's up (B9)

    init(context: DialogContext, controller: String, draft: DialogGenericSerial, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.controller = controller
        self.onFinish = onFinish
        _draft = State(initialValue: draft)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Generic Serial Configuration")
            GroupBox("Serial Port Settings") {
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text("Currently:")
                        Text(context.serialSettingsLabel(settings: draft.settings)).fontWeight(.medium)
                    }
                    if let port = draft.settings?.portName, let note = context.savedPortNote(name: port) {
                        Label(note, systemImage: "exclamationmark.triangle")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    HStack {
                        Spacer()
                        Button("Setup/Change Serial Port") { choosingPort = true }
                    }
                }
                .padding(6)
            }
            GroupBox("Packet Data") {
                VStack(alignment: .leading, spacing: 6) {
                    Toggle("Send a text header", isOn: $draft.headerOn).toggleStyle(.checkbox)
                    TextField("", text: $draft.header)
                        .labelsHidden()
                        .disabled(!draft.headerOn)
                        .padding(.leading, 20)
                    Toggle("Send a text footer", isOn: $draft.footerOn).toggleStyle(.checkbox)
                    TextField("", text: $draft.footer)
                        .labelsHidden()
                        .disabled(!draft.footerOn)
                        .padding(.leading, 20)
                }
                .padding(6)
            }
            StudioDoneButton(live) { onFinish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 440)
        .studioLive(live, draft: $draft) { d in guard d.settings != nil else { throw StudioLiveHold() }; _ = try context.applyGenericSerial(controller: controller, draft: d) } // STUDIO: live sheet, Undo while it's up (B9)
        .sheet(isPresented: $choosingPort) {
            SerialPortConfigSheet(context: context, current: draft.settings) { settings in
                choosingPort = false
                if let settings {
                    draft.settings = settings
                }
            }
        }
        .dialogAlert($alert)
    }

    private func ok() {
        do {
            onFinish(try context.applyGenericSerial(controller: controller, draft: draft))
        } catch {
            alert = .notice("Generic Serial Configuration", error.userMessage)
        }
    }
}

/// Renard and DMX Pro go straight to Serial Port Config (F100, F102).
struct SerialOnlySheet: View {
    let context: DialogContext
    let controller: String
    let current: DialogSerialSettings?
    let onFinish: (Bool) -> Void

    var body: some View {
        SerialPortConfigSheet(context: context, current: current) { settings in
            guard let settings else {
                onFinish(false)
                return
            }
            onFinish((try? context.applySerialOnly(controller: controller, settings: settings)) != nil)
        }
    }
}

/// Dummy Lighting Configuration (F38 to F45). Reads from the data, so it never needs a running controller.
struct DummyLightingSheet: View {
    let context: DialogContext
    let controller: String
    let onFinish: (Bool) -> Void
    @State private var draft: DialogDummyLighting
    @State private var live = StudioLiveSheet(step: "Configure Controller") // STUDIO: live sheet, Undo while it's up (B9)

    init(context: DialogContext, controller: String, draft: DialogDummyLighting, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.controller = controller
        self.onFinish = onFinish
        _draft = State(initialValue: draft)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Dummy Lighting Configuration")
            HStack {
                Text("Window Title:")
                TextField("", text: $draft.title).labelsHidden()
            }
            Picker("", selection: $draft.style) {
                Text("Set up as monochrome channels").tag(DialogRenderStyle.monochrome)
                Text("Set up as RGB channels, with every 3 inputs mapping to the R, G, B components of a single RGB output")
                    .fixedSize(horizontal: false, vertical: true)
                    .tag(DialogRenderStyle.rgbMultiChannel)
                Text("Set up as RGB channels, with every input\nmapping to a full-color RGB channel")
                    .fixedSize(horizontal: false, vertical: true)
                    .tag(DialogRenderStyle.rgbSingleChannel)
            }
            .pickerStyle(.radioGroup)
            .labelsHidden()
            StudioDoneButton(live) { onFinish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 420)
        .studioLive(live, draft: $draft) { try context.applyDummyLighting(controller: controller, draft: $0) } // STUDIO: live sheet, Undo while it's up (B9)
    }
}

/// DDP Configuration (F95 to F98).
struct DdpSheet: View {
    let context: DialogContext
    let controller: String
    let onFinish: (Bool) -> Void
    @State private var draft: DialogDdp
    @State private var resolving = false
    @State private var live = StudioLiveSheet(step: "Configure Controller") // STUDIO: live sheet, Undo while it's up (B9)
    @State private var alert: DialogAlert?
    @FocusState private var fieldFocused: Bool

    init(context: DialogContext, controller: String, draft: DialogDdp, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.controller = controller
        self.onFinish = onFinish
        _draft = State(initialValue: draft)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("DDP Configuration")
            GroupBox("Configuration") {
                VStack(alignment: .leading, spacing: 10) {
                    Picker("", selection: $draft.hostMode) {
                        Text("IP Address").tag(false)
                        Text("Host Name").tag(true)
                    }
                    .pickerStyle(.radioGroup)
                    .horizontalRadioGroupLayout()
                    .labelsHidden()
                    HStack {
                        if draft.hostMode {
                            Text("Host Name:")
                            TextField("", text: $draft.hostText).labelsHidden().focused($fieldFocused)
                        } else {
                            Text("IP Address:")
                            TextField("", text: $draft.ipText).labelsHidden()
                        }
                    }
                }
                .padding(6)
            }
            HStack {
                if resolving {
                    ProgressView().controlSize(.small)
                }
                StudioDoneButton(live) { onFinish($0) } // STUDIO: live, closes with Done (B9)
            }
        }
        .padding(20)
        .frame(width: 380)
        // A host name resolves as it applies, so typing waits a little longer before it does.
        .studioLive(live, draft: $draft, delay: .milliseconds(1200)) { d in guard context.ddpCanApply(draft: d) else { throw StudioLiveHold() }; try context.applyDdp(controller: controller, draft: d) } // STUDIO: live sheet, Undo while it's up (B9)
        .dialogAlert($alert)
    }

    private func ok() {
        resolving = true
        let context = context
        let controller = controller
        let draft = draft
        Task {
            let failure: (any Error)? = await offMain {
                do {
                    try context.applyDdp(controller: controller, draft: draft)
                    return nil
                } catch {
                    return error
                }
            }
            resolving = false
            if let failure {
                alert = .notice("DDP Setup Error", failure.userMessage) { fieldFocused = true }
            } else {
                onFinish(true)
            }
        }
    }
}

/// OpenDMX Setup (F104 to F107).
struct OpenDmxSheet: View {
    let context: DialogContext
    let controller: String
    let onFinish: (Bool) -> Void
    @State private var draft: DialogOpenDmx
    @State private var live = StudioLiveSheet(step: "Configure Controller") // STUDIO: live sheet, Undo while it's up (B9)

    init(context: DialogContext, controller: String, draft: DialogOpenDmx, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.controller = controller
        self.onFinish = onFinish
        _draft = State(initialValue: draft)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("OpenDMX Setup")
            Picker("", selection: $draft.selected) {
                if draft.devices.isEmpty {
                    Text("").tag(UInt32?.none)
                }
                ForEach(Array(draft.devices.enumerated()), id: \.offset) { index, device in
                    Text(device.label).tag(UInt32?.some(UInt32(index)))
                }
            }
            .labelsHidden()
            .frame(maxWidth: .infinity, alignment: .leading)
            StudioDoneButton(live) { onFinish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 420)
        .studioLive(live, draft: $draft) { try context.applyOpenDmx(controller: controller, draft: $0) } // STUDIO: live sheet, Undo while it's up (B9)
    }
}

/// Elexol USB I/O 24 Configuration (F109 to F115). The title's USB is upstream's own wording.
struct ElexolSheet: View {
    let context: DialogContext
    let controller: String
    let onFinish: (Bool) -> Void
    @State private var draft: DialogElexol
    @State private var testing = false
    @State private var live = StudioLiveSheet(step: "Configure Controller") // STUDIO: live sheet, Undo while it's up (B9)
    @State private var alert: DialogAlert?

    init(context: DialogContext, controller: String, draft: DialogElexol, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.controller = controller
        self.onFinish = onFinish
        _draft = State(initialValue: draft)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Elexol USB I/O 24 Configuration")
            GroupBox("Ethernet Settings") {
                HStack(spacing: 12) {
                    Grid(alignment: .leading, verticalSpacing: 8) {
                        GridRow {
                            Text("IP Address")
                            TextField("", text: $draft.ipText).labelsHidden()
                        }
                        GridRow {
                            Text("Port")
                            TextField("", text: $draft.portText).labelsHidden().monospacedDigit()
                        }
                    }
                    Button("Test", action: test)
                        .disabled(testing)
                        .help("Send a test command to the device")
                }
                .padding(6)
            }
            GroupBox("Advanced") {
                VStack(spacing: 4) {
                    HStack {
                        Text("Minium 'ON' Intensity (0-255):")
                        IntSlider(value: $draft.minIntensity, range: 0...255, tickEvery: 16)
                    }
                    Text(String(draft.minIntensity)).monospacedDigit()
                }
                .padding(6)
            }
            StudioDoneButton(live) { onFinish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 460)
        .studioLive(live, draft: $draft) { try context.applyElexol(controller: controller, draft: $0) } // STUDIO: live sheet, Undo while it's up (B9)
        .dialogAlert($alert)
    }

    /// Only ever runs when clicked.
    private func test() {
        testing = true
        let context = context
        let (ip, port) = (draft.ipText, draft.portText)
        Task {
            let text = await offMain { context.testElexol(ip: ip, port: port) }
            testing = false
            alert = .notice("Test Connection", text)
        }
    }

    private func ok() {
        do {
            try context.applyElexol(controller: controller, draft: draft)
            onFinish(true)
        } catch {
            let message = error.userMessage
            alert = .notice("Error", message)
        }
    }
}

/// Launcher Commands and RDS Configuration (F116 to F129). Upstream writes through and has only Save. This is staged, with Cancel.
struct RdsSheet: View {
    let context: DialogContext
    let controller: String
    let onFinish: (Bool) -> Void
    @State private var draft: DialogRds
    @State private var ports: [DialogSerialPort]
    @State private var testText = ""
    @State private var status = "DIYC"
    @State private var sending = false
    @State private var live = StudioLiveSheet(step: "Configure Controller") // STUDIO: live sheet, Undo while it's up (B9)

    init(context: DialogContext, controller: String, draft: DialogRds, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.controller = controller
        self.onFinish = onFinish
        _draft = State(initialValue: draft)
        _ports = State(initialValue: context.serialPorts())
    }

    var body: some View {
        let rules = context.rdsRules(draft: draft)
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .top, spacing: 14) {
                VStack(alignment: .leading, spacing: 12) {
                    DialogTitle("Launcher Commands and RDS Configuration")
                    GroupBox("RDS Hardware") { hardware(rules).padding(6) }
                    GroupBox("Test Interface") {
                        HStack {
                            Text("Macro value {text}")
                            TextField("", text: Binding(get: { testText }, set: { testText = String($0.prefix(Int(rules.testTextMax))) }))
                                .labelsHidden()
                            Button("Send", action: send).disabled(sending)
                        }
                        .padding(6)
                    }
                }
                VStack(alignment: .leading, spacing: 12) {
                    if let logo = VixenResources.image("dialogs/rds-logo.png") {
                        // Black ink on nothing, which a dark sheet would hide.
                        Image(nsImage: logo).renderingMode(.template).resizable().aspectRatio(contentMode: .fit).frame(width: 160, height: 90)
                            .foregroundStyle(.secondary)
                    }
                    GroupBox("Options") {
                        VStack(alignment: .leading, spacing: 6) {
                            Toggle("BiDirectional", isOn: $draft.biDirectional).toggleStyle(.checkbox)
                            Toggle("Slow", isOn: $draft.slow).toggleStyle(.checkbox)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(6)
                    }
                    Spacer()
                    StudioDoneButton(live) { onFinish($0) } // STUDIO: live, closes with Done (B9)
                }
                .frame(width: 190)
            }
            .padding(20)
            Divider()
            Text(status)
                .font(.caption)
                .padding(.horizontal, 12)
                .padding(.vertical, 4)
        }
        .frame(width: 720)
        .studioLive(live, draft: $draft) { try context.applyRds(controller: controller, draft: $0) } // STUDIO: live sheet, Undo while it's up (B9)
    }

    private func hardwareBinding() -> Binding<DialogRdsHardware> {
        Binding(get: { draft.hardware }, set: { draft = context.rdsWithHardware(draft: draft, hardware: $0) })
    }

    private func hardware(_ rules: DialogRdsRules) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 16) {
                Grid(alignment: .leading, verticalSpacing: 4) {
                    radioRow(.mrds192, "MRDS192", .mrds1322, "MRDS1322")
                    radioRow(.http, "HTTP(S)", .vfmt212r, "V-FMT212R")
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text("Serial Port")
                    Picker("Serial Port", selection: $draft.portPath) {
                        if draft.portPath == nil {
                            Text(draft.unavailablePort.map { "\($0) (unavailable)" } ?? "").tag(String?.none)
                        }
                        ForEach(ports, id: \.path) { port in
                            Text(port.label).tag(String?.some(port.path))
                        }
                    }
                    .labelsHidden()
                    .frame(minWidth: 170, alignment: .leading)
                    .disabled(!rules.serialEnabled)
                }
            }
            HStack {
                Text("URL").foregroundStyle(rules.urlEnabled ? .primary : .secondary)
                TextField("", text: $draft.url).labelsHidden().disabled(!rules.urlEnabled)
            }
            HStack(spacing: 8) {
                Text("Macro Fields:").foregroundStyle(.secondary)
                Text("{text}, {time}").foregroundStyle(.secondary)
            }
            Toggle("HTTP(s) Requires Authentication", isOn: $draft.requiresAuth)
                .toggleStyle(.checkbox)
                .disabled(!rules.authEnabled)
            HStack {
                Text("UserName").foregroundStyle(rules.credentialsEnabled ? .primary : .secondary)
                TextField("", text: limited($draft.username)).labelsHidden().disabled(!rules.credentialsEnabled)
                Text("Password").foregroundStyle(rules.credentialsEnabled ? .primary : .secondary)
                SecureField("", text: limited($draft.password)).labelsHidden().disabled(!rules.credentialsEnabled)
            }
        }
    }

    /// F123: eight characters each.
    private func limited(_ text: Binding<String>) -> Binding<String> {
        Binding(get: { text.wrappedValue }, set: { text.wrappedValue = String($0.prefix(8)) })
    }

    private func radioRow(_ a: DialogRdsHardware, _ aTitle: String, _ b: DialogRdsHardware, _ bTitle: String) -> some View {
        GridRow {
            radio(a, aTitle)
            radio(b, bTitle)
        }
    }

    private func radio(_ value: DialogRdsHardware, _ title: String) -> some View {
        Button {
            hardwareBinding().wrappedValue = value
        } label: {
            HStack(spacing: 4) {
                Image(systemName: draft.hardware == value ? "largecircle.fill.circle" : "circle")
                    .foregroundStyle(draft.hardware == value ? Color.accentColor : .secondary)
                Text(title)
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(title)
        .accessibilityAddTraits(draft.hardware == value ? .isSelected : [])
    }

    /// Only ever runs when clicked. Reports what really happened, where upstream reports that a request started.
    private func send() {
        sending = true
        let context = context
        let controller = controller
        let (draft, text) = (draft, testText)
        Task {
            let result = await offMain { (try? context.sendRdsTest(controller: controller, draft: draft, text: text)) ?? "Data Not Sent" }
            sending = false
            status = result
        }
    }
}

/// Launcher Controller Configuration (F131, F132). Staged, with Cancel, where upstream writes through.
struct LauncherSheet: View {
    let context: DialogContext
    let controller: String
    let onFinish: (Bool) -> Void
    @State private var hide: Bool
    @State private var live = StudioLiveSheet(step: "Configure Controller") // STUDIO: live sheet, Undo while it's up (B9)

    init(context: DialogContext, controller: String, hide: Bool, onFinish: @escaping (Bool) -> Void) {
        self.context = context
        self.controller = controller
        self.onFinish = onFinish
        _hide = State(initialValue: hide)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            DialogTitle("Launcher Controller Configuration")
            Toggle("Hide Launched Windows", isOn: $hide).toggleStyle(.checkbox)
            StudioDoneButton(live) { onFinish($0) } // STUDIO: live, closes with Done (B9)
        }
        .padding(20)
        .frame(width: 340)
        .studioLive(live, draft: $hide) { try context.applyLauncher(controller: controller, hideLaunchedWindows: $0) } // STUDIO: live sheet, Undo while it's up (B9)
    }
}
