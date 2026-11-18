import AppKit
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

enum Vixen2Presenter {
    static func present(core: VixenCore, path: String, over parent: NSWindow?, completion: @escaping (SequencerSession?) -> Void) {
        Task {
            do {
                let dialog = try await Task.detached { try core.vixen2Import(path: path) }.value
                let info = dialog.info()
                var audio: String?, profile: String?
                if !info.audioName.isEmpty {
                    await SchedulerAlerts.tell("Select Audio Location", "Audio File \(info.audioName) is associated with this sequence, please select the location of the audio file.", style: .information, window: parent)
                    audio = await choose(title: "Open Vixen 2.x Audio  [\(info.audioName)]", extension: nil, folder: "Audio", over: parent)
                }
                if !info.profileName.isEmpty {
                    await SchedulerAlerts.tell("Select Profile Location", "Vixen \(info.profileName).pro is associated with this sequence, please select the location of the profile.", style: .information, window: parent)
                    profile = await choose(title: "Open Vixen 2.x Profile [\(info.profileName)]", extension: "pro", folder: "Profiles", over: parent)
                }
                let audioPath = audio, profilePath = profile
                try await Task.detached { try dialog.finishInput(audio: audioPath, profile: profilePath) }.value
                show(dialog: dialog, over: parent, completion: completion)
            } catch {
                await SchedulerAlerts.tell("Import Failed", "Could not read the Vixen 2 sequence: \(error.userMessage)", style: .error, window: parent)
                completion(nil)
            }
        }
    }
    static func choose(title: String, extension ext: String?, folder: String, over parent: NSWindow?) async -> String? {
        let panel = NSOpenPanel(); panel.title = title; panel.allowsMultipleSelection = false; panel.canChooseDirectories = false
        if let ext { panel.allowedContentTypes = [UTType(filenameExtension: ext) ?? .data] }
        panel.directoryURL = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first?.appendingPathComponent("Vixen/\(folder)")
        let response = if let parent { await panel.beginSheetModal(for: parent) } else { await panel.begin() }
        return response == .OK ? panel.url?.path : nil
    }
    static func show(dialog: Vixen2Import, over parent: NSWindow?, completion: @escaping (SequencerSession?) -> Void) {
        let model = Vixen2ConverterModel(dialog: dialog)
        let window = NSWindow(contentRect: .init(x: 0, y: 0, width: 680, height: 370), styleMask: [.titled], backing: .buffered, defer: false)
        // Swift/ARC owns the window, including the standalone close path.
        window.isReleasedWhenClosed = false
        let host = NSHostingController(rootView: Vixen2ConverterView(model: model).studioAppearance()); host.sizingOptions = [] // STUDIO: follow the chosen appearance
        window.contentViewController = host; window.setContentSize(.init(width: 680, height: 370)); window.title = "Vixen 2 to Vixen 3 Converter"
        model.window = window; Vixen2ConverterModel.current = model
        model.complete = { [weak window] session in
            guard let window else { return }
            if let parent = window.sheetParent { parent.endSheet(window) } else { window.close() }
            Vixen2ConverterModel.current = nil
            Vixen2ConverterModel.lastSession = session
            completion(session)
        }
        if let parent { parent.beginSheet(window, completionHandler: nil) } else { window.center(); window.makeKeyAndOrderFront(nil) }
    }
}

