import AppKit
import SwiftUI

/// A picture before a row's title.
nonisolated public enum OutlineRowIcon: Hashable, Sendable {
    case folder
    case ball(UInt32)
}

/// One row of an `OutlineView`. Rows are identified by a stable string, normally a node's id, or a path where one node can appear in several places.
nonisolated public struct OutlineRow: Hashable, Sendable {
    public var id: String
    public var title: String
    public var hasChildren: Bool
    /// Small coloured dot before the title, the way trees show a patched or unpatched state.
    public var statusColor: UInt32?
    public var isDimmed: Bool
    public var icon: OutlineRowIcon?
    /// Small coloured dots after the title, such as element tags.
    public var trailingDots: [UInt32]
    /// How many children it has, when that's known. A row over `OutlineView.syncChildLimit` fetches its children off the main thread before expanding.
    public var childCount: Int?

    public init(
        id: String,
        title: String,
        hasChildren: Bool,
        statusColor: UInt32? = nil,
        isDimmed: Bool = false,
        icon: OutlineRowIcon? = nil,
        trailingDots: [UInt32] = [],
        childCount: Int? = nil
    ) {
        self.id = id
        self.title = title
        self.hasChildren = hasChildren
        self.statusColor = statusColor
        self.isDimmed = isDimmed
        self.icon = icon
        self.trailingDots = trailingDots
        self.childCount = childCount
    }
}

/// Answers the tree's questions on demand, so only rows actually on screen are ever fetched. A tree of 50,000
/// nodes costs the same to show as a tree of 50.
@MainActor
public protocol OutlineDataProvider: AnyObject {
    /// The children of `parent`, or the roots when `parent` is nil.
    func outlineChildren(of parent: String?) -> [OutlineRow]
    /// Items dropped onto `target` at `index`. Return whether the move was accepted.
    func outlineMove(items: [String], to target: String?, index: Int, copy: Bool) -> Bool
    /// Whether these items may be dropped there.
    func outlineCanMove(items: [String], to target: String?) -> Bool
    /// A title edited in place.
    func outlineRename(item: String, to title: String)
    /// `outlineChildren`, for a parent big enough that fetching on the main thread would show. Fetch on a background task here.
    func outlineChildrenAsync(of parent: String?) async -> [OutlineRow]
    /// Whether a drop is allowed at this exact spot: `index` nil means on the row `parent`, otherwise a child position under `parent`. Asked once per spot during a drag.
    func outlineCanDrop(items: [String], parent: String?, index: Int?, copy: Bool) -> Bool
    /// A drop at this exact spot. The provider tells the handle what to reload afterwards.
    func outlineDrop(items: [String], parent: String?, index: Int?, copy: Bool)
    /// New data for these rows. Nil where a row has gone. Fetch on a background task here.
    func outlineRows(ids: [String]) async -> [OutlineRow?]
    /// Fresh icons for rows on screen, in order. Empty if they can't be had right now: an edit's own refresh will come.
    func outlineIcons(ids: [String]) -> [OutlineRowIcon?]
}

extension OutlineDataProvider {
    public func outlineMove(items: [String], to target: String?, index: Int, copy: Bool) -> Bool { false }
    public func outlineCanMove(items: [String], to target: String?) -> Bool { false }
    public func outlineRename(item: String, to title: String) {}
    public func outlineChildrenAsync(of parent: String?) async -> [OutlineRow] { outlineChildren(of: parent) }
    public func outlineCanDrop(items: [String], parent: String?, index: Int?, copy: Bool) -> Bool { outlineCanMove(items: items, to: parent) }
    public func outlineDrop(items: [String], parent: String?, index: Int?, copy: Bool) {
        _ = outlineMove(items: items, to: parent, index: index ?? -1, copy: copy)
    }
    public func outlineRows(ids: [String]) async -> [OutlineRow?] { [] }
    public func outlineIcons(ids: [String]) -> [OutlineRowIcon?] { [] }
}

/// The selected rows, in selection order: display order, except that a range made with ⇧ runs from its anchor to its endpoint, which is what patching order follows.
nonisolated public struct OutlineSelection: Sendable, Equatable {
    public var ids: [String]
    /// The row last acted on, which stays even if that action was to ⌘-click it off. Nil when nothing is selected.
    public var primary: String?
    /// For a tree that keeps a collapse's rows selected: those of `ids` that aren't on screen, so aren't highlighted.
    public var hidden: [String]

    public init(ids: [String] = [], primary: String? = nil, hidden: [String] = []) {
        self.ids = ids
        self.primary = primary
        self.hidden = hidden
    }
}

/// Commands the tree responds to while it has the focus, as the Edit menu and the keyboard send them.
nonisolated public enum OutlineCommand: Sendable {
    case copy
    case paste
    case pasteAsNew
    case delete
}

/// What a window keeps to tell its tree what to do: reload exactly what changed, select from code, and read what's on screen.
@MainActor
public final class OutlineHandle {
    fileprivate weak var coordinator: OutlineView.Coordinator?
    /// Set once `loadRoots` has been asked for, so a tree that arrives later, after that call gave up waiting or with no
    /// loaded tree before it, still fetches its rows.
    private var wantsRoots = false

    public init() {}

    public var selection: OutlineSelection {
        coordinator?.currentSelection() ?? OutlineSelection()
    }

    /// The first fetch, off the main thread. A tree with a handle shows nothing until this has run.
    public func loadRoots() async {
        wantsRoots = true
        // The view may only just be on its way, if SwiftUI is building it from the same change that asked for this.
        for _ in 0..<500 where coordinator == nil {
            try? await Task.sleep(for: .milliseconds(2))
        }
        // A tree being replaced can still be the one attached when this starts; if its successor took over during the
        // fetch, that one loads too, so the rows end up in the tree on screen.
        for _ in 0..<4 {
            guard let target = coordinator else { return }
            await target.loadRoots()
            if coordinator === target { return }
        }
    }

    /// A new tree takes over. Once roots have been asked for, `loadRoots` may already have gone to the tree it replaces, or
    /// given up before any tree arrived, so the new tree loads its own.
    fileprivate func attach(_ next: OutlineView.Coordinator) {
        guard coordinator !== next else { return }
        coordinator = next
        if wantsRoots, !next.hasRoots {
            Task { await next.loadRoots() }
        }
    }

