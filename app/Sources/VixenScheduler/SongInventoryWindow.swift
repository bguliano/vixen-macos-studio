import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// The transport runs on a worker, with the same timeout and User-Agent as DownloadService.
nonisolated final class SongInventoryHTTP: SongInventoryClient, @unchecked Sendable {
    private let session: URLSession
    private let tests: Bool
    init(tests: Bool) {
        self.tests = tests
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 20; config.timeoutIntervalForResource = 20
        session = URLSession(configuration: config, delegate: RedirectGuard(tests: tests), delegateQueue: nil)
    }
    func cancel() { session.invalidateAndCancel() }
    func get(url text: String) -> SongFetch {
        guard let url = URL(string: text) else { return SongFetch(body: "", error: "Invalid URL") }
        if tests && !["127.0.0.1", "localhost", "[::1]", "::1"].contains(url.host ?? "") { return SongFetch(body: "", error: "External network is disabled in UI tests.") }
        var request = URLRequest(url: url); request.setValue("Mozilla/5.0 (compatible; Vixen/1.0)", forHTTPHeaderField: "User-Agent")
        let semaphore = DispatchSemaphore(value: 0), box = ResponseBox()
        let task = session.dataTask(with: request) { data, response, error in
            if let error { box.value = SongFetch(body: "", error: error.localizedDescription) }
            else if let response = response as? HTTPURLResponse, (200..<300).contains(response.statusCode), let data, let body = String(data: data, encoding: .utf8) ?? String(data: data, encoding: .utf16) { box.value = SongFetch(body: body, error: nil) }
            else { box.value = SongFetch(body: "", error: "The server returned an unsuccessful response.") }
            semaphore.signal()
        }
        task.resume(); semaphore.wait(); return box.value
    }
    private final class ResponseBox: @unchecked Sendable { var value = SongFetch(body: "", error: "No response") }
    private final class RedirectGuard: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
        let tests: Bool
        init(tests: Bool) { self.tests = tests }
        func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
            let allowed = !tests || ["127.0.0.1", "localhost", "[::1]", "::1"].contains(request.url?.host ?? "")
            completionHandler(allowed ? request : nil)
        }
    }
}

enum SongInventoryPresenter {
    static func present(over parent: NSWindow?, completion: @escaping (String?, String?) -> Void) {
        Task {
            while parent?.attachedSheet != nil { try? await Task.sleep(for: .milliseconds(50)) }
            let model = SongInventoryModel()
            let window = NSWindow(contentRect: .init(x: 0, y: 0, width: 800, height: 600), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
            // Swift/ARC owns the window, including the standalone close path.
            window.isReleasedWhenClosed = false
            let host = NSHostingController(rootView: SongInventoryViewContent(model: model).studioAppearance()); host.sizingOptions = [] // STUDIO: follow the chosen appearance
            window.contentViewController = host; window.contentMinSize = .init(width: 650, height: 420); window.setContentSize(.init(width: 800, height: 600)); window.title = "Song Inventory"
            model.window = window; window.delegate = model.delegate; model.delegate.model = model
            SongInventoryModel.current = model
            model.complete = { [weak window, weak model] chosen in
                guard let window, let model else { return }
                if let parent = window.sheetParent { parent.endSheet(window) } else { window.close() }
                SongInventoryModel.current = nil
                if chosen {
                    let browser = model.browser, client = model.client
                    Task {
                        do { let xml = try await Task.detached { try browser.download(client: client) }.value; SongInventoryModel.lastResult = xml; completion(xml, nil) }
                        catch { SongInventoryModel.lastResult = "error: \(error.userMessage)"; completion(nil, error.userMessage) }
                        client.cancel()
                    }
                } else { model.client.cancel(); SongInventoryModel.lastResult = "cancelled"; completion(nil, nil) }
            }
            if let parent { parent.beginSheet(window, completionHandler: nil) } else { window.center(); window.makeKeyAndOrderFront(nil) }
            model.load()
        }
    }
}

final class SongInventoryDelegate: NSObject, NSWindowDelegate {
    weak var model: SongInventoryModel?
    func windowShouldClose(_ sender: NSWindow) -> Bool { model?.finish(false); return false }
}

@Observable final class SongInventoryModel {
    @ObservationIgnored static weak var current: SongInventoryModel?
    @ObservationIgnored static var lastResult = ""
    let browser = SongInventoryBrowser()
    let client = SongInventoryHTTP(tests: UITestHarness.isActive)
    let delegate = SongInventoryDelegate()
    var view: SongInventoryView
    var loading = true
    var revision = 0
    @ObservationIgnored var closed = false
    @ObservationIgnored weak var window: NSWindow?
    @ObservationIgnored var complete: (Bool) -> Void = { _ in }
    init() { view = browser.view() }
    func refresh() { view = browser.view() }
    func load() {
        let url = UITestHarness.isActive ? ProcessInfo.processInfo.environment["VIXEN_TEST_SONG_VENDORS"] : nil
        let browser = browser, client = client
        Task {
            let errors = await Task.detached { browser.load(client: client, vendorUrl: url) }.value
            guard !closed else { return }; loading = false; refresh(); revision += 1
            for error in errors { guard !closed else { return }; await SchedulerAlerts.tell(error.title, error.message, style: .error, window: window) }
        }
    }
    func provider(_ index: UInt32) { browser.selectProvider(index: index); refresh(); revision += 1 }
    func select(_ path: String) { if browser.selectSong(path: path) { refresh() } }
    func finish(_ selected: Bool) { guard !closed else { return }; closed = true; complete(selected) }
    func open(_ link: String?) {
        guard let link, let url = URL(string: link) else { return }
        if UITestHarness.isActive { UITestHarness.note("song inventory link: \(link)") } else { NSWorkspace.shared.open(url) }
    }
}

private struct SongInventoryViewContent: View {
    @Bindable var model: SongInventoryModel
    var body: some View {
        VStack(spacing: 0) {
            HSplitView {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Provider")
                    Picker("Provider", selection: Binding(get: { model.view.provider ?? 0 }, set: { model.provider($0) })) {
                        ForEach(model.view.vendors.indices, id: \.self) { Text(model.view.vendors[$0]).tag(UInt32($0)) }
                    }.labelsHidden().disabled(model.view.vendors.isEmpty)
                    SongCategoryTree(model: model)
                    Grid(alignment: .leading) {
                        GridRow { Text("Name"); Text(model.view.vendorName) }
                        GridRow { Text("Website"); Button(model.view.vendorName) { model.open(model.view.website) }.buttonStyle(.link).disabled(model.view.website == nil) }
                    }
                }.padding(12).frame(minWidth: 200, idealWidth: 250, maxWidth: 300)
                VStack(alignment: .leading) {
                    if let song = model.view.selected {
                        TabView {
                            VStack(alignment: .leading, spacing: 22) {
                                Grid(alignment: .leading, horizontalSpacing: 20, verticalSpacing: 16) {
                                    GridRow { Text("Song:"); Button(song.title) { model.open(song.link) }.buttonStyle(.link) }
                                    GridRow { Text("Title:"); Text(song.title).textSelection(.enabled) }
                                    GridRow { Text("Artist:"); Text(song.artist).textSelection(.enabled) }
                                    GridRow { Text("Creator:"); Text(song.creator).textSelection(.enabled) }
                                }
                                Button("Select") { model.finish(true) }
                                Text("All song links and timing are provided by the contributing project and are not validated by the Vixen Team.").font(.callout).foregroundStyle(.secondary)
                                Spacer()
                            }.padding(18).tabItem { Text("Song Info") }
                        }
                    } else { Spacer() }
                }.padding(12).frame(minWidth: 360, maxWidth: .infinity)
            }
            Divider()
            HStack { if model.loading { ProgressView().controlSize(.small); Text("Loading inventories…").foregroundStyle(.secondary) }; Spacer(); Button("Cancel") { model.finish(false) }.keyboardShortcut(.cancelAction) }.padding(8)
        }.frame(minWidth: 620, minHeight: 390)
    }
}

