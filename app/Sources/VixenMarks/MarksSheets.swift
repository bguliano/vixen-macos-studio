import AppKit
import SwiftUI
import UniformTypeIdentifiers
import VixenBridge
import VixenKit

/// Whichever sheet the panel has open.
struct MarksSheetContent: View {
    let model: MarksPanelModel
    let sheet: MarksSheet

    var body: some View {
        switch sheet {
        case let .offset(row):
            OffsetTimeSheet(prompt: markOffsetPrompt()) { ms in
                model.sheet = nil
                if let ms {
                    model.offset(row, by: ms)
                }
            }
        case .importType:
            ImportTypeSheet { kind in
                if let kind {
                    Task { await model.importTypeChosen(kind) }
                } else {
                    model.sheet = nil
                }
            }
        case let .importSelection(options):
            ImportSelectionSheet(options: options) { model.importSelectionFinished($0) }
        case .pangolinColor:
            PangolinColorSheet { model.pangolinColorChosen($0) }
        case let .export(rows):
            ExportMarksSheet(rows: rows) { choice in
                if let choice {
                    Task { await model.exportChosen(format: choice.format, included: choice.included, textIncluded: choice.textIncluded) }
                } else {
                    model.sheet = nil
                }
            }
        }
    }
}

/// A sheet's title, standing in for the dialog's title bar.
struct SheetTitle: View {
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        Text(text).font(.headline)
    }
}

/// F18's `NumberDialog`: Return accepts, Escape cancels. 0 changes nothing.
struct OffsetTimeSheet: View {
    let prompt: MarkOffsetPrompt
    let onFinish: (Int32?) -> Void
    @State private var value: Int32

    init(prompt: MarkOffsetPrompt, onFinish: @escaping (Int32?) -> Void) {
        self.prompt = prompt
        self.onFinish = onFinish
        _value = State(initialValue: prompt.defaultMs)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            SheetTitle(prompt.title)
            Text(prompt.prompt)
            HStack {
                TextField("Milliseconds", value: $value, format: .number.grouping(.never))
                    .monospacedDigit()
                    .frame(width: 90)
                    .labelsHidden()
                    .onSubmit(accept)
                Stepper("Milliseconds", value: $value, in: prompt.minimumMs...prompt.maximumMs, step: 1)
                    .labelsHidden()
            }
            HStack {
                Spacer()
                Button("Cancel") { onFinish(nil) }
                    .keyboardShortcut(.cancelAction)
                Button("OK", action: accept)
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 300)
    }

    private func accept() {
        onFinish(min(max(value, prompt.minimumMs), prompt.maximumMs))
    }
}

/// F25: Import Type Selection.
struct ImportTypeSheet: View {
    let onFinish: (MarkImportKind?) -> Void
    @State private var kind: MarkImportKind = .vampBars

    static let kinds: [MarkImportKind] = [.vampBars, .vampBeats, .audacityBeats, .vixen3, .xTiming, .papagayo, .singingFaces, .pangolinBeyond]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            SheetTitle("Import Type Selection")
            Picker("Import type", selection: $kind) {
                ForEach(Self.kinds, id: \.self) { kind in
                    Text(markImportLabel(kind: kind)).tag(kind)
                }
            }
            .pickerStyle(.radioGroup)
            .labelsHidden()
            HStack {
                Spacer()
                Button("OK") { onFinish(kind) }
                    .keyboardShortcut(.defaultAction)
                Button("Cancel") { onFinish(nil) }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(20)
        .frame(width: 280)
    }
}

/// F35: Select Imported Mark Collections. Every row starts checked; Space toggles the selected row's check.
struct ImportSelectionSheet: View {
    let options: [MarkImportOption]
    let onFinish: ([UInt32]?) -> Void
    @State private var checked: [Bool]
    @State private var selection: Int?