    /// A tree leaving lets go, so calls made before its successor arrives wait for that one.
    fileprivate func detach(_ leaving: OutlineView.Coordinator) {
        if coordinator === leaving {
            coordinator = nil
        }
    }

    /// Fetches again the children of the roots if `roots`, and of every loaded row that `parents` says yes to, keeping expansion, scroll position and whatever is still selected.
    public func reload(roots: Bool, parents: (String) -> Bool) async {
        await coordinator?.reload(roots: roots, parents: parents)
    }

    /// New row data for every loaded row `rows` says yes to, with no change to what's under them.
    public func refresh(rows: (String) -> Bool) async {
        await coordinator?.refresh(rows: rows)
    }

    /// The selection in the order the rows are shown, which is what most commands act in. The selection order is what patching follows.
    public var selectionInDisplayOrder: [String] {
        coordinator?.selectionInDisplayOrder() ?? []
    }

    /// Scrolls just enough to show a row.
    public func reveal(_ id: String) {
        coordinator?.reveal(id)
    }

    /// Patching changed: rows on screen fetch their icons now, and others as they scroll in.
    public func invalidateIcons() {
        coordinator?.invalidateIcons()
    }

    /// Expands `expanding` in order, then selects `ids`, reporting the new selection. `scrollToTop` puts the first of them at the top of the view.
    public func select(_ ids: [String], expanding: [String] = [], scrollToTop: Bool = false) async {
        await coordinator?.select(ids, expanding: expanding, scrollToTop: scrollToTop)
    }

    public func expand(_ ids: [String]) async {
        await coordinator?.expand(ids)
    }

    /// `treeview.CollapseAll()`.
    public func collapseAll() {
        coordinator?.collapseAll()
    }

    public func takeFocus() {
        coordinator?.takeFocus()
    }

    /// The scroll view, for measuring.
    public var scrollView: NSScrollView? {
        coordinator?.outline?.enclosingScrollView
    }

    /// Rows currently expanded into the table, for measuring.
    public var rowCount: Int {
        coordinator?.outline?.numberOfRows ?? 0
    }

    /// Selects by row number, as a click would, for tests and measuring.
    public func selectRows(_ rows: IndexSet) {
        coordinator?.outline?.selectRowIndexes(rows, byExtendingSelection: false)
    }

    /// A range gesture from code: the row of `anchor` stays the anchor and the selection runs to `endpoint`, as ⇧-click does.
    public func selectRange(anchor: String, endpoint: String) {
        coordinator?.selectRange(anchor: anchor, endpoint: endpoint)
    }
}

/// A tree view backed by `NSOutlineView`: virtualized, multi-select, drag to rearrange, inline rename and context menus, used for element trees and controller trees.
public struct OutlineView: NSViewRepresentable {
    let provider: OutlineDataProvider
    /// Change this to reload the tree after the data changes. A tree with a handle reloads through that instead.
    let revision: Int
    @Binding var selection: Set<String>
    var allowsMultipleSelection: Bool
    var allowsDragging: Bool
    var allowsRename: Bool
    var onDoubleClick: ((String) -> Void)?
    var menu: ((Set<String>) -> NSMenu?)?
    var handle: OutlineHandle?
    var onSelectionChange: ((OutlineSelection) -> Void)?
    var onCommand: ((OutlineCommand) -> Void)?
    var canPerform: ((OutlineCommand) -> Bool)?
    var syncChildLimit: Int
    /// Rows a collapse hides stay selected, and are highlighted again when shown, as upstream's controller tree keeps them (Display Setup F81).
    var keepsHiddenSelection = false
    var rowAccessibilityLabel: String?
    /// Turns row numbers in display order into selection order for a range gesture: selected, anchor, endpoint. Rust has the rule, and this kit can't name an area's API.
    var rangeOrder: (([UInt32], UInt32, UInt32) -> [UInt32])?

    public init(
        provider: OutlineDataProvider,
        revision: Int,
        selection: Binding<Set<String>>,
        allowsMultipleSelection: Bool = true,
        allowsDragging: Bool = false,
        allowsRename: Bool = false,
        onDoubleClick: ((String) -> Void)? = nil,
        menu: ((Set<String>) -> NSMenu?)? = nil,
        onClickedRow: ((String?) -> Void)? = nil
    ) {
        self.provider = provider
        self.revision = revision
        _selection = selection
        self.allowsMultipleSelection = allowsMultipleSelection
        self.allowsDragging = allowsDragging
        self.allowsRename = allowsRename
        self.onDoubleClick = onDoubleClick
        self.menu = menu
        handle = nil
        onSelectionChange = nil
        onCommand = nil
        canPerform = nil
        syncChildLimit = 2000
    }

    /// A tree driven through a handle, for big data: nothing about it runs on SwiftUI's update, selection comes back in order, and only what changed gets reloaded.
    public init(
        provider: OutlineDataProvider,
        handle: OutlineHandle,
        allowsMultipleSelection: Bool = true,
        allowsDragging: Bool = false,
        allowsRename: Bool = false,
        syncChildLimit: Int = 2000,
        keepsHiddenSelection: Bool = false,
        accessibilityLabel: String? = nil,
        rangeOrder: (([UInt32], UInt32, UInt32) -> [UInt32])? = nil,
        onSelectionChange: @escaping (OutlineSelection) -> Void,
        onDoubleClick: ((String) -> Void)? = nil,
        onCommand: ((OutlineCommand) -> Void)? = nil,
        canPerform: ((OutlineCommand) -> Bool)? = nil,
        menu: (() -> NSMenu?)? = nil
    ) {
        self.provider = provider
        revision = 0
        _selection = .constant([])
        self.allowsMultipleSelection = allowsMultipleSelection
        self.allowsDragging = allowsDragging
        self.allowsRename = allowsRename
        self.onDoubleClick = onDoubleClick
        self.menu = menu.map { build in { _ in build() } }
        self.handle = handle
        self.onSelectionChange = onSelectionChange
        self.onCommand = onCommand
        self.canPerform = canPerform
        self.syncChildLimit = syncChildLimit
        self.keepsHiddenSelection = keepsHiddenSelection
        rowAccessibilityLabel = accessibilityLabel
        self.rangeOrder = rangeOrder
    }