private struct SongCategoryTree: NSViewRepresentable {
    let model: SongInventoryModel
    func makeCoordinator() -> Coordinator { Coordinator(model: model) }
    func makeNSView(context: Context) -> NSScrollView {
        let tree = SongOutlineView(), column = NSTableColumn(identifier: .init("song")); tree.addTableColumn(column); tree.outlineTableColumn = column; tree.headerView = nil; tree.rowHeight = 24
        tree.delegate = context.coordinator; tree.dataSource = context.coordinator; tree.setAccessibilityLabel("Song inventory categories")
        tree.target = context.coordinator; tree.action = #selector(Coordinator.clicked(_:))
        let scroll = NSScrollView(); scroll.documentView = tree; scroll.hasVerticalScroller = true; scroll.autohidesScrollers = true; context.coordinator.tree = tree; return scroll
    }
    func updateNSView(_ view: NSScrollView, context: Context) { let c = context.coordinator; if c.revision != model.revision { c.revision = model.revision; c.children = [:]; c.tree?.reloadData() } }
    final class Item { let row: SongInventoryRow; init(_ row: SongInventoryRow) { self.row = row } }
    final class Coordinator: NSObject, NSOutlineViewDataSource, NSOutlineViewDelegate {
        let model: SongInventoryModel
        weak var tree: NSOutlineView?
        var revision = -1
        var children: [String: [Item]] = [:]
        init(model: SongInventoryModel) { self.model = model }
        func children(_ item: Any?) -> [Item] { let key = (item as? Item)?.row.path ?? ""; if let cached = children[key] { return cached }; let rows = model.browser.treeRows(parent: key.isEmpty ? nil : key).map(Item.init); children[key] = rows; return rows }
        func outlineView(_ outlineView: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int { children(item).count }
        func outlineView(_ outlineView: NSOutlineView, child index: Int, ofItem item: Any?) -> Any { children(item)[index] }
        func outlineView(_ outlineView: NSOutlineView, isItemExpandable item: Any) -> Bool { (item as? Item)?.row.childCount ?? 0 > 0 }
        func outlineView(_ outlineView: NSOutlineView, viewFor tableColumn: NSTableColumn?, item: Any) -> NSView? {
            guard let item = item as? Item else { return nil }
            if item.row.song {
                let button = SongRowButton(title: item.row.name, target: self, action: #selector(selectSong(_:)))
                button.path = item.row.path; button.isBordered = false; button.alignment = .left
                button.contentTintColor = .linkColor; button.lineBreakMode = .byTruncatingTail
                return button
            }
            let field = NSTextField(labelWithString: item.row.name); field.lineBreakMode = .byTruncatingTail; return field
        }
        @objc func selectSong(_ sender: SongRowButton) { model.select(sender.path) }
        func outlineViewSelectionDidChange(_ notification: Notification) { guard let tree, let item = tree.item(atRow: tree.selectedRow) as? Item, item.row.song else { return }; model.select(item.row.path) }
        @objc func clicked(_ sender: NSOutlineView) { guard let item = sender.item(atRow: sender.clickedRow) as? Item, item.row.song else { return }; model.select(item.row.path) }
    }
}

private final class SongOutlineView: NSOutlineView {
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}

private final class SongRowButton: NSButton {
    var path = ""
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
}