@Observable
final class Vixen2ConverterModel {
    @ObservationIgnored static weak var current: Vixen2ConverterModel?
    @ObservationIgnored static var lastSession: SequencerSession?
    let dialog: Vixen2Import
    var info: Vixen2Info
    var selection: String?
    var converting = false
    var progress: UInt32 = 0
    @ObservationIgnored weak var window: NSWindow?
    @ObservationIgnored var complete: (SequencerSession?) -> Void = { _ in }
    init(dialog: Vixen2Import) { self.dialog = dialog; info = dialog.info() }
    func select(_ name: String?) {
        do { try dialog.selectMap(name: name); selection = name; info = dialog.info() }
        catch { tell(error) }
    }
    func tell(_ error: Error) { Task { await SchedulerAlerts.tell("Vixen 2 Import", error.userMessage, style: .error, window: window) } }
    func edit() {
        Vixen2MapPresenter.show(dialog: dialog.editMap(), name: selection ?? "", parent: window) { [weak self] map, name in
            guard let self else { return }
            if let map, let name {
                do { try self.dialog.saveMap(name: name, map: map) } catch { self.tell(error); return }
            }
            self.select(nil)
        }
    }
    func convert() {
        guard selection != nil, !converting else { return }
        Task {
            if dialog.missingAudio() {
                await SchedulerAlerts.tell("Couldn't find audio", "Could not locate the audio file '\(info.audioName)'; please add it manually after import (Under Tools -> Associate Audio).", style: .error, window: window)
            }
            converting = true
            let dialog = dialog
            let ticker = Task { [weak self] in
                while !Task.isCancelled { try? await Task.sleep(for: .milliseconds(100)); guard let self, !Task.isCancelled else { return }; self.progress = dialog.info().progress }
            }
            do {
                let session = try await Task.detached { try dialog.convert() }.value
                ticker.cancel(); converting = false
                // Let SwiftUI finish dismissing the progress sheet before ending its owner.
                try? await Task.sleep(for: .milliseconds(100)); complete(session)
            } catch { ticker.cancel(); converting = false; tell(error) }
        }
    }
}

