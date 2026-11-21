import AppKit
import SwiftUI
import VixenBridge
import VixenKit

#if canImport(VixenTimeline)
import VixenTimeline
#endif

/// The menu bar: `Sequence, Edit, View, Tools, Help` with upstream's items, order and separators. Commands act on the active sequencer window, which stays active while a floating
/// panel has the keyboard (F225).
struct SequencerCommands: Commands {
    let registry: SequencerRegistry

    var body: some Commands {
        CommandMenu("Sequence") {
            SequenceMenu(registry: registry)
        }
        CommandGroup(replacing: .undoRedo) {
            UndoRedoItems(registry: registry)
        }
        CommandGroup(replacing: .pasteboard) {
            PasteboardItems(registry: registry)
        }
        CommandGroup(after: .pasteboard) {
            EditExtrasItems(registry: registry)
        }
        CommandGroup(after: .toolbar) {
            ViewMenuItems(registry: registry)
        }
        CommandMenu("Tools") {
            ToolsMenu(registry: registry)
        }
        CommandGroup(after: .help) {
            HelpItems(registry: registry)
        }
    }
}

/// What most items need: enabled only with a loaded sequence in front, and not during a system configuration session.
private extension SequencerRegistry {
    var ready: SequencerModel? {
        guard let active, active.ui != nil, !active.app.systemConfigSessionActive else { return nil }
        return active
    }
}

private func toggle(_ title: String, _ value: Bool?, _ set: @escaping (Bool) -> Void) -> some View {
    Toggle(title, isOn: Binding(get: { value ?? false }, set: set))
        .disabled(value == nil)
}

// MARK: Sequence, F24 to F38

private struct SequenceMenu: View {
    let registry: SequencerRegistry