    public func makeCoordinator() -> Coordinator { Coordinator(self) }

    public func makeNSView(context: Context) -> NSScrollView {
        let outline = TreeOutlineView()
        outline.headerView = nil
        outline.style = .fullWidth
        outline.rowSizeStyle = .small
        outline.usesAutomaticRowHeights = false
        outline.rowHeight = 19
        outline.indentationPerLevel = 14
        outline.allowsMultipleSelection = allowsMultipleSelection
        outline.autoresizesOutlineColumn = false
        if let rowAccessibilityLabel {
            outline.setAccessibilityLabel(rowAccessibilityLabel)
        }
        let column = NSTableColumn(identifier: .init("title"))
        column.resizingMask = .autoresizingMask
        outline.addTableColumn(column)
        outline.outlineTableColumn = column
        outline.columnAutoresizingStyle = .uniformColumnAutoresizingStyle
        outline.dataSource = context.coordinator
        outline.delegate = context.coordinator
        outline.target = context.coordinator
        outline.doubleAction = #selector(Coordinator.doubleClicked(_:))
        outline.tree = context.coordinator
        if allowsDragging {
            outline.registerForDraggedTypes([Coordinator.pasteboardType])
            outline.setDraggingSourceOperationMask([.move, .copy], forLocal: true)
        }
        let scroll = NSScrollView()
        scroll.documentView = outline
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        context.coordinator.outline = outline
        context.coordinator.lastRevision = revision
        handle?.attach(context.coordinator)
        return scroll
    }

    public func updateNSView(_ scroll: NSScrollView, context: Context) {
        let c = context.coordinator
        c.parent = self
        handle?.attach(c)
        // Everything below is the plain form. A tree with a handle has nothing to do on SwiftUI's update.
        guard handle == nil else { return }
        if c.lastRevision != revision {
            c.lastRevision = revision
            c.reloadAll()
        }
        c.applySelection(selection)
    }

    public static func dismantleNSView(_ scroll: NSScrollView, coordinator: Coordinator) {
        coordinator.parent.handle?.detach(coordinator)
    }

    @MainActor
    public final class Coordinator: NSObject, NSOutlineViewDataSource, NSOutlineViewDelegate, NSTextFieldDelegate {
        static let pasteboardType = NSPasteboard.PasteboardType("com.vixenlights.outline-items")

        /// AppKit identifies rows by object, so each id keeps one box for as long as its row is loaded, which is how expansion survives a reload.
        final class Box: NSObject {
            let id: String
            var row: OutlineRow
            var children: [Box]?
            /// `iconGeneration` when this row's icon was last fetched.
            var iconGeneration = 0
            init(_ row: OutlineRow, generation: Int) {
                id = row.id
                self.row = row
                iconGeneration = generation
            }
        }

        var parent: OutlineView
        weak var outline: TreeOutlineView?
        var lastRevision = 0
        private var roots: [Box]?
        private var boxes: [String: Box] = [:]
        private var applyingSelection = false
        private var selectionOrder: [String] = []
        private var primary: String?
        private var anchor: String?
        private var endpoint: String?
        private var rangeGesture = false
        private var selectionReportPending = false
        private var renameSelectionTime = 0.0
        private var iconGeneration = 1
        private var iconRefreshPending = false
        private var draggedIDs: [String] = []
        private var lastDropSpot: String?
        private var lastDropAnswer: NSDragOperation = []
        /// Selected rows a collapse took off the screen, in selection order, when the tree keeps them.
        private var hiddenSelected: [String] = []

        init(_ parent: OutlineView) {
            self.parent = parent
        }

        private var usesHandle: Bool { parent.handle != nil }

        /// Whether the roots have been fetched into this tree.
        var hasRoots: Bool { roots != nil }

        private func makeBoxes(_ rows: [OutlineRow]) -> [Box] {
            var out: [Box] = []
            out.reserveCapacity(rows.count)
            for row in rows {
                if let existing = boxes[row.id] {
                    existing.row = row
                    existing.iconGeneration = iconGeneration
                    out.append(existing)
                } else {
                    let box = Box(row, generation: iconGeneration)
                    boxes[row.id] = box
                    out.append(box)
                }
            }
            return out
        }

        private func forget(_ gone: [Box]) {
            for box in gone {
                boxes[box.id] = nil
                if let children = box.children {
                    forget(children)
                }
            }
        }

        /// Replaces a parent's children, reusing the boxes of rows that stay.
        private func setChildren(of box: Box?, rows: [OutlineRow]) {
            let old = box == nil ? (roots ?? []) : (box?.children ?? [])
            let keep = Set(rows.map(\.id))
            forget(old.filter { !keep.contains($0.id) })
            let new = makeBoxes(rows)
            if let box {
                box.children = new
            } else {
                roots = new
            }
        }

        private func children(of box: Box?) -> [Box] {
            if let box {
                if box.children == nil {
                    setChildren(of: box, rows: parent.provider.outlineChildren(of: box.id))
                }
                return box.children ?? []
            }
            if roots == nil, !usesHandle {
                setChildren(of: nil, rows: parent.provider.outlineChildren(of: nil))
            }
            return roots ?? []
        }

        // MARK: Reloading

        /// The plain form: everything again, keeping expansion.
        func reloadAll() {
            guard let outline else { return }
            var expanded = Set<String>()
            func collect(_ boxes: [Box]) {
                for b in boxes where outline.isItemExpanded(b) {
                    expanded.insert(b.id)
                    collect(b.children ?? [])
                }
            }
            collect(roots ?? [])
            forget(roots ?? [])
            roots = nil
            outline.reloadData()
            func restore(_ boxes: [Box]) {
                for b in boxes where expanded.contains(b.id) {
                    outline.expandItem(b)
                    restore(children(of: b))
                }
            }
            restore(children(of: nil))
        }

        func loadRoots() async {
            let rows = await parent.provider.outlineChildrenAsync(of: nil)
            setChildren(of: nil, rows: rows)
            outline?.reloadData()
        }

