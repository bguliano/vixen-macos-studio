import AppKit
import VixenBridge

/// How narrow the sequencer window's three panes may get (UI roadmap R42). Upstream lets the window shrink to 640 points
/// with side panes 200 and 340 wide, which left the timeline about 100 points and pushed it under the inspector; and every
/// window resize was saved as the panes' new widths, so a briefly narrowed window kept squeezed panels for good.
///
/// Here each pane keeps a minimum the split view honours, the window's minimum width is the sum for the panes that are
/// showing, and only a divider the user drags is remembered.
enum StudioDockLayout {
    /// Wide enough for the Effects library's search field, group names and the effect names beside their icons.
    static let leftMinimum: CGFloat = 180
    /// The row names and enough of the grid to place an effect.
    static let centreMinimum: CGFloat = 320
    /// The inspector's labelled tabs and its value column.
    static let rightMinimum: CGFloat = 260
    /// Above the panes' holding priorities, so window resizes take space from the timeline first and never squeeze a
    /// pane below its minimum; below required, so a hidden pane's constraint gives way quietly.
    static let minimumPriority = NSLayoutConstraint.Priority(490)

    /// The window's minimum width: every showing pane at its minimum, and the dividers between them. Before the layout
    /// has loaded, both side panes count.
    static func minimumWidth(_ panels: ShellPanels?, divider: CGFloat = 1) -> CGFloat {
        var width = centreMinimum
        if !(panels?.leftTabs.isEmpty ?? false) { width += leftMinimum + divider }
        if !(panels?.rightTabs.isEmpty ?? false) { width += rightMinimum + divider }
        return width
    }

    /// The side panes' widths to open at in a split view `width` wide: the saved widths (nil for a hidden pane), raised
    /// to their minimums, then narrowed toward the minimums as far as needed to leave the timeline its own.
    static func fit(width: CGFloat, divider: CGFloat, left: Double?, right: Double?) -> (left: CGFloat?, right: CGFloat?) {
        var l = left.map { max(CGFloat($0), leftMinimum) }
        var r = right.map { max(CGFloat($0), rightMinimum) }
        let dividers = divider * CGFloat((l == nil ? 0 : 1) + (r == nil ? 0 : 1))
        var excess = (l ?? 0) + (r ?? 0) + dividers + centreMinimum - width
        if excess > 0, let current = r {
            let take = min(excess, current - rightMinimum)
            r = current - take
            excess -= take
        }
        if excess > 0, let current = l {
            l = current - min(excess, current - leftMinimum)
        }
        return (l, r)
    }

    /// Puts the dividers where `fit` says for the split view's current width. Placing a divider resizes the panes, which
    /// asks for this again; the nested call is skipped.
    @MainActor
    static func apply(to split: NSSplitView, left: Double?, right: Double?) {
        guard !applying, split.arrangedSubviews.count == 3 else { return }
        applying = true
        defer { applying = false }
        let width = split.bounds.width
        let fitted = fit(width: width, divider: split.dividerThickness, left: left, right: right)
        if let l = fitted.left {
            split.setPosition(l, ofDividerAt: 0)
        }
        if let r = fitted.right {
            split.setPosition(width - r - split.dividerThickness, ofDividerAt: 1)
        }
    }

    @MainActor private static var applying = false
    @MainActor private static var resizing = false

    /// Runs a window resize's re-layout of the panes, which isn't the user choosing widths.
    @MainActor
    static func whileResizing(_ body: () -> Void) {
        let outer = resizing
        resizing = true
        defer { resizing = outer }
        body()
    }

    /// Gives the left, centre and right panes their minimum widths.
    @MainActor
    static func constrain(_ panes: [NSView]) {
        for (pane, minimum) in zip(panes, [leftMinimum, centreMinimum, rightMinimum]) {
            let constraint = pane.widthAnchor.constraint(greaterThanOrEqualToConstant: minimum)
            constraint.priority = minimumPriority
            constraint.identifier = "studio.dock.minimum"
            constraint.isActive = true
        }
    }

    /// The side widths a divider drag sets: a pane whose width differs from where `fit` put it for the saved widths was
    /// dragged; the other keeps its saved width (nil), even while a narrow window holds it below that.
    @MainActor
    static func dragged(_ split: NSSplitView, left: Double?, right: Double?) -> (left: Double?, right: Double?) {
        guard split.arrangedSubviews.count == 3 else { return (nil, nil) }
        let fitted = fit(width: split.bounds.width, divider: split.dividerThickness, left: left, right: right)
        func moved(_ pane: NSView, _ expected: CGFloat?) -> Double? {
            guard let expected, !pane.isHidden, abs(pane.frame.width - expected) > 0.5 else { return nil }
            return Double(pane.frame.width)
        }
        return (moved(split.arrangedSubviews[0], fitted.left), moved(split.arrangedSubviews[2], fitted.right))
    }

    /// True when the split view's panes changed because the user is dragging one of its dividers, not because the
    /// window resized or a pane was shown or hidden.
    @MainActor
    static func isDividerDrag(in split: NSSplitView) -> Bool {
        // A window resize or a divider this type placed can follow a click, so the last event alone can't tell.
        guard !applying, !resizing else { return false }
        guard let window = split.window, !window.inLiveResize, let event = NSApp.currentEvent, event.window === window else {
            return false
        }
        return event.type == .leftMouseDragged || event.type == .leftMouseUp
    }
}