    var body: some View {
        let model = registry.ready
        let ui = model?.ui
        let transport = ui?.transport
        Button("Save") {
            guard let focused = registry.focusedModel(), focused === registry.ready else { return }
            Task { await focused.save(forcePanel: false) }
        }
        .keyboardShortcut(registry.inFront ? KeyboardShortcut("s", modifiers: .command) : nil)
        .disabled(model == nil)
        // ⇧⌘S is the align-to-mark-start hotkey, so Save As is ⌥⌘S (F25, F198).
        Button("Save As...") {
            guard let focused = registry.focusedModel(), focused === registry.ready else { return }
            Task { await focused.save(forcePanel: true) }
        }
        .keyboardShortcut(registry.inFront ? KeyboardShortcut("s", modifiers: [.command, .option]) : nil)
        .disabled(model == nil)
        toggle("Auto Save", ui?.prefs.autoSave) { model?.set(.autoSave, $0) }
        Divider()
        Menu("Playback") {
            Button("Play") { model?.run(.playPause) }
                .keyboardShortcut(registry.inFront && transport?.playHasShortcut == true ? KeyboardShortcut(.init(Character(UnicodeScalar(NSF5FunctionKey)!)), modifiers: []) : nil)
                .disabled(!(transport?.playMenuEnabled ?? false))
            // Always disabled, as upstream's (KEEP F28): pausing is done by Play.
            Button("Pause") {}
                .disabled(!(transport?.pauseMenuEnabled ?? false))
            Button("Stop") { model?.run(.stop) }
                .keyboardShortcut(registry.inFront ? KeyboardShortcut(.init(Character(UnicodeScalar(NSF8FunctionKey)!)), modifiers: []) : nil)
                .disabled(!(transport?.stopEnabled ?? false))
            Toggle("Loop", isOn: Binding(get: { transport?.looping ?? false }, set: { _ in model?.run(.toggleLoop) }))
                .keyboardShortcut(registry.inFront ? KeyboardShortcut(.init(Character(UnicodeScalar(NSF9FunctionKey)!)), modifiers: []) : nil)
                .disabled(model == nil)
            Menu("Play Options") {
                delayItem("Delay Off", .off, model)
                delayItem("Delay 5 Seconds", .seconds5, model)
                delayItem("Delay 10 Seconds", .seconds10, model)
                delayItem("Delay 20 Seconds", .seconds20, model)
                delayItem("Delay 30 Seconds", .seconds30, model)
                delayItem("Delay 60 Seconds", .seconds60, model)
            }
        }
        Divider()
        Button("Export") { model?.export() }
            .disabled(!(model?.canExport ?? false))
        Divider()
        // ⌘Q stays Quit, so Close is ⌘W (F38).
        Button("Close") { registry.active?.window?.performClose(nil) }
            .keyboardShortcut(registry.inFront ? KeyboardShortcut("w", modifiers: .command) : nil)
            .disabled(registry.active == nil)
        if UITestHarness.isActive {
            Divider()
            Button("Write Test State") { registry.active?.writeTestState() }
            #if canImport(VixenTimeline)
            Button("Test Color Red") {
                ColorPanelSession.shared.testSetColor(0xFFFF_0000)
                ColorPanelSession.shared.finish()
            }
            // 800,335 from the top left, in window coordinates: the second relay row with the toolbars as they first show.
            Button("Test Pointer") {
                if let window = registry.active?.window {
                    registry.active?.testPointer = NSPoint(x: 800, y: window.frame.height - 335)
                }
            }
            Menu("Test Keys") {
                testKey("Home", .home)
                testKey("End", .end)
                testKey("PageUp", .pageUp)
                testKey("PageDown", .pageDown)
                testKey("Cmd Home", .home, cmd: true)
                testKey("Cmd End", .end, cmd: true)
                testKey("Cmd PageUp", .pageUp, cmd: true)
                testKey("Cmd PageDown", .pageDown, cmd: true)
                testKey("F5", .f5)
                testKey("F8", .f8)
                testKey("F9", .f9)
                testKey("Space", .space)
                testKey("Shift Space", .space, shift: true)
                testKey("Escape", .escape)
                testKey("Up", .up)
                testKey("Down", .down)
                testKey("Delete", .delete)
                ForEach(["s", "e", "b", "c", "u", "r", "n", "d", "i", "t"], id: \.self) { letter in
                    testKey(letter, .letter(value: letter))
                    testKey("Shift \(letter)", .letter(value: letter), shift: true)
                    testKey("Cmd \(letter)", .letter(value: letter), cmd: true)
                }
            }
            Button("Test Shift On") { registry.active?.testShift = true }
            Button("Test Shift Off") { registry.active?.testShift = false }
            Button("Test Drop Red") { registry.active?.testDrop(.color(argb: 0xFFFF_0000)) }
            Button("Test Drop Gradient") { registry.active?.testDrop(.gradient(value: solidGradientValue(argb: 0xFF00_FF00))) }
            Button("Test Drop Curve") { registry.active?.testDrop(.curve(value: flat100CurveValue())) }
            Button("Test Drop Pulse On Row") {
                registry.active?.testDropOnRow(typeId: registry.active?.effects.first { $0.name == "Pulse" }?.typeId, file: nil, row: 1)
            }
            Button("Test Drop File On Row") {
                registry.active?.testDropOnRow(typeId: nil, file: ProcessInfo.processInfo.environment["VIXEN_TEST_FILE"], row: 1)
            }
            Menu("Test Context Menu") {
                ForEach(registry.active?.testContextPaths ?? [], id: \.self) { path in
                    Button(path) { registry.active?.performContextMenuItem(path: path) }
                }
            }
            #endif
        }
    }

    private func testKey(_ title: String, _ key: ShellKey, shift: Bool = false, cmd: Bool = false) -> some View {
        Button(title) {
            guard let model = registry.active, let shell = model.shell else { return }
            model.notePointerForKeys()
            model.afterCommand(shell.keyDown(key: key, shift: shift, cmd: cmd, opt: false))
        }
    }

    private func delayItem(_ title: String, _ delay: ShellDelay, _ model: SequencerModel?) -> some View {
        Toggle(title, isOn: Binding(
            get: { model?.ui?.transport.delay == delay },
            set: { _ in
                model?.shell?.setDelay(delay: delay)
                model?.refresh()
            }
        ))
        .disabled(model == nil)
    }
}

// MARK: Edit, F39 to F53

/// Text fields keep their own: the standard action goes down the responder chain whenever no sequencer is in front or a text view has the keyboard.
@MainActor
private func textIsFocused(_ registry: SequencerRegistry) -> Bool {
    // The harness runs with no key window at all.
    guard let key = NSApp.keyWindow else { return !UITestHarness.isActive }
    if key.firstResponder is NSText {
        return true
    }
    return registry.active?.window !== key && !(key is FloatingPanelWindow)
}

