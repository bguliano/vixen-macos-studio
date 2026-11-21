import AppKit
import SwiftUI
import VixenBridge
import VixenKit

struct PropVendorSheet: View {
    @Bindable var model: PropVendorModel
    var body: some View {
        VStack(spacing: 0) {
            Text("Vendor Inventory").font(.headline).frame(maxWidth: .infinity, alignment: .leading).padding(12)
            HSplitView {
                VStack(alignment: .leading, spacing: 10) {
                    Picker("Vendor", selection: Binding(get: { model.view.vendor ?? 0 }, set: model.selectVendor)) {
                        ForEach(model.view.vendors.indices, id: \.self) { Text(model.view.vendors[$0]).tag(UInt32($0)) }
                    }
                    PropVendorTree(model: model)
                    PropVendorImage(url: model.view.logo, client: model.client).frame(maxHeight: 100)
                    Grid(alignment: .leading) {
                        GridRow { Text("Contact"); Text(model.view.contact) }
                        GridRow { Text("Phone"); Text(model.view.phone) }
                    }
                    if !model.view.email.isEmpty {
                        Button("Email") { model.open(model.view.email.hasPrefix("mailto:") ? model.view.email : "mailto:" + model.view.email) }.buttonStyle(.link)
                    }
                    ForEach(model.view.links.indices, id: \.self) { index in
                        Button(model.view.links[index].name) { model.open(model.view.links[index].url) }.buttonStyle(.link)
                    }
                }.padding(12).frame(minWidth: 220, idealWidth: 250, maxWidth: 320).background(PropSplitStart())
                VStack(alignment: .leading, spacing: 10) {
                    if let product = model.view.product {
                        PropVendorImage(url: product.image, client: model.client).frame(minHeight: 120, idealHeight: 220, maxHeight: 250)
                        VStack(spacing: 8) {
                            Picker("Product details", selection: $model.tab) {
                                Text("Product Info").tag(0)
                                if !product.models.isEmpty { Text("Model Options").tag(1) }
                            }.pickerStyle(.segmented).labelsHidden()
                            if model.tab == 0 {
                            ScrollView {
                                Grid(alignment: .leading, horizontalSpacing: 18, verticalSpacing: 8) {
                                    GridRow { Text("Product"); Button(product.name) { model.open(product.url) }.buttonStyle(.link).disabled(product.url == nil) }
                                    info("Type", product.productType); info("Material", product.material)
                                    info("Height", product.height); info("Width", product.width); info("Thickness", product.thickness)
                                    info("Pixel Count", String(product.pixelCount)); info("Pixel Description", product.pixelDescription)
                                    info("Pixel Spacing", product.pixelSpacing); info("Notes", product.notes)
                                }.frame(maxWidth: .infinity, alignment: .leading).padding(12)
                            }
                            } else {
                                ScrollView {
                                    VStack(alignment: .leading, spacing: 18) {
                                        ForEach(product.models.indices, id: \.self) { index in
                                            HStack(alignment: .top, spacing: 16) {
                                                Button("Select") { model.selectModel(UInt32(index)) }
                                                Grid(alignment: .leading) { info("Name", product.models[index].name); info("Description", product.models[index].description) }
                                            }
                                        }
                                    }.frame(maxWidth: .infinity, alignment: .leading).padding(12)
                                }
                            }
                        }
                    } else { Spacer() }
                }.padding(12).frame(minWidth: 420, maxWidth: .infinity)
            }
            Divider()
            HStack { Spacer(); Button("Done") { model.close() }.keyboardShortcut(.cancelAction) }.padding(10) // STUDIO: the browser changes nothing until Select, so it closes with Done
        }.frame(minWidth: 740, idealWidth: 820, minHeight: 580, idealHeight: 640)
    }
    private func info(_ label: String, _ value: String) -> some View { GridRow { Text(label); Text(value).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) } }
}

private struct PropVendorImage: View {
    let url: String?
    let client: PropVendorHTTP
    @State private var image: NSImage?
    var body: some View {
        ZStack { if let image { Image(nsImage: image).resizable().scaledToFit() } }
            .frame(maxWidth: .infinity)
            .task(id: url) {
                image = nil
                guard let url else { return }
                let response = await Task.detached { client.get(url: url) }.value
                guard !Task.isCancelled, response.error == nil else { return }
                image = NSImage(data: Data(response.bytes))
            }
    }
}

private struct PropVendorTree: NSViewRepresentable {
    let model: PropVendorModel
    func makeCoordinator() -> Coordinator { Coordinator(model: model) }
    func makeNSView(context: Context) -> NSScrollView {
        let tree = Tree(), column = NSTableColumn(identifier: .init("product"))
        tree.addTableColumn(column); tree.outlineTableColumn = column; tree.headerView = nil; tree.rowHeight = 24
        tree.delegate = context.coordinator; tree.dataSource = context.coordinator; tree.setAccessibilityLabel("Vendor inventory categories")
        let scroll = NSScrollView(); scroll.documentView = tree; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true
        context.coordinator.tree = tree
        if UITestHarness.isActive { model.testExpand = { [weak tree] in tree?.expandItem(nil, expandChildren: true) } }
        return scroll
    }
    func updateNSView(_ view: NSScrollView, context: Context) {
        let c = context.coordinator
        if c.revision != model.revision { c.revision = model.revision; c.cache = [:]; c.tree?.reloadData() }
    }
    final class Tree: NSOutlineView { override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true } }
    final class Item { let row: PropVendorRow; init(_ row: PropVendorRow) { self.row = row } }
    final class Coordinator: NSObject, NSOutlineViewDataSource, NSOutlineViewDelegate {
        let model: PropVendorModel
        var revision = -1
        var cache: [String: [Item]] = [:]
        weak var tree: NSOutlineView?
        init(model: PropVendorModel) { self.model = model }
        func children(_ item: Any?) -> [Item] {
            let key = (item as? Item)?.row.path ?? ""
            if let rows = cache[key] { return rows }
            let rows = model.browser.treeRows(parent: key.isEmpty ? nil : key).map(Item.init); cache[key] = rows; return rows
        }
        func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int { children(item).count }
        func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any { children(item)[index] }
        func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool { (item as? Item)?.row.childCount ?? 0 > 0 }
        func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
            guard let item = item as? Item else { return nil }
            let field = NSTextField(labelWithString: item.row.name); field.lineBreakMode = .byTruncatingTail; return field
        }
        func outlineViewSelectionDidChange(_ notification: Notification) {
            guard let tree, let item = tree.item(atRow: tree.selectedRow) as? Item, item.row.product else { return }
            model.selectProduct(item.row.path)
        }
    }
}
