import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// A column's 12pt centred heading over its divider (F2).
struct SetupPanelHeader: View {
    let title: String

    var body: some View {
        VStack(spacing: 4) {
            Text(title)
                .font(.system(size: 12, weight: .semibold))
                .frame(maxWidth: .infinity)
            Divider()
        }
    }
}

/// An icon button whose tooltip and accessibility label are upstream's tooltip, so scripts and VoiceOver reach it by that.
struct SetupIconButton: View {
    let symbol: String
    let label: String
    var enabled = true
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol)
                .frame(width: 18, height: 18)
        }
        .disabled(!enabled)
        .help(label)
        .accessibilityLabel(label)
    }
}

// MARK: Elements

private final class ElementTreeProvider: OutlineDataProvider {
    unowned let model: SetupModel

    init(model: SetupModel) {
        self.model = model
    }

    func outlineChildren(of parent: String?) -> [OutlineRow] {
        model.session?.elementRows(parent: parent).map(\.outlineRow) ?? []
    }

    func outlineChildrenAsync(of parent: String?) async -> [OutlineRow] {
        guard let session = model.session else { return [] }
        return await offMain { session.elementRows(parent: parent).map(\.outlineRow) }
    }

    func outlineCanDrop(items: [String], parent: String?, index: Int?, copy: Bool) -> Bool {
        model.canDrop(items: items, parent: parent, index: index, copy: copy)
    }

    func outlineDrop(items: [String], parent: String?, index: Int?, copy: Bool) {
        let model = model
        Task { await model.drop(items: items, parent: parent, index: index, copy: copy) }
    }

    func outlineRows(ids: [String]) async -> [OutlineRow?] {
        guard let session = model.session else { return [] }
        return await offMain { session.elementRowsAt(paths: ids).map { $0?.outlineRow } }
    }

    func outlineIcons(ids: [String]) -> [OutlineRowIcon?] {
        model.session?.tryElementIcons(paths: ids).map(\.picture) ?? []
    }
}

/// The tree alone, so nothing else in the panel makes SwiftUI look at it again.
struct ElementOutline: View { // STUDIO: shared with StudioElementsPanel
    let model: SetupModel
    @State private var provider: ElementTreeProvider?

    var body: some View {
        Group {
            if let provider {
                OutlineView(
                    provider: provider,
                    handle: model.elementTree,
                    allowsDragging: true,
                    accessibilityLabel: "Elements",
                    rangeOrder: { selected, anchor, endpoint in setupOrderedRows(selected: selected, anchor: anchor, endpoint: endpoint) },
                    onSelectionChange: { model.elementSelectionChanged($0) },
                    onCommand: { command in
                        Task {
                            switch command {
                            case .copy: await model.copyElements()
                            case .paste: await model.paste(asNew: false)
                            case .pasteAsNew: await model.paste(asNew: true)
                            case .delete: await model.deleteElements()
                            }
                        }
                    },
                    canPerform: { command in
                        switch command {
                        case .copy, .delete: model.elementCount > 0
                        case .paste, .pasteAsNew: true
                        }
                    },
                    menu: { model.elementMenu() }
                )
            } else {
                Color.clear
            }
        }
        .onAppear {
            if provider == nil {
                provider = ElementTreeProvider(model: model)
            }
        }
    }
}

/// The left column (F9 to F20).
struct ElementsPanel: View {
    @Bindable var model: SetupModel

