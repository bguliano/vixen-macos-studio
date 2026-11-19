import SwiftUI
import VixenBridge
import VixenKit

/// Studio's sequencer toolbar: the window's native, customisable toolbar (View ▸ Customize Toolbar…), replacing
/// Vixen's draggable tool strips. Every item calls the same shell commands as the menus and keyboard shortcuts.
///
/// Defaults: panel toggles at the edges, editing tools on the left, the transport and clock in the centre, view
/// controls on the right. Less common tools (alignment, export, drag box filter) are available to add.
struct StudioSequencerToolbar: CustomizableToolbarContent {
    let model: SequencerModel

    var body: some CustomizableToolbarContent {
        ToolbarItem(id: "effects-panel", placement: .navigation) {
            panelToggle("Effects", systemImage: "sidebar.leading", panel: .effects,
                        help: "Show or hide the effects library")
        }

        ToolbarItem(id: "mode", placement: .navigation) {
            StudioModePicker(model: model)
        }
        ToolbarItem(id: "snap", placement: .navigation) {
            StudioSnapControl(model: model)
        }

        ToolbarItem(id: "transport", placement: .principal) {
            StudioTransport(model: model)
        }
        .customizationBehavior(.disabled)

        ToolbarItem(id: "zoom", placement: .primaryAction) {
            ControlGroup {
                Button { model.run(.zoomTimeOut) } label: { Label("Zoom Out", systemImage: "minus.magnifyingglass") }
                    .help("Zoom time out (⌘−)")
                Button { model.run(.zoomTimeIn) } label: { Label("Zoom In", systemImage: "plus.magnifyingglass") }
                    .help("Zoom time in (⌘=)")
            } label: {
                Label("Zoom", systemImage: "magnifyingglass")
            }
        }
        ToolbarItem(id: "rows", placement: .primaryAction) {
            Menu {
                Button("Taller Rows") { model.run(.zoomRowsIn) }
                Button("Shorter Rows") { model.run(.zoomRowsOut) }
                Button("Default Row Height") { model.run(.resetRowHeight) }
                Divider()
                Button("Collapse All Groups") { model.run(.collapseAll) }
            } label: {
                Label("Rows", systemImage: "arrow.up.and.down.text.horizontal")
            }
            .help("Row height and grouping")
        }
        ToolbarItem(id: "speed", placement: .primaryAction) {
            StudioSpeedMenu(model: model)
        }
        ToolbarItem(id: "audio", placement: .primaryAction) {
            StudioAudioMenu(model: model)
        }
        ToolbarItem(id: "editor-panel", placement: .primaryAction) {
            panelToggle("Inspector", systemImage: "sidebar.trailing", panel: .effectEditor,
                        help: "Show or hide the effect editor and libraries")
        }

        ToolbarItem(id: "save", placement: .primaryAction, showsByDefault: false) {
            Button { Task { await model.save(forcePanel: false) } } label: {
                Label("Save", systemImage: "square.and.arrow.down")
            }
            .help("Save the sequence (⌘S)")
        }
        ToolbarItem(id: "export", placement: .primaryAction, showsByDefault: false) {
            Button { model.export() } label: { Label("Export", systemImage: "square.and.arrow.up") }
                .disabled(!model.canExport)
                .help("Export the sequence for a player such as FPP")
        }
        ToolbarItem(id: "align", placement: .primaryAction, showsByDefault: false) {
            Menu {
                Button("Align Starts to Marks") { model.run(.alignStartToMarks) }
                Button("Align Ends to Marks") { model.run(.alignEndToMarks) }
                Button("Align Both to Marks") { model.run(.alignBothToMarks) }
                Divider()
                Button("Distribute Equally") { model.run(.distributeEqually) }
                Button("Distribute…") { model.run(.distributeDialog) }
                Button("Close Gaps") { model.run(.closeGaps) }
            } label: {
                Label("Align", systemImage: "align.horizontal.left")
            }
            .help("Align and distribute selected effects")
        }
    }

    private func panelToggle(_ title: String, systemImage: String, panel: ShellPanel, help: String) -> some View {
        Toggle(isOn: Binding(
            get: { model.panels?.visible.contains(panel) ?? false },
            set: { _ in model.togglePanel(panel) }
        )) {
            Label(title, systemImage: systemImage)
        }
        .toggleStyle(.button)
        .help(help)
    }
}

/// Select or Draw: the two pointer modes, labelled.
private struct StudioModePicker: View {
    let model: SequencerModel

    var body: some View {
        Picker("Mode", selection: Binding(
            get: { (model.ui?.drawMode ?? false) ? 1 : 0 },
            set: { model.run($0 == 1 ? .drawMode : .selectionMode) }
        )) {
            Label("Select", systemImage: "cursorarrow").tag(0)
            Label("Draw", systemImage: "pencil").tag(1)
        }
        .pickerStyle(.segmented)
        .labelStyle(.titleAndIcon)
        .fixedSize()
        .help("Select moves and resizes effects; Draw paints new effects onto rows")
    }
}

/// Snapping, with its strength in the same control.
private struct StudioSnapControl: View {
    let model: SequencerModel