        func reload(roots reloadRoots: Bool, parents: (String) -> Bool) async {
            guard let outline else { return }
            // Parents first in depth order isn't needed: each reload only touches its own children.
            var targets: [Box] = []
            func walk(_ list: [Box]) {
                for b in list {
                    guard let children = b.children else { continue }
                    if parents(b.id) {
                        targets.append(b)
                    }
                    walk(children)
                }
            }
            walk(roots ?? [])
            if reloadRoots {
                let rows = await parent.provider.outlineChildrenAsync(of: nil)
                setChildren(of: nil, rows: rows)
                outline.reloadData()
            }
            for box in targets where boxes[box.id] === box {
                let rows = await parent.provider.outlineChildrenAsync(of: box.id)
                setChildren(of: box, rows: rows)
                box.row.hasChildren = !rows.isEmpty
                box.row.childCount = rows.count
                outline.reloadItem(box, reloadChildren: true)
            }
            restoreSelection()
        }

        func refresh(rows wanted: (String) -> Bool) async {
            guard let outline else { return }
            var targets: [Box] = []
            func walk(_ list: [Box]) {
                for b in list {
                    if wanted(b.id) {
                        targets.append(b)
                    }
                    if let children = b.children {
                        walk(children)
                    }
                }
            }
            walk(roots ?? [])
            guard !targets.isEmpty else { return }
            let fresh = await parent.provider.outlineRows(ids: targets.map(\.id))
            for (box, row) in zip(targets, fresh) {
                guard let row else { continue }
                let lostChildren = !row.hasChildren && box.children != nil
                box.row = row
                box.iconGeneration = iconGeneration
                if lostChildren {
                    forget(box.children ?? [])
                    box.children = nil
                }
                outline.reloadItem(box, reloadChildren: lostChildren)
            }
        }

        // MARK: Icons

        func invalidateIcons() {
            iconGeneration += 1
            scheduleIconRefresh()
        }

        private func scheduleIconRefresh() {
            guard !iconRefreshPending else { return }
            iconRefreshPending = true
            DispatchQueue.main.async { [weak self] in
                self?.iconRefreshPending = false
                self?.refreshVisibleIcons()
            }
        }

        private func refreshVisibleIcons() {
            guard let outline else { return }
            let visible = outline.rows(in: outline.visibleRect)
            var stale: [(row: Int, box: Box)] = []
            for r in visible.lowerBound..<(visible.lowerBound + visible.length) {
                if let box = outline.item(atRow: r) as? Box, box.iconGeneration != iconGeneration {
                    stale.append((r, box))
                }
            }
            guard !stale.isEmpty else { return }
            let icons = parent.provider.outlineIcons(ids: stale.map(\.box.id))
            guard icons.count == stale.count else { return }
            for (entry, icon) in zip(stale, icons) {
                entry.box.iconGeneration = iconGeneration
                guard entry.box.row.icon != icon else { continue }
                entry.box.row.icon = icon
                (outline.view(atColumn: 0, row: entry.row, makeIfNecessary: false) as? OutlineCell)?.setIcon(icon)
            }
        }

        // MARK: Selection

        func applySelection(_ ids: Set<String>) {
            guard let outline else { return }
            var indexes = IndexSet()
            for row in 0..<outline.numberOfRows {
                if let b = outline.item(atRow: row) as? Box, ids.contains(b.id) {
                    indexes.insert(row)
                }
            }
            if indexes != outline.selectedRowIndexes {
                applyingSelection = true
                outline.selectRowIndexes(indexes, byExtendingSelection: false)
                applyingSelection = false
            }
        }

        func currentSelection() -> OutlineSelection {
            OutlineSelection(ids: selectionOrder, primary: selectionOrder.isEmpty ? nil : primary, hidden: hiddenSelected)
        }

        /// Puts the selection back on the rows that are still there, one pass over the rows however many are selected.
        private func restoreSelection() {
            guard let outline else { return }
            let before = selectionOrder
            selectionOrder = before.filter { boxes[$0] != nil }
            let wanted = Set(selectionOrder)
            var indexes = IndexSet()
            var shown = Set<String>()
            if !wanted.isEmpty {
                for row in 0..<outline.numberOfRows {
                    if let b = outline.item(atRow: row) as? Box, wanted.contains(b.id) {
                        indexes.insert(row)
                        shown.insert(b.id)
                    }
                }
            }
            if parent.keepsHiddenSelection {
                hiddenSelected = selectionOrder.filter { !shown.contains($0) }
            }
            applyingSelection = true
            outline.selectRowIndexes(indexes, byExtendingSelection: false)
            applyingSelection = false
            if selectionOrder != before {
                parent.onSelectionChange?(currentSelection())
            }
        }

        func select(_ ids: [String], expanding: [String], scrollToTop: Bool) async {
            guard let outline else { return }
            await expand(expanding)
            let wanted = Set(ids)
            var indexes = IndexSet()
            var first: Int?
            if !wanted.isEmpty {
                for row in 0..<outline.numberOfRows {
                    if let b = outline.item(atRow: row) as? Box, wanted.contains(b.id) {
                        indexes.insert(row)
                        if first == nil { first = row }
                    }
                }
            }
            rangeGesture = false
            anchor = ids.first
            endpoint = nil
            primary = ids.first
            selectionOrder = ids.filter { boxes[$0] != nil }
            if parent.keepsHiddenSelection {
                let shown = Set(indexes.compactMap { (outline.item(atRow: $0) as? Box)?.id })
                hiddenSelected = selectionOrder.filter { !shown.contains($0) }
            }
            applyingSelection = true
            outline.selectRowIndexes(indexes, byExtendingSelection: false)
            applyingSelection = false
            if let first {
                if scrollToTop, let clip = outline.enclosingScrollView?.contentView {
                    let rect = outline.rect(ofRow: first)
                    let top = min(rect.minY, max(0, outline.bounds.height - clip.bounds.height))
                    clip.scroll(to: NSPoint(x: clip.bounds.minX, y: max(0, top)))
                    outline.enclosingScrollView?.reflectScrolledClipView(clip)
                } else {
                    outline.scrollRowToVisible(first)
                }
            }
            parent.onSelectionChange?(currentSelection())
        }

