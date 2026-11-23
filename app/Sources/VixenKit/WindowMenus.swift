import AppKit

/// The menu bar keeps to the window in front. A window's own menu (the prop editor's Prop, the preview designer's Settings)
/// fills only while that window is in front, and SwiftUI still shows it, empty, otherwise: this hides every top-level menu
/// with nothing in it, and shows it again once it has items. Menus other code hid (the sequencer's) stay as they were.
@MainActor
public enum WindowMenus {
    private static var installed = false
    private static var pending = false
    /// True while arranging: the changes made then (SwiftUI filling a menu, hiding an item) don't arrange again.
    private static var arranging = false
    /// The menus hidden here, so a menu hidden by its own window is never shown here.
    private static let hiddenHere = NSHashTable<NSMenuItem>.weakObjects()

    /// Watches key-window and menu changes. Safe to call more than once.
    public static func install() {
        guard !installed else { return }
        installed = true
        let center = NotificationCenter.default
        for name in [NSWindow.didBecomeKeyNotification, NSWindow.didResignKeyNotification, NSWindow.didBecomeMainNotification,
                     NSApplication.didBecomeActiveNotification, NSApplication.didResignActiveNotification] {
            center.addObserver(forName: name, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated { scheduleArrange(again: true) }
            }
        }
        for name in [NSMenu.didAddItemNotification, NSMenu.didRemoveItemNotification, NSMenu.didChangeItemNotification] {
            center.addObserver(forName: name, object: nil, queue: .main) { note in
                // Menus post these on the main thread, where this runs too.
                nonisolated(unsafe) let object = note.object
                MainActor.assumeIsolated {
                    guard !arranging, let menu = object as? NSMenu, let main = NSApp.mainMenu else { return }
                    if menu === main || menu.supermenu === main { scheduleArrange(again: false) }
                }
            }
        }
        scheduleArrange(again: true)
    }

    /// Arranges again shortly, for a window menu that fills without a key-window change.
    public static func setNeedsArrange() {
        scheduleArrange(again: true)
    }

    /// Arranges once the current change has settled. SwiftUI fills a window's menus a moment after the window becomes key,
    /// so a key change arranges again shortly after.
    private static func scheduleArrange(again: Bool) {
        if !pending {
            pending = true
            DispatchQueue.main.async {
                pending = false
                arrange()
            }
        }
        if again {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { arrange() }
        }
    }

    /// Hides each top-level menu with no visible items and shows the ones hidden here that have items again.
    public static func arrange() {
        guard !arranging, let main = NSApp.mainMenu else { return }
        arranging = true
        defer { arranging = false }
        for item in main.items {
            guard let sub = item.submenu else { continue }
            // SwiftUI brings a menu's items up to date when it's about to open.
            sub.delegate?.menuNeedsUpdate?(sub)
            let empty = !sub.items.contains { !$0.isSeparatorItem && !$0.isHidden }
            if empty {
                if !item.isHidden {
                    item.isHidden = true
                    hiddenHere.add(item)
                }
            } else if hiddenHere.contains(item) {
                hiddenHere.remove(item)
                item.isHidden = false
            }
        }
    }
}