    var body: some View {
        let snapOn = model.ui?.prefs.snapTo ?? true
        let strength = Int(model.ui?.prefs.snapStrength ?? 2)
        Menu {
            Toggle("Snap to Marks and Effects", isOn: Binding(get: { snapOn }, set: { model.set(.snapTo, $0) }))
            Divider()
            Picker("Strength", selection: Binding(get: { strength }, set: { setStrength($0) })) {
                Text("Gentle").tag(1)
                Text("Normal").tag(2)
                Text("Strong").tag(3)
                Text("Strongest").tag(4)
            }
            .pickerStyle(.inline)
        } label: {
            Label(snapOn ? "Snap" : "Snap Off", systemImage: snapOn ? "dot.squareshape.split.2x2" : "squareshape.split.2x2.dotted")
        } primaryAction: {
            model.set(.snapTo, !snapOn)
        }
        .help("Click to turn snapping on or off; hold for strength")
    }

    private func setStrength(_ value: Int) {
        model.shell?.setSnapStrength(strength: Int32(value))
        model.refresh()
    }
}

/// Transport buttons and the clock, centred in the toolbar.
private struct StudioTransport: View {
    let model: SequencerModel

    var body: some View {
        let transport = model.ui?.transport
        HStack(spacing: StudioMetrics.spacingS) {
            ControlGroup {
                Button { model.run(.gotoStart) } label: { Label("Go to Start", systemImage: "backward.end.fill") }
                    .help("Go to start (Home)")
                Button { model.run(.playPause) } label: {
                    Label(playTitle(transport?.icon), systemImage: playImage(transport?.icon))
                }
                .disabled(!(transport?.playEnabled ?? false))
                .help(transport?.playTooltip ?? "Play (Space)")
                Button { model.run(.stop) } label: { Label("Stop", systemImage: "stop.fill") }
                    .disabled(!(transport?.stopEnabled ?? false))
                    .help("Stop (F8)")
                Button { model.run(.gotoEnd) } label: { Label("Go to End", systemImage: "forward.end.fill") }
                    .help("Go to end (End)")
            }
            .controlGroupStyle(.navigation)
            Toggle(isOn: Binding(get: { transport?.looping ?? false }, set: { _ in model.run(.toggleLoop) })) {
                Label("Loop", systemImage: "repeat")
            }
            .toggleStyle(.button)
            .help("Loop playback (F9)")
            StudioClock(model: model)
        }
    }

    private func playImage(_ icon: ShellPlayIcon?) -> String {
        switch icon {
        case .pause: "pause.fill"
        case .hourglass: "hourglass"
        default: "play.fill"
        }
    }

    private func playTitle(_ icon: ShellPlayIcon?) -> String {
        switch icon {
        case .pause: "Pause"
        case .hourglass: "Rendering"
        default: "Play"
        }
    }
}

/// Position and length, plus rendering progress, so it's always clear whether playback will show finished effects.
private struct StudioClock: View {
    let model: SequencerModel

    var body: some View {
        let ui = model.ui
        HStack(spacing: StudioMetrics.spacingS) {
            Text(model.positionText)
                .font(.system(.title3, design: .monospaced).weight(.medium))
                .accessibilityLabel("Position \(model.positionText)")
            Text("/ \(ui?.lengthText ?? "0:00.000")")
                .font(.system(.callout, design: .monospaced))
                .foregroundStyle(.secondary)
            if let ui, ui.renderingVisible {
                ProgressView(value: Double(max(0, min(100, ui.renderingPercent))), total: 100)
                    .progressViewStyle(.circular)
                    .controlSize(.small)
                    .help("Rendering effects: \(ui.renderingPercent)%")
            }
        }
        .monospacedDigit()
        .padding(.horizontal, StudioMetrics.spacingM)
        .padding(.vertical, 3)
        .fixedSize()
    }
}

/// Playback speed, for practising timing at a slower tempo.
private struct StudioSpeedMenu: View {
    let model: SequencerModel

    var body: some View {
        let speed = model.ui?.speed
        Menu {
            Button("Slower") { model.run(.speedDecrease) }
                .disabled(!(speed?.decreaseEnabled ?? false))
            Button("Faster") { model.run(.speedIncrease) }
                .disabled(!(speed?.increaseEnabled ?? false))
            Divider()
            Button("Normal Speed") { model.run(.speedReset) }
            Button("Maximum Speed") { model.run(.speedMax) }
        } label: {
            Label("Speed \(speed?.gaugeText ?? "100")%", systemImage: "gauge.with.dots.needle.50percent")
        }
        .disabled(!(speed?.gaugeEnabled ?? false))
        .help("Playback speed. Changes how fast the sequence plays while editing, not the saved sequence.")
    }
}

/// The sequence's audio and the output device.
private struct StudioAudioMenu: View {
    let model: SequencerModel
    @State private var devices: [ShellAudioDevice] = []

    var body: some View {
        let ui = model.ui
        Menu {
            Button(ui?.removeAudioEnabled == true ? "Change Audio…" : "Add Audio…") { model.associateAudio() }
            Button("Remove Audio") { model.removeAudio() }
                .disabled(!(ui?.removeAudioEnabled ?? false))
            #if canImport(VixenMarks)
            Button("Detect Beats and Bars…") { model.sheet = .beatsAndBars }
                .disabled(!(ui?.beatBarEnabled ?? false))
            #endif
            Divider()
            Section("Output Device") {
                ForEach(devices, id: \.id) { device in
                    Toggle(device.name, isOn: Binding(get: { device.checked }, set: { _ in
                        model.shell?.setAudioDevice(id: device.id)
                        devices = model.shell?.audioDevices() ?? []
                    }))
                }
            }
        } label: {
            Label("Audio", systemImage: "music.note")
        }
        .onAppear { devices = model.shell?.audioDevices() ?? [] }
        .help(ui?.audioTooltip ?? "Add audio to this sequence")
    }
}