        func expand(_ ids: [String]) async {
            guard let outline else { return }
            for id in ids {
                guard let box = boxes[id] else { continue }
                if box.children == nil {
                    let rows = await parent.provider.outlineChildrenAsync(of: id)
                    setChildren(of: box, rows: rows)
                    box.row.hasChildren = !rows.isEmpty
                    box.row.childCount = rows.count
                    outline.reloadItem(box, reloadChildren: true)
                }
                if !outline.isItemExpanded(box) {
                    outline.expandItem(box)
                }
            }
        }

        func collapseAll() {
            guard let outline else { return }
            outline.collapseItem(nil, collapseChildren: true)
        }

        func takeFocus() {
            guard let outline else { return }
            outline.window?.makeFirstResponder(outline)
        }

        func selectionInDisplayOrder() -> [String] {
            guard let outline else { return [] }
            flushSelection()
            return outline.selectedRowIndexes.compactMap { (outline.item(atRow: $0) as? Box)?.id }
        }

        func reveal(_ id: String) {
            guard let outline, let box = boxes[id] else { return }
            let row = outline.row(forItem: box)
            if row >= 0 {
                outline.scrollRowToVisible(row)
            }
        }

        func selectRange(anchor: String, endpoint: String) {
            guard let outline, let a = boxes[anchor], let e = boxes[endpoint] else { return }
            let (ra, re) = (outline.row(forItem: a), outline.row(forItem: e))
            guard ra >= 0, re >= 0 else { return }
            self.anchor = anchor
            self.endpoint = endpoint
            rangeGesture = true
            outline.selectRowIndexes(IndexSet(integersIn: min(ra, re)...max(ra, re)), byExtendingSelection: false)
        }

        fileprivate func noteMouseDown(_ event: NSEvent) {
            guard let outline else { return }
            let point = outline.convert(event.locationInWindow, from: nil)
            let row = outline.row(at: point)
            let id = row >= 0 ? (outline.item(atRow: row) as? Box)?.id : nil
            // A plain or ⇧ click picks a new selection, and what a collapse hid goes with the old one. The disclosure triangle and ⌘ leave it.
            let onTriangle = row >= 0 && outline.frameOfOutlineCell(atRow: row).contains(point)
            if parent.keepsHiddenSelection, !onTriangle, !event.modifierFlags.contains(.command) {
                hiddenSelected = []
            }
            if event.modifierFlags.contains(.shift), let id {
                if anchor == nil {
                    anchor = selectionOrder.last ?? id
                }
                endpoint = id
                rangeGesture = true
            } else {
                rangeGesture = false
                endpoint = nil
                anchor = id
            }
            if let id {
                primary = id
            }
        }

        /// ⇧ arrow keys move the endpoint and keep the anchor. Anything else starts afresh from wherever the selection lands.
        fileprivate func noteKeyDown(_ event: NSEvent) {
            guard let outline else { return }
            let key = event.charactersIgnoringModifiers?.unicodeScalars.first.map { Int($0.value) } ?? 0
            let isUp = key == NSUpArrowFunctionKey
            let isDown = key == NSDownArrowFunctionKey
            let isHome = key == NSHomeFunctionKey
            let isEnd = key == NSEndFunctionKey
            guard isUp || isDown || isHome || isEnd || key == NSLeftArrowFunctionKey || key == NSRightArrowFunctionKey || key == NSPageUpFunctionKey || key == NSPageDownFunctionKey else { return }
            if parent.keepsHiddenSelection, key != NSLeftArrowFunctionKey, key != NSRightArrowFunctionKey {
                hiddenSelected = []
            }
            if event.modifierFlags.contains(.shift), isUp || isDown || isHome || isEnd {
                let current = anchor.flatMap { boxes[$0] }.map { outline.row(forItem: $0) } ?? -1
                let start = current >= 0 ? current : (outline.selectedRowIndexes.last ?? 0)
                if anchor == nil, let box = outline.item(atRow: start) as? Box {
                    anchor = box.id
                }
                let from = endpoint.flatMap { boxes[$0] }.map { outline.row(forItem: $0) } ?? start
                let last = outline.numberOfRows - 1
                let to = isUp ? max(from - 1, 0) : isDown ? min(from + 1, last) : isHome ? 0 : last
                if let box = outline.item(atRow: to) as? Box {
                    endpoint = box.id
                    primary = box.id
                }
                rangeGesture = true
            } else {
                rangeGesture = false
                endpoint = nil
                anchor = nil
            }
        }

        public func outlineViewSelectionDidChange(_ notification: Notification) {
            if parent.allowsRename { renameSelectionTime = ProcessInfo.processInfo.systemUptime }
            guard !applyingSelection, let outline else { return }
            guard usesHandle else {
                let ids = Set(outline.selectedRowIndexes.compactMap { (outline.item(atRow: $0) as? Box)?.id })
                if ids != parent.selection {
                    parent.selection = ids
                }
                return
            }
            // A held key or a drag changes this often. One report per trip round the run loop is plenty.
            guard !selectionReportPending else { return }
            selectionReportPending = true
            DispatchQueue.main.async { [weak self] in
                self?.reportSelection()
            }
        }

        fileprivate func flushSelection() {
            if selectionReportPending {
                reportSelection()
            }
        }

        private func reportSelection() {
            selectionReportPending = false
            guard let outline else { return }
            let selected = outline.selectedRowIndexes
            var rows: [Int] = []
            rows.reserveCapacity(selected.count)
            rows.append(contentsOf: selected)
            if rangeGesture, let a = anchor.flatMap({ boxes[$0] }), let e = endpoint.flatMap({ boxes[$0] }) {
                let (ra, re) = (outline.row(forItem: a), outline.row(forItem: e))
                if ra >= 0, re >= 0, let order = parent.rangeOrder {
                    rows = order(rows.map { UInt32($0) }, UInt32(ra), UInt32(re)).map { Int($0) }
                }
            }
            selectionOrder = rows.compactMap { (outline.item(atRow: $0) as? Box)?.id }
            if parent.keepsHiddenSelection, !hiddenSelected.isEmpty {
                let shown = Set(selectionOrder)
                hiddenSelected.removeAll { shown.contains($0) || boxes[$0] == nil }
                selectionOrder.append(contentsOf: hiddenSelected)
            }
            if selectionOrder.isEmpty {
                primary = nil
            } else if primary == nil || !usesHandle {
                primary = selectionOrder.last
            } else if !rangeGesture, selected.count == 1 {
                primary = selectionOrder.first
            }
            parent.onSelectionChange?(currentSelection())
        }