private struct UndoRedoItems: View {
    let registry: SequencerRegistry

    var body: some View {
        let ui = registry.inFront ? registry.ready?.ui : nil
        Button(ui?.undoName.map { "Undo \($0)" } ?? "Undo") {
            if textIsFocused(registry) {
                NSApp.sendAction(Selector(("undo:")), to: nil, from: nil)
            } else {
                registry.ready?.undo()
            }
        }
        .keyboardShortcut("z", modifiers: .command)
        Button(ui?.redoName.map { "Redo \($0)" } ?? "Redo") {
            if textIsFocused(registry) {
                NSApp.sendAction(Selector(("redo:")), to: nil, from: nil)
            } else {
                registry.ready?.redo()
            }
        }
        .keyboardShortcut("z", modifiers: [.command, .shift])
        if registry.inFront {
            // Upstream's Ctrl+Y, accepted as a hidden equivalent (F46).
            Button("Redo") {
                if !textIsFocused(registry) {
                    registry.ready?.redo()
                }
            }
            .keyboardShortcut("y", modifiers: .command)
            .hidden()
        }
    }
}

/// With a sequencer in front these are upstream's Edit menu. Otherwise they're the standard items, for text fields everywhere else.
private struct PasteboardItems: View {
    let registry: SequencerRegistry