    init(options: [MarkImportOption], onFinish: @escaping ([UInt32]?) -> Void) {
        self.options = options
        self.onFinish = onFinish
        _checked = State(initialValue: Array(repeating: true, count: options.count))
        _selection = State(initialValue: options.isEmpty ? nil : 0)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SheetTitle("Select Imported Mark Collections")
            Text("Select the imported Mark Collections to add to this sequence.")
            List(selection: $selection) {
                ForEach(options.indices, id: \.self) { i in
                    HStack(spacing: 6) {
                        Toggle(options[i].name, isOn: $checked[i])
                            .toggleStyle(.checkbox)
                            .labelsHidden()
                            .focusable(false)
                        Text(options[i].name)
                        if options[i].hasDuplicateName {
                            MarksImage(name: "caution-icon", size: 14)
                                .help(markDuplicateNameTip())
                                .accessibilityLabel(markDuplicateNameTip())
                        }
                    }
                    .tag(i)
                }
            }
            .onKeyPress(.space) {
                if let selection, checked.indices.contains(selection) {
                    checked[selection].toggle()
                }
                return .handled
            }
            HStack {
                Spacer()
                Button("Import") { // STUDIO: a verb, the import is one step on Edit ▸ Undo (R40)
                    onFinish(checked.indices.filter { checked[$0] }.map { UInt32($0) })
                }
                .keyboardShortcut(.defaultAction)
                .disabled(!checked.contains(true))
                Button("Cancel") { onFinish(nil) }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(16)
        .frame(width: 600, height: 400)
    }
}

/// F34's single collection: upstream opens its colour picker, and cancelling it cancels the import.
struct PangolinColorSheet: View {
    let onFinish: (UInt32?) -> Void
    @State private var color = Color.white

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            SheetTitle("Pangolin Beyond Import")
            ColorPicker("Color of the Beyond Marks collection", selection: $color, supportsOpacity: false)
            HStack {
                Spacer()
                Button("Cancel") { onFinish(nil) }
                    .keyboardShortcut(.cancelAction)
                Button("OK") { onFinish(NSColor(color).usingColorSpace(.sRGB)?.argb ?? 0xFFFF_FFFF) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 340)
    }
}

/// What F37's OK hands on.
struct ExportChoice {
    let format: MarkExportFormat
    let included: [String]
    let textIncluded: [String]
}

/// F37: Export Mark Collections. The table on the left, the format on the right.
struct ExportMarksSheet: View {
    let rows: [MarkExportRow]
    let onFinish: (ExportChoice?) -> Void
    @State private var included: [Bool]
    @State private var textIncluded: [Bool]
    @State private var format: MarkExportFormat = .vixen

    init(rows: [MarkExportRow], onFinish: @escaping (ExportChoice?) -> Void) {
        self.rows = rows
        self.onFinish = onFinish
        _included = State(initialValue: Array(repeating: true, count: rows.count))
        _textIncluded = State(initialValue: rows.map(\.includeText))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            SheetTitle("Export Mark Collections")
            HStack(alignment: .top, spacing: 16) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Select Mark Collections To Export").bold()
                    HStack(spacing: 0) {
                        MarksImage(name: "select-forecolor", size: 16)
                            .frame(width: MarksColumns.toggle)
                            .help("Include Collection")
                        if format == .pangolinBeyond {
                            MarksImage(name: "text-forecolor", size: 16)
                                .frame(width: MarksColumns.toggle)
                                .help("Include Mark Text")
                        }
                        Text("Collection").padding(.leading, 4)
                        Spacer()
                    }
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(rows.indices, id: \.self) { i in
                                HStack(spacing: 0) {
                                    Toggle("Include \(rows[i].name)", isOn: $included[i])
                                        .toggleStyle(.checkbox)
                                        .labelsHidden()
                                        .frame(width: MarksColumns.toggle)
                                    if format == .pangolinBeyond {
                                        Toggle("Include text of \(rows[i].name)", isOn: $textIncluded[i])
                                            .toggleStyle(.checkbox)
                                            .labelsHidden()
                                            .frame(width: MarksColumns.toggle)
                                    }
                                    Text(rows[i].name)
                                        .foregroundStyle(Color(argb: rows[i].argb))
                                        .lineLimit(1)
                                        .padding(.leading, 4)
                                    Spacer()
                                }
                                .frame(height: MarksColumns.rowHeight)
                            }
                        }
                    }
                    .background(Color.black)
                }
                .frame(maxWidth: .infinity)
                VStack(alignment: .leading, spacing: 6) {
                    Text("Export Format").bold()
                    Picker("Export Format", selection: $format) {
                        Text("Vixen").tag(MarkExportFormat.vixen)
                        Text("Audacity").tag(MarkExportFormat.audacity)
                        Text("Pangolin Beyond").tag(MarkExportFormat.pangolinBeyond)
                    }
                    .pickerStyle(.radioGroup)
                    .labelsHidden()
                }
                .frame(width: 180, alignment: .leading)
            }
            HStack {
                Spacer()
                Button("Export…") { // STUDIO: a verb; the save panel follows (R40)
                    let chosen = rows.indices.filter { included[$0] }.map { rows[$0].id }
                    let text = rows.indices.filter { textIncluded[$0] }.map { rows[$0].id }
                    onFinish(ExportChoice(format: format, included: chosen, textIncluded: text))
                }
                .keyboardShortcut(.defaultAction)
                Button("Cancel") { onFinish(nil) }
                    .keyboardShortcut(.cancelAction)
            }
        }
        .padding(16)
        .frame(width: 600, height: 400)
    }
}