    var body: some View {
        VStack(spacing: 6) {
            SetupPanelHeader(title: "Elements")
            HStack(spacing: 6) {
                Text("Add:")
                Picker("Add:", selection: $model.templateName) {
                    ForEach(model.templates, id: \.name) { Text($0.name).tag($0.name) }
                }
                .labelsHidden()
                SetupIconButton(symbol: "plus", label: "Add Elements", enabled: model.selectedTemplate != nil) {
                    Task { await model.addFromTemplate() }
                }
            }
            ElementOutline(model: model)
                .frame(minHeight: 120)
                .background(Color(nsColor: .textBackgroundColor).opacity(0.25))
                .clipShape(RoundedRectangle(cornerRadius: 4))
            HStack(spacing: 6) {
                SetupIconButton(symbol: "trash", label: "Delete Elements", enabled: model.elementCount > 0) {
                    Task { await model.deleteElements() }
                }
                SetupIconButton(symbol: "pencil", label: "Rename Elements", enabled: model.elementCount > 0) {
                    Task { await model.renameElements() }
                }
                SetupIconButton(symbol: "text.magnifyingglass", label: "Find outputs these elements are patched to", enabled: model.elementCount > 0) {
                    Task { await model.findOutputs() }
                }
                Spacer(minLength: 0)
            }
            GroupBox("Selected Item(s):") {
                VStack(alignment: .leading, spacing: 6) {
                    HStack(spacing: 6) {
                        Text("Configure:")
                        Picker("Configure:", selection: $model.helperName) {
                            ForEach(model.helperNames, id: \.self) { Text($0).tag($0) }
                        }
                        .labelsHidden()
                        SetupIconButton(symbol: "gearshape.2", label: "Configure", enabled: model.elementCount > 0) {
                            Task { await model.runHelper() }
                        }
                    }
                    Text("Properties:")
                    List(model.properties, id: \.typeId, selection: $model.propertySelection) { row in
                        Text(row.typeName)
                            .lineLimit(1)
                    }
                    .listStyle(.plain)
                    .frame(minHeight: 70, maxHeight: 96)
                    .clipShape(RoundedRectangle(cornerRadius: 4))
                    .contextMenu(forSelectionType: String.self) { _ in
                    } primaryAction: { _ in
                        Task { await model.configureProperty() }
                    }
                    // F16: under the list, Add, Delete, Configure.
                    HStack(spacing: 4) {
                        SetupIconButton(symbol: "plus", label: "Add Property", enabled: model.elementCount > 0) {
                            Task { await model.addProperty() }
                        }
                        SetupIconButton(symbol: "minus", label: "Delete Property", enabled: !model.properties.isEmpty && !model.propertySelection.isEmpty) {
                            Task { await model.deleteProperties() }
                        }
                        SetupIconButton(symbol: "gearshape", label: "Configure Property", enabled: !model.properties.isEmpty && model.propertySelection.count == 1) {
                            Task { await model.configureProperty() }
                        }
                        Spacer()
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(8)
    }
}

// MARK: Controllers

private final class ControllerTreeProvider: OutlineDataProvider {
    unowned let model: SetupModel

    init(model: SetupModel) {
        self.model = model
    }

    func outlineChildren(of parent: String?) -> [OutlineRow] {
        guard let session = model.session else { return [] }
        let rows = parent.map { session.controllerChildren(parent: $0) } ?? session.controllerRows()
        return rows.map(\.outlineRow)
    }

    func outlineChildrenAsync(of parent: String?) async -> [OutlineRow] {
        guard let session = model.session else { return [] }
        let rows = await offMain { parent.map { session.controllerChildren(parent: $0) } ?? session.controllerRows() }
        return rows.map(\.outlineRow)
    }

    /// F79: root rows among root rows, and never on a row.
    func outlineCanDrop(items: [String], parent: String?, index: Int?, copy: Bool) -> Bool {
        parent == nil && index != nil && !copy && items.allSatisfy { $0.hasPrefix("c") }
    }

    func outlineDrop(items: [String], parent: String?, index: Int?, copy: Bool) {
        guard let index else { return }
        let model = model
        Task { await model.dropControllers(items: items, index: index) }
    }

    func outlineRows(ids: [String]) async -> [OutlineRow?] {
        guard let session = model.session else { return [] }
        let roots = await offMain { session.controllerRows() }
        let byID = Dictionary(uniqueKeysWithValues: roots.map { ($0.id, $0) })
        return ids.map { byID[$0]?.outlineRow }
    }

    func outlineIcons(ids: [String]) -> [OutlineRowIcon?] {
        model.session?.tryControllerIcons(rows: ids).map(\.picture) ?? []
    }
}

struct ControllerOutline: View { // STUDIO: shared with StudioControllersPanel
    let model: SetupModel
    @State private var provider: ControllerTreeProvider?

    var body: some View {
        Group {
            if let provider {
                OutlineView(
                    provider: provider,
                    handle: model.controllerTree,
                    allowsDragging: true,
                    syncChildLimit: Int(setupOutputPageSize()),
                    keepsHiddenSelection: true,
                    accessibilityLabel: "Controllers",
                    rangeOrder: { selected, anchor, endpoint in setupOrderedRows(selected: selected, anchor: anchor, endpoint: endpoint) },
                    onSelectionChange: { model.controllerSelectionChanged($0) },
                    // F82: on any row with a controller selected, and it doesn't expand or collapse.
                    onDoubleClick: { _ in
                        Task { await model.configureController() }
                    },
                    onCommand: { command in
                        // F83: Delete only. No copy or paste here.
                        if command == .delete {
                            Task { await model.deleteControllers() }
                        }
                    },
                    canPerform: { $0 == .delete },
                    menu: { model.controllerMenu() }
                )
            } else {
                Color.clear
            }
        }
        .onAppear {
            if provider == nil {
                provider = ControllerTreeProvider(model: model)
            }
        }
    }
}

/// The right column (F76 to F91), with the window's OK, Cancel and Help under it.
struct ControllersPanel: View {
    @Bindable var model: SetupModel

    var body: some View {
        VStack(spacing: 6) {
            SetupPanelHeader(title: "Controllers")
            HStack(spacing: 6) {
                Text("Add:")
                Picker("Add:", selection: $model.controllerTypeID) {
                    ForEach(model.controllerTypes, id: \.id) { Text($0.name).tag($0.id) }
                }
                .labelsHidden()
                SetupIconButton(symbol: "plus", label: "Add", enabled: !model.controllerTypeID.isEmpty) {
                    Task { await model.addController() }
                }
                SetupIconButton(symbol: "arrow.up", label: "Move Controller(s) up", enabled: model.controllerPanel.anyController) {
                    Task { await model.moveControllers(up: true) }
                }
                SetupIconButton(symbol: "arrow.down", label: "Move Controller(s) down", enabled: model.controllerPanel.anyController) {
                    Task { await model.moveControllers(up: false) }
                }
            }
            ControllerOutline(model: model)
                .frame(minHeight: 120)
                .background(Color(nsColor: .textBackgroundColor).opacity(0.25))
                .clipShape(RoundedRectangle(cornerRadius: 4))
            HStack(spacing: 6) {
                SetupIconButton(symbol: "text.magnifyingglass", label: "Find elements patched to these outputs", enabled: model.controllerPanel.canFindElements) {
                    Task { await model.findElements() }
                }
                Spacer(minLength: 0)
            }
            GroupBox("Selected Controller") {
                VStack(alignment: .leading, spacing: 6) {
                    Grid(alignment: .leading, verticalSpacing: 4) {
                        GridRow {
                            Text("Type:")
                            Text(model.controllerPanel.typeLabel)
                                .lineLimit(1)
                                .truncationMode(.tail)
                        }
                        GridRow {
                            Text("Outputs:")
                            Text(model.controllerPanel.outputsLabel)
                                .monospacedDigit()
                        }
                    }
                    HStack(spacing: 6) {
                        SetupIconButton(symbol: "gearshape", label: "Configure", enabled: model.controllerPanel.oneController) {
                            Task { await model.configureController() }
                        }
                        SetupIconButton(symbol: "number", label: "Channel Count", enabled: model.controllerPanel.oneController) {
                            Task { await model.channelCount() }
                        }
                        SetupIconButton(symbol: "pencil", label: "Rename", enabled: model.controllerPanel.oneController) {
                            Task { await model.renameController() }
                        }
                        SetupIconButton(symbol: "trash", label: "Delete", enabled: model.controllerPanel.anyController) {
                            Task { await model.deleteControllers() }
                        }
                        SetupIconButton(symbol: "play.fill", label: "Start / Enable", enabled: model.controllerPanel.canStart) {
                            Task { await model.startControllers() }
                        }
                        SetupIconButton(symbol: "stop.fill", label: "Stop / Disable", enabled: model.controllerPanel.canStop) {
                            Task { await model.stopControllers() }
                        }
                        Spacer(minLength: 0)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            HStack(spacing: 8) {
                Spacer(minLength: 0)
                Button("OK") { model.ok() }
                    .keyboardShortcut(.defaultAction)
                Button("Cancel") { model.cancel() }
                    .keyboardShortcut(.cancelAction)
                Button {
                    model.help()
                } label: {
                    Label("Help", systemImage: "questionmark.circle")
                }
            }
            .controlSize(.regular)
        }
        .padding(8)
    }
}

// MARK: Simple patching

/// The centre column's default view (F61 to F70).
struct SimplePatchingView: View {
    @Bindable var model: SetupModel

    var body: some View {
        ScrollView {
            VStack(spacing: 10) {
                HStack(alignment: .top, spacing: 10) {
                    elementCounts
                    controllerCounts
                }
                patchingOptions
            }
            .padding(8)
            .frame(maxWidth: .infinity)
        }
    }

    private func countRow(_ label: String, _ value: UInt32, bold: Bool = false, help: String) -> some View {
        GridRow {
            Text(label)
                .fontWeight(bold ? .bold : .regular)
                .help(help)
            Text("\(value)")
                .fontWeight(bold ? .bold : .regular)
                .monospacedDigit()
                .frame(minWidth: 44, alignment: .trailing)
                .help(help)
        }
    }

    private var elementCounts: some View {
        GroupBox("Selected Elements") {
            VStack(spacing: 12) {
                Grid(alignment: .leading, verticalSpacing: 3) {
                    countRow("Total Patch Points:", model.elementStats.total, bold: true, help: "The total number of Patch Points connected to the selected elements.  Patch Points are the outputs from any element or filter, but before it gets to the controllers.")
                    countRow("Connected:", model.elementStats.connected, help: "The number of Patch Points connected to the selected elements that are being patched to controller outputs.")
                    countRow("Unconnected:", model.elementStats.unconnected, help: "The number of Patch Points connected to the selected elements that have nothing else connected.")
                    Color.clear.frame(height: 12).gridCellUnsizedAxes(.horizontal)
                    countRow("Items:", model.elementStats.items, help: "The number of items selected in the element view.")
                    countRow("Groups:", model.elementStats.groups, help: "The number of groups found from the selected elements.")
                    countRow("Elements:", model.elementStats.leaves, help: "The number of elements found in (or descending from) the selected elements.")
                    countRow("Filters:", model.elementStats.filters, help: "The number of filters found in the patching connections from the selected elements.")
                }
                Button("Unpatch Elements") {
                    Task { await model.unpatchElements() }
                }
                .disabled(model.elementStats.connected == 0)
            }
            .frame(maxWidth: .infinity)
        }
    }

    private var controllerCounts: some View {
        GroupBox("Selected Controllers") {
            VStack(spacing: 12) {
                Grid(alignment: .leading, verticalSpacing: 3) {
                    countRow("Outputs:", model.controllerStats.outputs, bold: true, help: "The total number of controller outputs selected.")
                    countRow("Patched:", model.controllerStats.patched, help: "The number of controller outputs selected that are already connected to something.")
                    countRow("Unpatched:", model.controllerStats.unpatched, help: "The number of controller outputs selected that are not connected to anything.")
                    Color.clear.frame(height: 12).gridCellUnsizedAxes(.horizontal)
                    countRow("Controllers:", model.controllerStats.controllers, help: "The number of controllers (or part thereof) selected.")
                    Color.clear.frame(height: 12).gridCellUnsizedAxes(.horizontal)
                    GridRow {
                        Text("First output:")
                            .help("The first output in the list of selected outputs (as will be used for patching).")
                        Text(model.controllerStats.firstLabel)
                            .fixedSize(horizontal: false, vertical: true)
                            .help("The first output in the list of selected outputs (as will be used for patching).")
                    }
                    GridRow {
                        Text("Last output:")
                            .help("The last output in the list of selected outputs (as will be used for patching).")
                        Text(model.controllerStats.lastLabel)
                            .fixedSize(horizontal: false, vertical: true)
                            .help("The last output in the list of selected outputs (as will be used for patching).")
                    }
                }
                Button("Unpatch Controllers") {
                    Task { await model.unpatchControllers() }
                }
                .disabled(model.controllerStats.patched == 0)
            }
            .frame(maxWidth: .infinity)
        }
    }

    private var patchingOptions: some View {
        GroupBox("Patching Options") {
            VStack(spacing: 10) {
                HStack(alignment: .top, spacing: 10) {
                    GroupBox {
                        VStack(alignment: .leading, spacing: 6) {
                            Picker("Patch points", selection: $model.usesAllPoints) {
                                Text("Use unconnected patch points only").tag(false)
                                Text("Use all available patch points").tag(true)
                            }
                            .pickerStyle(.radioGroup)
                            .labelsHidden()
                            Toggle("Reverse Element Order", isOn: $model.reversesElements)
                                .toggleStyle(.checkbox)
                                .help("The order in which Elements will be patched to controller outputs. This does not effect the order in which color channels are patched to controller outputs.")
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    GroupBox {
                        VStack(alignment: .leading, spacing: 6) {
                            Picker("Outputs", selection: $model.usesAllOutputs) {
                                Text("Only use unpatched outputs").tag(false)
                                Text("Use all selected controller outputs").tag(true)
                            }
                            .pickerStyle(.radioGroup)
                            .labelsHidden()
                            Toggle("Reverse order of selected outputs", isOn: $model.reversesOutputs)
                                .toggleStyle(.checkbox)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                Text(model.summary.text)
                    .monospacedDigit()
                    .multilineTextAlignment(.center)
                Text(model.summary.warning.isEmpty ? " " : model.summary.warning)
                    .foregroundStyle(Color(red: 1, green: 1, blue: 0))
                    .multilineTextAlignment(.center)
                Button("Patch Elements") {
                    Task { await model.patchElements() }
                }
                .font(.system(size: 12))
                .disabled(!model.summary.canPatch)
            }
            .frame(maxWidth: .infinity)
        }
    }
}

// MARK: Menus

/// A menu item's closure. The item keeps it alive as its target.
private final class SetupMenuAction: NSObject {
    let handler: () -> Void

    init(_ handler: @escaping () -> Void) {
        self.handler = handler
    }

    @objc func fire() {
        handler()
    }
}

/// A menu item that runs a closure.
func SetupMenuItem(_ title: String, enabled: Bool = true, tooltip: String? = nil, handler: (() -> Void)? = nil) -> NSMenuItem {
    let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
    if let handler {
        let action = SetupMenuAction(handler)
        item.target = action
        item.action = #selector(SetupMenuAction.fire)
        item.representedObject = action
    }
    item.isEnabled = enabled
    item.toolTip = tooltip
    return item
}

private func tagDot(_ argb: UInt32) -> NSImage {
    NSImage(size: NSSize(width: 10, height: 10), flipped: false) { rect in
        NSColor(argb: argb).setFill()
        NSBezierPath(ovalIn: rect.insetBy(dx: 1, dy: 1)).fill()
        return true
    }
}

extension SetupModel {
    /// F32, F33, F54, in `Items.AddRange` order.
    func elementMenu() -> NSMenu? {
        guard let (state, tags) = elementMenuState() else { return nil }
        let run: (@escaping (SetupModel) async -> Void) -> (() -> Void) = { work in
            { [weak self] in
                guard let self else { return }
                Task { await work(self) }
            }
        }
        let menu = NSMenu()
        menu.autoenablesItems = false
        menu.addItem(SetupMenuItem("Cut", enabled: false))
        menu.addItem(SetupMenuItem("Copy", enabled: state.copy, handler: run { await $0.copyElements() }))
        menu.addItem(SetupMenuItem("Paste", enabled: state.paste, handler: run { await $0.paste(asNew: false) }))
        menu.addItem(SetupMenuItem("Paste as New", enabled: state.pasteAsNew, handler: run { await $0.paste(asNew: true) }))
        menu.addItem(.separator())
        let properties = NSMenu()
        properties.autoenablesItems = false
        properties.addItem(SetupMenuItem("Copy properties", enabled: state.copyProperties, handler: run { await $0.copyProperties() }))
        properties.addItem(SetupMenuItem("Paste properties", enabled: state.pasteProperties, handler: run { await $0.pasteProperties() }))
        let propertiesItem = SetupMenuItem("Item properties", enabled: state.itemProperties)
        propertiesItem.submenu = properties
        menu.addItem(propertiesItem)
        menu.addItem(.separator())
        menu.addItem(SetupMenuItem("Add", handler: run { await $0.addElement() }))
        menu.addItem(SetupMenuItem("Add Multiple", handler: run { await $0.addMultiple() }))
        menu.addItem(SetupMenuItem("Delete", enabled: state.delete, handler: run { await $0.deleteElements() }))
        menu.addItem(SetupMenuItem("Create Group", enabled: state.createGroup, handler: run { await $0.createGroup() }))
        menu.addItem(SetupMenuItem("Rename", enabled: state.rename, handler: run { await $0.renameElements() }))
        menu.addItem(SetupMenuItem("Find/Replace", enabled: state.findReplace, handler: run { await $0.findReplace() }))
        menu.addItem(SetupMenuItem("Collapse All", handler: run { $0.collapseAll() }))
        menu.addItem(SetupMenuItem("Reverse Elements", enabled: state.reverse, handler: run { await $0.reverseElements() }))
        menu.addItem(SetupMenuItem("Sort", enabled: state.sort, handler: run { await $0.sortElements() }))
        let wire = NSMenu()
        wire.autoenablesItems = false
        wire.addItem(SetupMenuItem("Front Wire Diagram", handler: run { await $0.exportWireDiagram(flip: false) }))
        wire.addItem(SetupMenuItem("Back Wire Diagram", handler: run { await $0.exportWireDiagram(flip: true) }))
        let wireItem = SetupMenuItem("Export Wire Diagram", enabled: state.exportWireDiagram)
        wireItem.submenu = wire
        menu.addItem(wireItem)
        menu.addItem(SetupMenuItem("Export Element Tree", enabled: state.exportElementTree, tooltip: "Export the element tree for importing into other profiles.", handler: run { await $0.exportElementTree() }))
        menu.addItem(.separator())
        // STUDIO-BEGIN patch-by-intent: Reverse, Swap With and Move to Output on one element's patched outputs, in the live Display pane
        if let patch = studioPatchMenuItem() {
            menu.addItem(patch)
            menu.addItem(.separator())
        }
        // STUDIO-END patch-by-intent

        let tagMenu = NSMenu()
        tagMenu.autoenablesItems = false
        tagMenu.addItem(SetupMenuItem("⌘-Click to Cascade Tag", enabled: false))
        tagMenu.addItem(.separator())
        for tag in tags {
            let id = tag.id
            let item = SetupMenuItem(tag.name) { [weak self] in
                let cascade = NSEvent.modifierFlags.contains(.command)
                guard let self else { return }
                Task { await self.toggleTag(id, cascade: cascade) }
            }
            switch tag.check {
            case .off: item.state = .off
            case .on: item.state = .on
            case .mixed: item.state = .mixed
            }
            if let color = tag.color {
                item.image = tagDot(color)
            }
            tagMenu.addItem(item)
        }
        tagMenu.addItem(.separator())
        tagMenu.addItem(SetupMenuItem("Manage Tags...", handler: run { await $0.manageTags() }))
        let tagsItem = SetupMenuItem("Tags", enabled: state.tags)
        tagsItem.submenu = tagMenu
        menu.addItem(tagsItem)
        return menu
    }

    /// F92, F93.
    func controllerMenu() -> NSMenu? {
        // F93 with F81: rows hidden by a collapse are still selected, but with nothing highlighted there's no menu.
        guard let session, !controllerHighlighted.isEmpty else { return nil }
        let state = session.controllerMenuState(rows: controllerSelection)
        guard state.show else { return nil }
        let run: (@escaping (SetupModel) async -> Void) -> (() -> Void) = { work in
            { [weak self] in
                guard let self else { return }
                Task { await work(self) }
            }
        }
        let menu = NSMenu()
        menu.autoenablesItems = false
        if state.controllerItems {
            menu.addItem(SetupMenuItem("Configure", enabled: state.configure, handler: run { await $0.configureController() }))
            menu.addItem(SetupMenuItem("Channel Count", enabled: state.channelCount, handler: run { await $0.channelCount() }))
            menu.addItem(SetupMenuItem("Rename", enabled: state.rename, handler: run { await $0.renameController() }))
            menu.addItem(SetupMenuItem("Delete", enabled: state.delete, handler: run { await $0.deleteControllers() }))
            menu.addItem(SetupMenuItem("Start / Enable", enabled: state.start, handler: run { await $0.startControllers() }))
            menu.addItem(SetupMenuItem("Stop / Disable", enabled: state.stop, handler: run { await $0.stopControllers() }))
            menu.addItem(.separator())
            menu.addItem(SetupMenuItem(state.unpatchControllersTitle, enabled: state.unpatchControllers, handler: run { await $0.unpatchControllers() }))
        } else {
            if state.insertChannels {
                menu.addItem(SetupMenuItem("Insert Channels", handler: run { await $0.insertChannels() }))
            }
            menu.addItem(SetupMenuItem("Remove Channels", handler: run { await $0.removeChannels() }))
            menu.addItem(SetupMenuItem(state.unpatchChannelsTitle, enabled: state.unpatchChannels, handler: run { await $0.unpatchControllers() }))
            menu.addItem(SetupMenuItem(state.findElementsTitle, enabled: state.findElements, handler: run { await $0.findElements() }))
        }
        return menu
    }
}