        // MARK: Data source and delegate

        public func outlineView(_ o: NSOutlineView, numberOfChildrenOfItem item: Any?) -> Int {
            children(of: item as? Box).count
        }

        public func outlineView(_ o: NSOutlineView, child index: Int, ofItem item: Any?) -> Any {
            children(of: item as? Box)[index]
        }

        public func outlineView(_ o: NSOutlineView, isItemExpandable item: Any) -> Bool {
            (item as? Box)?.row.hasChildren ?? false
        }

        /// F81: the selected rows under a parent about to collapse stay selected while hidden.
        public func outlineViewItemWillCollapse(_ notification: Notification) {
            guard parent.keepsHiddenSelection, let outline, let box = notification.userInfo?["NSObject"] as? Box else { return }
            let start = outline.row(forItem: box)
            guard start >= 0 else { return }
            let level = outline.level(forRow: start)
            var hidden: [String] = []
            var r = start + 1
            while r < outline.numberOfRows, outline.level(forRow: r) > level {
                if outline.isRowSelected(r), let b = outline.item(atRow: r) as? Box {
                    hidden.append(b.id)
                }
                r += 1
            }
            guard !hidden.isEmpty else { return }
            // In selection order, which is what they had.
            let wanted = Set(hidden)
            let ordered = selectionOrder.filter { wanted.contains($0) }
            hiddenSelected.append(contentsOf: ordered.filter { !hiddenSelected.contains($0) })
        }

        /// F81: and come back highlighted when shown again.
        public func outlineViewItemDidExpand(_ notification: Notification) {
            guard parent.keepsHiddenSelection, !hiddenSelected.isEmpty, let outline, let box = notification.userInfo?["NSObject"] as? Box else { return }
            let start = outline.row(forItem: box)
            guard start >= 0 else { return }
            let level = outline.level(forRow: start)
            let wanted = Set(hiddenSelected)
            var indexes = IndexSet()
            var shown = Set<String>()
            var r = start + 1
            while r < outline.numberOfRows, outline.level(forRow: r) > level {
                if let b = outline.item(atRow: r) as? Box, wanted.contains(b.id) {
                    indexes.insert(r)
                    shown.insert(b.id)
                }
                r += 1
            }
            guard !indexes.isEmpty else { return }
            hiddenSelected.removeAll { shown.contains($0) }
            applyingSelection = true
            outline.selectRowIndexes(indexes, byExtendingSelection: true)
            applyingSelection = false
            // The same rows are selected, but no longer hidden.
            parent.onSelectionChange?(currentSelection())
        }

        /// A parent too big to fetch on the main thread fetches off it, and expands when that's done.
        public func outlineView(_ o: NSOutlineView, shouldExpandItem item: Any) -> Bool {
            guard let box = item as? Box, box.children == nil, (box.row.childCount ?? 0) > parent.syncChildLimit else { return true }
            let id = box.id
            Task { @MainActor [weak self] in
                await self?.expand([id])
            }
            return false
        }

        public func outlineView(_ o: NSOutlineView, viewFor column: NSTableColumn?, item: Any) -> NSView? {
            guard let box = item as? Box else { return nil }
            let id = NSUserInterfaceItemIdentifier("cell")
            let cell = (o.makeView(withIdentifier: id, owner: nil) as? OutlineCell) ?? OutlineCell(identifier: id)
            cell.configure(box.row, renameDelegate: parent.allowsRename ? self : nil)
            if box.iconGeneration != iconGeneration {
                scheduleIconRefresh()
            }
            return cell
        }

        public func outlineView(_ o: NSOutlineView, typeSelectStringFor column: NSTableColumn?, item: Any) -> String? {
            (item as? Box)?.row.title
        }

        @objc func doubleClicked(_ sender: Any?) {
            guard let outline, outline.clickedRow >= 0, let b = outline.item(atRow: outline.clickedRow) as? Box else { return }
            if let handler = parent.onDoubleClick {
                handler(b.id)
            } else if b.row.hasChildren {
                if outline.isItemExpanded(b) { outline.collapseItem(b) } else { outline.expandItem(b) }
            }
        }

        fileprivate func menuForSelection() -> NSMenu? {
            flushSelection()
            return parent.menu?(parent.selection)
        }

        // Inline rename is opt-in. Preserve the prop editor's 750 ms selection gate.
        fileprivate func beginInlineRename() -> Bool {
            guard parent.allowsRename else { return false }
            guard let outline, !outline.selectedRowIndexes.isEmpty,
                  ProcessInfo.processInfo.systemUptime - renameSelectionTime >= 0.75 else { return true }
            let row = primary.flatMap { boxes[$0] }.map { outline.row(forItem: $0) } ?? outline.selectedRow
            guard row >= 0, outline.selectedRowIndexes.contains(row),
                  let cell = outline.view(atColumn: 0, row: row, makeIfNecessary: true) as? NSTableCellView,
                  let field = cell.textField else { return true }
            field.selectText(nil)
            return true
        }

        public func control(_ control: NSControl, textShouldBeginEditing fieldEditor: NSText) -> Bool {
            guard parent.allowsRename, let outline else { return false }
            let row = outline.row(for: control)
            return row >= 0 && outline.selectedRowIndexes.contains(row)
                && ProcessInfo.processInfo.systemUptime - renameSelectionTime >= 0.75
        }

        public func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
            guard commandSelector == #selector(NSResponder.cancelOperation(_:)), parent.allowsRename,
                  let field = control as? NSTextField, let outline else { return false }
            let row = outline.row(for: field)
            guard row >= 0, let box = outline.item(atRow: row) as? Box else { return false }
            field.stringValue = box.row.title; textView.string = box.row.title
            outline.window?.makeFirstResponder(outline)
            return true
        }

        public func controlTextDidEndEditing(_ obj: Notification) {
            guard let field = obj.object as? NSTextField, let outline else { return }
            let row = outline.row(for: field)
            guard row >= 0, let b = outline.item(atRow: row) as? Box, field.stringValue != b.row.title else { return }
            parent.provider.outlineRename(item: b.id, to: field.stringValue)
        }