// MARK: - Files

/// The open and save panels, with upstream's filter and an All Files choice. Under the UI test harness, which can't drive these panels, the
/// next path comes from a queue a script fills with the `marksNextFile` hook; an empty queue is Cancel.
enum MarkFiles {
    static var queued: [String] = []

    static func chooseOpen(filter: MarkFileFilter, directory: String?) async -> String? {
        if UITestHarness.isActive {
            return queued.isEmpty ? nil : queued.removeFirst()
        }
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        if let directory {
            panel.directoryURL = URL(fileURLWithPath: directory, isDirectory: true)
        }
        let accessory = FilterAccessory(panel: panel, filter: filter)
        panel.accessoryView = accessory.view
        panel.isAccessoryViewDisclosed = true
        let response: NSApplication.ModalResponse
        if let window = NSApp.keyWindow {
            response = await panel.beginSheetModal(for: window)
        } else {
            response = panel.runModal()
        }
        _ = accessory
        return response == .OK ? panel.url?.path : nil
    }

    static func chooseSave(filter: MarkFileFilter, directory: String?) async -> String? {
        if UITestHarness.isActive {
            return queued.isEmpty ? nil : queued.removeFirst()
        }
        let panel = NSSavePanel()
        panel.allowedContentTypes = filter.extensions.compactMap { UTType(filenameExtension: $0) }.prefix(1).map { $0 }
        panel.allowsOtherFileTypes = true
        panel.canCreateDirectories = true
        if let directory {
            panel.directoryURL = URL(fileURLWithPath: directory, isDirectory: true)
        }
        let response: NSApplication.ModalResponse
        if let window = NSApp.keyWindow {
            response = await panel.beginSheetModal(for: window)
        } else {
            response = panel.runModal()
        }
        return response == .OK ? panel.url?.path : nil
    }
}

/// The open panel's format pop-up: upstream's filter, or All Files.
final class FilterAccessory: NSObject {
    let view: NSView
    private weak var panel: NSOpenPanel?
    private let types: [UTType]

    init(panel: NSOpenPanel, filter: MarkFileFilter) {
        self.panel = panel
        types = filter.extensions.compactMap { UTType(filenameExtension: $0) }
        let popup = NSPopUpButton(frame: .zero, pullsDown: false)
        let patterns = filter.extensions.map { "*.\($0)" }.joined(separator: ";")
        popup.addItems(withTitles: [filter.description.contains("(") ? filter.description : "\(filter.description) (\(patterns))", "All Files (*.*)"])
        let label = NSTextField(labelWithString: "Format:")
        let stack = NSStackView(views: [label, popup])
        stack.orientation = .horizontal
        stack.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 12)
        view = stack
        super.init()
        popup.target = self
        popup.action = #selector(changed(_:))
        panel.allowedContentTypes = types
    }

    @objc private func changed(_ sender: NSPopUpButton) {
        panel?.allowedContentTypes = sender.indexOfSelectedItem == 0 ? types : []
    }
}
