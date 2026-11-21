import AppKit
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

enum PropModelSource: Sendable { case xmodel(XModelImport), vendor(PropVendorDownload) }

struct PropXModelRequest: Identifiable {
    let id = UUID()
    let source: PropModelSource
    let choices: [XModelChoice]
}

extension PropEditorModel {
    func importXModelDocument() {
        Task {
            guard await confirmReplacement() else { return }
            let panel = NSOpenPanel()
            panel.allowedContentTypes = [UTType(filenameExtension: "xmodel") ?? .xml]
            panel.allowsMultipleSelection = false
            panel.directoryURL = app?.core.map { URL(fileURLWithPath: $0.dataRoot()) }
            guard await panel.begin() == .OK, let url = panel.url else { return }
            await importXModel(path: url.path, confirm: false)
        }
    }
    func importXModel(path: String, confirm: Bool) async {
        guard !busy else { return }
        if confirm { guard await confirmReplacement() else { return } }
        busy = true
        do {
            let file = try await Task.detached { try XModelImport.open(path: path) }.value
            let choices = file.choices()
            if choices.count == 1, let first = choices.first {
                busy = false
                await importXModel(source: .xmodel(file), index: first.index)
            } else {
                xmodelRequest = PropXModelRequest(source: .xmodel(file), choices: choices)
                busy = false
            }
        } catch {
            busy = false
            await tell("Model import error", error.userMessage)
        }
    }
    func importXModel(source: PropModelSource, index: UInt32) async {
        guard !busy else { return }
        busy = true
        do {
            let imported = try await Task.detached {
                switch source {
                case .xmodel(let file): return try file.importModel(index: index)
                case .vendor(let download): return try download.importModel(index: index)
                }
            }.value
            replace(imported.editor)
            for warning in imported.warnings {
                NSLog("xModel import: %@", warning)
                if UITestHarness.isActive { UITestHarness.note("xmodel warning \(warning)") }
            }
            busy = false
        } catch {
            busy = false
            await tell("Model import error", error.userMessage)
        }
    }
}

struct PropXModelSheet: View {
    let request: PropXModelRequest
    let completion: (UInt32?) -> Void
    @State private var selected: UInt32?
    init(request: PropXModelRequest, completion: @escaping (UInt32?) -> Void) {
        self.request = request; self.completion = completion
        _selected = State(initialValue: request.choices.first(where: \.supported)?.index ?? request.choices.first?.index)
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Select xModel").font(.headline)
            Text("Select the embedded model to import.").fontWeight(.semibold)
            Text("The Custom Prop Editor can import one model at a time.")
            Table(request.choices, selection: $selected) {
                TableColumn("Model", value: \.displayName)
                TableColumn("Type") { choice in Text(choice.modelType).foregroundStyle(.secondary) }
            }.frame(minHeight: 180)
            HStack { Spacer(); Button("Cancel") { completion(nil) }.keyboardShortcut(.cancelAction); Button("Import") { completion(selected) }.keyboardShortcut(.defaultAction).disabled(selected == nil) } // STUDIO: a verb on the confirmation
        }.padding(16).frame(minWidth: 420, idealWidth: 480, minHeight: 300)
    }
}
extension XModelChoice: Identifiable { public var id: UInt32 { index } }