        // MARK: Drag and drop

        /// Only so many rows carry a pasteboard item, however many are dragged: making one per selected row is what would stall the start of a big drag.
        private static let draggedItemLimit = 16

        public func outlineView(_ o: NSOutlineView, pasteboardWriterForItem item: Any) -> NSPasteboardWriting? {
            guard parent.allowsDragging, let b = item as? Box else { return nil }
            let row = o.row(forItem: b)
            let rank = o.selectedRowIndexes.count(in: 0..<max(row, 0))
            if rank >= Self.draggedItemLimit, row != outline?.mouseDownRow {
                return nil
            }
            let p = NSPasteboardItem()
            p.setString(b.id, forType: Self.pasteboardType)
            return p
        }

        public func outlineView(_ o: NSOutlineView, draggingSession session: NSDraggingSession, willBeginAt screenPoint: NSPoint, forItems items: [Any]) {
            flushSelection()
            lastDropSpot = nil
            let clicked = (items.first as? Box)?.id
            if let outline, outline.selectedRowIndexes.contains(outline.mouseDownRow) {
                // In display order, whichever way the range was made: rows move in the order they're seen.
                draggedIDs = outline.selectedRowIndexes.compactMap { (outline.item(atRow: $0) as? Box)?.id }
            } else {
                draggedIDs = clicked.map { [$0] } ?? []
            }
        }

        public func outlineView(_ o: NSOutlineView, validateDrop info: NSDraggingInfo, proposedItem item: Any?, proposedChildIndex index: Int) -> NSDragOperation {
            guard !draggedIDs.isEmpty else { return [] }
            let target = item as? Box
            var childIndex: Int? = index == NSOutlineViewDropOnItemIndex ? nil : index
            if target == nil, childIndex == nil {
                // Dropped in the empty space under the rows: the end of the roots.
                childIndex = roots?.count ?? 0
                o.setDropItem(nil, dropChildIndex: childIndex ?? 0)
            }
            let copy = NSEvent.modifierFlags.contains(.option)
            let spot = "\(target?.id ?? "")|\(childIndex ?? -1)|\(copy)"
            if spot == lastDropSpot {
                return lastDropAnswer
            }
            lastDropSpot = spot
            let allowed = parent.provider.outlineCanDrop(items: draggedIDs, parent: target?.id, index: childIndex, copy: copy)
            lastDropAnswer = allowed ? (copy ? .copy : .move) : []
            return lastDropAnswer
        }

        public func outlineView(_ o: NSOutlineView, acceptDrop info: NSDraggingInfo, item: Any?, childIndex index: Int) -> Bool {
            let target = item as? Box
            let childIndex: Int? = index == NSOutlineViewDropOnItemIndex ? nil : index
            let copy = NSEvent.modifierFlags.contains(.option)
            guard usesHandle else {
                let accepted = parent.provider.outlineMove(items: draggedIDs, to: target?.id, index: index, copy: copy)
                if accepted {
                    target?.children = nil
                    reloadAll()
                }
                return accepted
            }
            parent.provider.outlineDrop(items: draggedIDs, parent: target?.id, index: childIndex, copy: copy)
            return true
        }

        public func outlineView(_ o: NSOutlineView, draggingSession session: NSDraggingSession, endedAt screenPoint: NSPoint, operation: NSDragOperation) {
            draggedIDs = []
            lastDropSpot = nil
        }

        fileprivate func runCommand(_ command: OutlineCommand) -> Bool {
            guard let run = parent.onCommand else { return false }
            flushSelection()
            if parent.canPerform?(command) == false {
                NSSound.beep()
                return true
            }
            run(command)
            return true
        }

        fileprivate func canRun(_ command: OutlineCommand) -> Bool {
            guard parent.onCommand != nil else { return false }
            return parent.canPerform?(command) ?? true
        }
    }
}

/// Shows the context menu for the row under the pointer, selecting it first as Finder does, and passes the keyboard's commands on.
final class TreeOutlineView: NSOutlineView {
    weak var tree: OutlineView.Coordinator?
    private(set) var mouseDownRow = -1

    override func menu(for event: NSEvent) -> NSMenu? {
        let row = row(at: convert(event.locationInWindow, from: nil))
        if row >= 0, !selectedRowIndexes.contains(row) {
            selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        }
        return tree?.menuForSelection() ?? super.menu(for: event)
    }

    override func mouseDown(with event: NSEvent) {
        mouseDownRow = row(at: convert(event.locationInWindow, from: nil))
        tree?.noteMouseDown(event)
        super.mouseDown(with: event)
    }

    override func keyDown(with event: NSEvent) {
        let key = event.charactersIgnoringModifiers?.unicodeScalars.first.map { Int($0.value) } ?? 0
        if (event.keyCode == 120 || key == NSF2FunctionKey), event.modifierFlags.intersection([.command, .option, .control, .shift]).isEmpty, tree?.beginInlineRename() == true { return }
        // Delete, forward delete and ⌘⌫, as Finder has it.
        if key == 0x7F || key == NSDeleteFunctionKey || key == NSBackspaceCharacter {
            if tree?.runCommand(.delete) == true {
                return
            }
        }
        tree?.noteKeyDown(event)
        super.keyDown(with: event)
    }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        if window?.firstResponder === self, event.modifierFlags.intersection(.deviceIndependentFlagsMask) == [.command, .shift],
           event.charactersIgnoringModifiers?.lowercased() == "v", tree?.runCommand(.pasteAsNew) == true {
            return true
        }
        return super.performKeyEquivalent(with: event)
    }

    @objc func copy(_ sender: Any?) {
        _ = tree?.runCommand(.copy)
    }

    @objc func paste(_ sender: Any?) {
        _ = tree?.runCommand(.paste)
    }

    @objc func pasteAsNew(_ sender: Any?) {
        _ = tree?.runCommand(.pasteAsNew)
    }

    @objc func delete(_ sender: Any?) {
        _ = tree?.runCommand(.delete)
    }

    nonisolated override func responds(to selector: Selector!) -> Bool {
        // With no commands set, these fall through to whatever else in the responder chain has them.
        if [#selector(copy(_:)), #selector(paste(_:)), #selector(delete(_:)), #selector(pasteAsNew(_:))].contains(selector) {
            return MainActor.assumeIsolated { tree?.parent.onCommand != nil }
        }
        return super.responds(to: selector)
    }

    override func validateUserInterfaceItem(_ item: any NSValidatedUserInterfaceItem) -> Bool {
        switch item.action {
        case #selector(copy(_:)): return tree?.canRun(.copy) ?? false
        case #selector(paste(_:)): return tree?.canRun(.paste) ?? false
        case #selector(pasteAsNew(_:)): return tree?.canRun(.pasteAsNew) ?? false
        case #selector(delete(_:)): return tree?.canRun(.delete) ?? false
        default: return super.validateUserInterfaceItem(item)
        }
    }
}