private struct Vixen2ConverterView: View {
    @Bindable var model: Vixen2ConverterModel
    var body: some View {
        VStack(spacing: 16) {
            HStack(alignment: .top, spacing: 18) {
                VStack(alignment: .leading) {
                    Text("Channel Maps:")
                    List(model.info.maps, id: \.self, selection: Binding(get: { model.selection }, set: { model.select($0) })) { Text($0) }
                        .frame(width: 215, height: 210).border(.separator)
                    Button(model.selection == nil ? "Create New Map" : "Edit Selected Map") { model.edit() }
                }
                Grid(alignment: .leading, verticalSpacing: 18) {
                    GridRow { Text("Vixen 2 Sequence:"); Text(model.info.path).textSelection(.enabled).lineLimit(3).truncationMode(.middle) }
                    GridRow { Text("Vixen 2 Profile:"); Text(model.info.profilePath.isEmpty ? model.info.profileName : model.info.profilePath).textSelection(.enabled).lineLimit(3) }
                    GridRow { Text("Selected Map:"); Text(model.selection ?? "") }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }
            HStack { Spacer(); Button("Convert Sequence") { model.convert() }.disabled(model.selection == nil).keyboardShortcut(.defaultAction); Button("Cancel") { model.complete(nil) }.keyboardShortcut(.cancelAction) }
        }.padding(20).frame(minWidth: 640, minHeight: 330)
        .sheet(isPresented: $model.converting) {
            VStack(alignment: .leading, spacing: 15) { Text("Coversion Progress Form").font(.headline); ProgressView(value: Double(model.progress), total: Double(max(1, model.info.channels))) }.padding(24).frame(width: 420).interactiveDismissDisabled()
        }
    }
}

enum Vixen2MapPresenter {
    static func show(dialog: Vixen2ChannelMap, name: String, parent: NSWindow?, completion: @escaping (Vixen2ChannelMap?, String?) -> Void) {
        let model = Vixen2MapModel(dialog: dialog, name: name)
        let window = NSWindow(contentRect: .init(x: 0, y: 0, width: 1080, height: 620), styleMask: [.titled, .resizable], backing: .buffered, defer: false)
        // Swift/ARC owns the window, including the standalone close path.
        window.isReleasedWhenClosed = false
        let host = NSHostingController(rootView: Vixen2MapView(model: model).studioAppearance()); host.sizingOptions = [] // STUDIO: follow the chosen appearance
        window.contentViewController = host; window.contentMinSize = .init(width: 1000, height: 400); window.setContentSize(.init(width: 1080, height: 620)); window.title = "Vixen 2.x Channel Mapping"
        model.window = window; Vixen2MapModel.current = model
        model.complete = { [weak window, weak model] accepted in
            guard let window, let model else { return }
            if let parent = window.sheetParent { parent.endSheet(window) } else { window.close() }
            Vixen2MapModel.current = nil; completion(accepted ? dialog : nil, accepted ? model.name : nil)
        }
        if let parent { parent.beginSheet(window, completionHandler: nil) } else { window.center(); window.makeKeyAndOrderFront(nil) }
    }
}

@Observable final class Vixen2MapModel {
    @ObservationIgnored static weak var current: Vixen2MapModel?
    let dialog: Vixen2ChannelMap
    var rows: [Vixen2Row]
    var colorNames: [UInt32: String] = [:]
    var name: String
    var repeatCount = 1
    var mixing = false
    var order = "RGB"
    var selected: IndexSet = []
    var nodes: [String] = []
    var revision = 0
    var importColor: Color = .red
    @ObservationIgnored weak var window: NSWindow?
    @ObservationIgnored var complete: (Bool) -> Void = { _ in }
    @ObservationIgnored private var editingColor = false
    init(dialog: Vixen2ChannelMap, name: String) { self.dialog = dialog; self.name = name; rows = dialog.rows(); cacheColorNames() }
    func cacheColorNames() {
        for color in Set(rows.flatMap { [$0.color, $0.importColor] }) where colorNames[color] == nil { colorNames[color] = vixen2ColorName(argb: color) }
    }
    func refresh() { rows = dialog.rows(); cacheColorNames(); revision += 1 }
    func drop(start: Int, nodes: [String]) {
        finishColor()
        Task {
            guard rows.indices.contains(start) else { return }
            if rows[start].target != nil {
                guard await SchedulerAlerts.confirm("Continue", "You are about to over write existing items.  Do you wish to continue?", style: .warning, confirm: "Continue", cancel: "Cancel", window: window) else { return }
            }
            do { try dialog.applyDrop(start: UInt32(start), nodes: nodes, repeat: UInt32(repeatCount), mixing: mixing, order: order); refresh() }
            catch { await SchedulerAlerts.tell("Vixen 2 Mapping", error.userMessage, style: .error, window: window) }
        }
    }
    func clear() { finishColor(); dialog.clear(indices: selected.map(UInt32.init)); refresh() }
    func color() {
        // The original chooser is modal: a color belongs to the rows selected
        // when it opens, even if the Mac's nonmodal panel stays open longer.
        let indices = selected.map(UInt32.init)
        ColorPanelSession.shared.begin(title: "Set Import Color", argb: 0xffff0000,
            notifyUnchanged: true, onEnd: { [weak self] in self?.editingColor = false }) { [weak self] color in
                guard let self else { return }
                dialog.setColor(indices: indices, color: color); refresh()
            }
        editingColor = true
    }
    private func finishColor() {
        // A later editor may have taken the shared panel. Completing this map
        // must not finish that other owner's pending color.
        if editingColor { ColorPanelSession.shared.finish() }
    }
    func cancel() { finishColor(); complete(false) }
    func accept() {
        finishColor()
        guard !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { Task { await SchedulerAlerts.tell("Missing Name", "Please enter name of Map.", style: .error, window: window) }; return }
        complete(true)
    }
}
private struct Vixen2MapView: View {
    @Bindable var model: Vixen2MapModel
    var body: some View {
        VStack(spacing: 12) {
            HSplitView { Vixen2MapTable(model: model).frame(minWidth: 740); Vixen2MapTree(model: model).frame(minWidth: 180, idealWidth: 220, maxWidth: 280) }
            HStack { Text("Mapping Name:"); TextField("", text: $model.name).frame(minWidth: 180); Button("Set Import Color") { model.color() }; Button("Map Selected") { if let row = model.selected.first { model.drop(start: row, nodes: model.nodes) } }.disabled(model.selected.isEmpty || model.nodes.isEmpty) }
            HStack {
                Text("Repeat Elements: x"); Stepper("\(model.repeatCount)", value: $model.repeatCount, in: 1...20).fixedSize().disabled(model.mixing)
                Toggle("Color Mixing", isOn: $model.mixing).onChange(of: model.mixing) { _, value in model.repeatCount = value ? 3 : 1 }
                Picker("Color order", selection: $model.order) { ForEach(["RGB", "RBG", "BRG", "BGR", "GRB", "GBR"], id: \.self) { Text($0) } }.labelsHidden().frame(width: 90).disabled(!model.mixing)
                Spacer(); Button("Cancel") { model.cancel() }.keyboardShortcut(.cancelAction); Button("OK") { model.accept() }.keyboardShortcut(.defaultAction)
            }
        }.padding(16).frame(minWidth: 970, minHeight: 370)
    }
}