    var body: some View {
        let model = registry.ready
        let ui = model?.ui
        if registry.inFront {
            Menu("Add Effect") {
                ForEach(registry.active?.effects ?? [], id: \.typeId) { effect in
                    Button {
                        _ = model?.shell?.addEffect(typeId: effect.typeId)
                        model?.refresh()
                    } label: {
                        Text(effect.name)
                    }
                }
            }
            .disabled(model == nil)
            // STUDIO-BEGIN place-effect-on-marks: the effects_on_marks command's sheet
            Button("Place Effect on Marks…") { model?.openPlaceOnMarks() }
                .disabled(model == nil)
            // STUDIO-END place-effect-on-marks
            Divider()
        }
        Button("Cut") {
            if textIsFocused(registry) {
                NSApp.sendAction(#selector(NSText.cut(_:)), to: nil, from: nil)
            } else {
                model?.cut()
            }
        }
        .keyboardShortcut("x", modifiers: .command)
        Button("Copy") {
            if textIsFocused(registry) {
                NSApp.sendAction(#selector(NSText.copy(_:)), to: nil, from: nil)
            } else {
                model?.copy()
            }
        }
        .keyboardShortcut("c", modifiers: .command)
        Button("Paste") {
            if textIsFocused(registry) {
                NSApp.sendAction(#selector(NSText.paste(_:)), to: nil, from: nil)
            } else {
                model?.paste(.default)
            }
        }
        .keyboardShortcut("v", modifiers: .command)
        if registry.inFront {
            Menu("Paste Special") {
                Button("Invert") { model?.paste(.invert) }
                    .disabled(!(ui?.pasteInvertEnabled ?? false))
                Button("Visible Marks") { model?.paste(.visibleMarks) }
                    .disabled(!(ui?.pasteMarksEnabled ?? false))
            }
            .disabled(!(ui?.pasteSpecialEnabled ?? false))
        }
        Divider()
    }
}

private struct EditExtrasItems: View {
    let registry: SequencerRegistry

    var body: some View {
        let model = registry.ready
        let ui = model?.ui
        if registry.inFront {
            // Upstream's separators after Paste Special and after Redo meet here, since Undo and Redo lead the Mac's Edit menu; the pasteboard group ends with one.
            Button("Select All Elements") {
                if textIsFocused(registry) {
                    NSApp.sendAction(#selector(NSText.selectAll(_:)), to: nil, from: nil)
                } else {
                    model?.run(.selectAll)
                }
            }
            .keyboardShortcut("a", modifiers: .command)
            Button("Delete") {
                if textIsFocused(registry) {
                    NSApp.sendAction(#selector(NSText.delete(_:)), to: nil, from: nil)
                } else {
                    model?.run(.deleteSelected)
                }
            }
            .keyboardShortcut(.delete, modifiers: [])
            .disabled(!(ui?.hasSelection ?? false))
            Divider()
            toggle("Snap To Marks / Effects", ui?.prefs.snapTo) { model?.set(.snapTo, $0) }
            Menu("Resize / Draw Indicator") {
                toggle("Enabled", ui?.prefs.resizeIndicator) { model?.set(.resizeIndicator, $0) }
                Divider()
                ForEach(["Blue", "Yellow", "Green", "White", "Red"], id: \.self) { name in
                    toggle(name, ui.map { $0.prefs.resizeIndicatorColor == name }) { _ in
                        model?.shell?.setResizeIndicatorColor(name: name)
                        model?.refresh()
                    }
                }
            }
            toggle("CAD Style Selection Box", ui?.prefs.cadStyle) { model?.set(.cadStyle, $0) }
            toggle("Legacy Cursor / Active Row", ui?.prefs.legacyCursor) { model?.set(.legacyCursor, $0) }
        } else {
            Button("Delete") {
                NSApp.sendAction(#selector(NSText.delete(_:)), to: nil, from: nil)
            }
            Button("Select All") {
                NSApp.sendAction(#selector(NSText.selectAll(_:)), to: nil, from: nil)
            }
            .keyboardShortcut("a", modifiers: .command)
        }
    }
}

// MARK: View, F54 to F77

private struct ViewMenuItems: View {
    let registry: SequencerRegistry

    var body: some View {
        if registry.inFront {
            items
        }
    }

    @ViewBuilder
    private var items: some View {
        let model = registry.ready
        let ui = model?.ui
        Button("Zoom Time In") { model?.run(.zoomTimeIn) }
            .keyboardShortcut("=", modifiers: .command)
            .disabled(model == nil)
        Button("Zoom Time Out") { model?.run(.zoomTimeOut) }
            .keyboardShortcut("-", modifiers: .command)
            .disabled(model == nil)
        toggle("Zoom Under Mouse Position", ui?.prefs.zoomUnderMouse) { model?.set(.zoomUnderMouse, $0) }
        Divider()
        Button("Zoom Rows In") { model?.run(.zoomRowsIn) }
            .keyboardShortcut("=", modifiers: [.command, .shift])
            .disabled(model == nil)
        Button("Zoom Rows Out") { model?.run(.zoomRowsOut) }
            .keyboardShortcut("-", modifiers: [.command, .shift])
            .disabled(model == nil)
        Button("Collapse All Element Groups") { model?.run(.collapseAll) }
            .disabled(model == nil)
        Button("Reset Row Height to Default") { model?.run(.resetRowHeight) }
            .disabled(model == nil)
        // STUDIO-BEGIN row-height-menu: View ▸ Row Height, compact or regular, remembered per sequence (R36)
        Picker("Row Height", selection: Binding(get: { model?.studioRowHeight ?? "regular" }, set: { model?.setStudioRowHeight($0) })) {
            Text("Compact").tag("compact")
            Text("Regular").tag("regular")
        }
        .disabled(model?.studioRowHeight == nil)
        // STUDIO-END row-height-menu
        Divider()
        toggle("Highlight Rows With Effects", ui?.prefs.highlightRows) { model?.set(.highlightRows, $0) }
        toggle("Show Effect Info", ui?.prefs.showEffectInfo) { model?.set(.showEffectInfo, $0) }
            .keyboardShortcut("i", modifiers: .command)
        toggle("Show Hidden Rows", ui?.showHiddenRows) { model?.set(.showHiddenRows, $0) }
            .help("Show rows tagged Hidden for this session. Resets to off the next time a sequence is opened.")
        Divider()
        toggle("Full Waveform", ui?.prefs.fullWaveform) { model?.set(.fullWaveform, $0) }
        toggle("Lock Ruler Height", ui?.prefs.lockRulerHeight) { model?.set(.lockRulerHeight, $0) }
        toggle("Lock Waveform Height", ui?.prefs.lockWaveformHeight) { model?.set(.lockWaveformHeight, $0) }
        Divider()
        // STUDIO: the native toolbar is customised with View ▸ Customize Toolbar…, so Vixen's Toolbars submenu is gone
        panelItem("Effect Window", .effects, model)
        panelItem("Mark Window", .marks, model)
        // Only shows it if hidden, and it can't be here: it's always the centre (F76).
        toggle("Timeline Window", model.map { _ in true }) { _ in }
        panelItem("Effect Editor Window", .effectEditor, model)
        panelItem("Layer Editor Window", .layers, model)
        panelItem("Find Effects or Layers", .findEffects, model)
        panelItem("Color Library Window", .colorLibrary, model)
        panelItem("Gradient Library Window", .gradientLibrary, model)
        panelItem("Curve Library Window", .curveLibrary, model)
    }

    private func panelItem(_ title: String, _ panel: ShellPanel, _ model: SequencerModel?) -> some View {
        Toggle(title, isOn: Binding(
            get: { model?.panels?.visible.contains(panel) ?? false },
            set: { _ in model?.togglePanel(panel) }
        ))
        .disabled(model == nil)
    }
}

// MARK: Tools, F78 to F95

private struct ToolsMenu: View {
    let registry: SequencerRegistry

    var body: some View {
        let model = registry.ready
        let ui = model?.ui
        Menu("Audio") {
            Button("Associate Audio...") { model?.associateAudio() }
                .disabled(model == nil)
            Button("Remove Audio") { model?.removeAudio() }
                .disabled(!(ui?.removeAudioEnabled ?? false))
            #if canImport(VixenMarks)
            Button("Beat/Bar Detection") { model?.sheet = .beatsAndBars }
                .disabled(!(ui?.beatBarEnabled ?? false))
            #else
            Button("Beat/Bar Detection") {}
                .disabled(true)
            #endif
            Button("Add Onset Marks") { model?.addOnsetMarks() } // STUDIO: sequence_onset_marks beside the beat marks
                .disabled(!(ui?.removeAudioEnabled ?? false)) // STUDIO: needs the sequence's audio
            Divider()
            toggle("Speed / Tempo", (ui?.speedTempoEnabled ?? false) ? ui?.prefs.useTempoForSpeed : nil) { model?.set(.speedTempo, $0) }
        }
        Button("Create Evenly Divided Marks") { model?.openEvenMarks() }
            .disabled(model == nil)
        Button("Sequence Length...") { model?.openSequenceLength() }
            .disabled(model == nil)
        Divider()
        Button("Curve Editor") { model?.openCurveSelector() }
            .disabled(model == nil)
        Button("Color Gradient Editor") { model?.openGradientSelector() }
            .disabled(model == nil)
        Button("Color Collection Editor") { model?.openColorCollections() }
            .disabled(model == nil)
        Menu("LipSync") {
            Menu("Default Image Map") {
                ForEach(model?.lipSyncMapList ?? [], id: \.name) { map in
                    Toggle(map.name, isOn: Binding(get: { map.isDefault }, set: { _ in model?.setDefaultLipSyncMap(map.name) }))
                }
            }
            .disabled(model == nil)
            Button("Edit Image Maps") { model?.openImageMaps() }
                .disabled(model == nil || !SequencerModel.hasImageMaps)
            Button("Edit Element Face Mapping") { model?.openFaceMapping() }
                .disabled(model == nil || !SequencerModel.hasImageMaps)
            #if canImport(VixenMarks)
            Button("Lyric Converter") { model?.openLyricConverter() }
                .disabled(model == nil)
            #else
            Button("Lyric Converter") {}
                .disabled(true)
            #endif
            Button("Papagayo Import") { model?.papagayoImport() }
                .disabled(model == nil)
        }
        Button("Bulk Effects Move") { model?.openBulkMove() }
            .disabled(model == nil)
        Menu("Effect Defaults") {
            Button("Export...") { model?.exportEffectDefaults() }
                .disabled(model == nil)
            Button("Import...") { model?.importEffectDefaults() }
                .disabled(model == nil)
            Button("Dump (Readable XML)...") { model?.dumpEffectDefaults() }
                .disabled(model == nil)
        }
    }
}

// MARK: Help, F96, F97

private struct HelpItems: View {
    let registry: SequencerRegistry

    var body: some View {
        // The Help menu is shared with the Administration window, whose Online Help opens the docs' front page and whose YouTube item is the same as upstream's here.
        if registry.inFront {
            Divider()
            Button("Sequencer Online Help") {
                if let url = URL(string: "https://www.vixenlights.com/docs/usage/sequencer/") {
                    NSWorkspace.shared.open(url)
                }
            }
        }
    }
}