/// A round status ball or a folder. Drawn rather than stacked from layers, because a tree has a lot of these going past.
private final class OutlineIconView: NSView {
    var icon: OutlineRowIcon? {
        didSet {
            if icon != oldValue {
                needsDisplay = true
            }
        }
    }

    private static let folder = NSImage(systemSymbolName: "folder.fill", accessibilityDescription: nil)?
        .withSymbolConfiguration(NSImage.SymbolConfiguration(pointSize: 11, weight: .regular).applying(.init(hierarchicalColor: .systemBlue)))

    override func draw(_ dirtyRect: NSRect) {
        switch icon {
        case .folder:
            guard let image = Self.folder else { return }
            let size = image.size
            image.draw(in: NSRect(x: (bounds.width - size.width) / 2, y: (bounds.height - size.height) / 2, width: size.width, height: size.height))
        case .ball(let argb):
            let rect = bounds.insetBy(dx: (bounds.width - 10) / 2, dy: (bounds.height - 10) / 2)
            NSColor(argb: argb).setFill()
            NSBezierPath(ovalIn: rect).fill()
            NSColor.black.withAlphaComponent(0.35).setStroke()
            let outline = NSBezierPath(ovalIn: rect.insetBy(dx: 0.5, dy: 0.5))
            outline.lineWidth = 1
            outline.stroke()
        case nil:
            break
        }
    }
}

/// F25's dots: 8pt, 3pt apart.
private final class OutlineDotsView: NSView {
    var colors: [UInt32] = [] {
        didSet {
            if colors != oldValue {
                invalidateIntrinsicContentSize()
                needsDisplay = true
            }
        }
    }

    override var intrinsicContentSize: NSSize {
        NSSize(width: colors.isEmpty ? 0 : CGFloat(colors.count) * 8 + CGFloat(colors.count - 1) * 3, height: 8)
    }

    override func draw(_ dirtyRect: NSRect) {
        var x: CGFloat = 0
        for color in colors {
            NSColor(argb: color).setFill()
            NSBezierPath(ovalIn: NSRect(x: x, y: (bounds.height - 8) / 2, width: 8, height: 8)).fill()
            x += 11
        }
    }
}

private final class OutlineCell: NSTableCellView {
    private let dot = NSView()
    private let iconView = OutlineIconView()
    private let label = NSTextField(labelWithString: "")
    private let dots = OutlineDotsView()
    private var labelLeading: NSLayoutConstraint?

    convenience init(identifier: NSUserInterfaceItemIdentifier) {
        self.init(frame: .zero)
        self.identifier = identifier
        dot.wantsLayer = true
        dot.layer?.cornerRadius = 4
        dot.translatesAutoresizingMaskIntoConstraints = false
        iconView.translatesAutoresizingMaskIntoConstraints = false
        label.translatesAutoresizingMaskIntoConstraints = false
        dots.translatesAutoresizingMaskIntoConstraints = false
        label.lineBreakMode = .byTruncatingTail
        label.font = .systemFont(ofSize: NSFont.smallSystemFontSize + 1)
        label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        // F21: drawn literally, so "&" shows as itself.
        label.usesSingleLineMode = true
        dots.setContentHuggingPriority(.required, for: .horizontal)
        dots.setContentCompressionResistancePriority(.required, for: .horizontal)
        addSubview(dot)
        addSubview(iconView)
        addSubview(label)
        addSubview(dots)
        textField = label
        let leading = label.leadingAnchor.constraint(equalTo: leadingAnchor)
        labelLeading = leading
        NSLayoutConstraint.activate([
            dot.leadingAnchor.constraint(equalTo: leadingAnchor),
            dot.centerYAnchor.constraint(equalTo: centerYAnchor),
            dot.widthAnchor.constraint(equalToConstant: 8),
            dot.heightAnchor.constraint(equalToConstant: 8),
            iconView.leadingAnchor.constraint(equalTo: leadingAnchor),
            iconView.centerYAnchor.constraint(equalTo: centerYAnchor),
            iconView.widthAnchor.constraint(equalToConstant: 16),
            iconView.heightAnchor.constraint(equalToConstant: 16),
            leading,
            label.centerYAnchor.constraint(equalTo: centerYAnchor),
            dots.leadingAnchor.constraint(equalTo: label.trailingAnchor, constant: 6),
            dots.centerYAnchor.constraint(equalTo: centerYAnchor),
            dots.heightAnchor.constraint(equalToConstant: 8),
            dots.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor),
        ])
    }

    func configure(_ row: OutlineRow, renameDelegate: NSTextFieldDelegate?) {
        label.stringValue = row.title
        label.textColor = row.isDimmed ? .secondaryLabelColor : .labelColor
        label.isEditable = renameDelegate != nil
        label.delegate = renameDelegate
        dots.colors = row.trailingDots
        if let icon = row.icon {
            dot.isHidden = true
            setIcon(icon)
            return
        }
        iconView.isHidden = true
        if let color = row.statusColor {
            dot.isHidden = false
            dot.layer?.backgroundColor = NSColor(argb: color).cgColor
            labelLeading?.constant = 13
        } else {
            dot.isHidden = true
            labelLeading?.constant = 0
        }
    }

    func setIcon(_ icon: OutlineRowIcon?) {
        iconView.icon = icon
        iconView.isHidden = icon == nil
        labelLeading?.constant = icon == nil ? 0 : 19
    }
}
