import AppKit
import VixenBridge
import VixenKit

/// Patching by intent in the Display pane: a Patch submenu on one element or group, showing where it's patched as
/// universe:channel ranges, with Reverse, Swap With and Move to Output. Rust works out which outputs move
/// (`studio_patch.rs`); each choice runs the MCP tool's command (`patch_reverse`, `patch_swap`, `patch_move`) as one step
/// on the shared display history, so Edit ▸ Undo takes it back.
extension SetupModel {
    private struct PatchRange: Decodable {
        let text: String
    }

    private struct PatchTarget: Decodable {
        let id: String
        let name: String
        let text: String
    }

    private struct PatchSpot: Decodable {
        let startOutput: UInt32
        let firstOutput: UInt32
        let lastOutput: UInt32
        let from: String
        let to: String
    }

    private struct PatchController: Decodable {
        let controllerId: String
        let controller: String
        let spots: [PatchSpot]
    }

    private struct PatchMenu: Decodable {
        let id: String
        let name: String
        let lights: Int
        let outputs: Int
        let ranges: [PatchRange]
        let canReverse: Bool
        let swapWith: [PatchTarget]
        let moveTo: [PatchController]
    }

    /// The Patch item for the element menu, or nil outside the live pane or with other than one row selected.
    func studioPatchMenuItem() -> NSMenuItem? {
        let live = StudioDisplayLive.shared
        guard live.paneVisible, let session, StudioDisplayLive.model === self else { return nil }
        let selection = elementTree.selectionInDisplayOrder
        guard selection.count == 1, let text = session.studioPatchMenu(row: selection[0]),
              let info = try? JSONDecoder().decode(PatchMenu.self, from: Data(text.utf8)) else { return nil }
        let enabled = live.blocked == nil
        let menu = NSMenu()
        menu.autoenablesItems = false
        if info.ranges.isEmpty {
            menu.addItem(SetupMenuItem("Not patched", enabled: false))
        } else {
            let lights = info.lights == 1 ? "1 light" : "\(info.lights) lights"
            menu.addItem(SetupMenuItem("\(lights) on \(info.outputs) outputs:", enabled: false))
            for range in info.ranges.prefix(6) {
                menu.addItem(SetupMenuItem("  " + range.text, enabled: false))
            }
            if info.ranges.count > 6 {
                menu.addItem(SetupMenuItem("  and \(info.ranges.count - 6) more ranges", enabled: false))
            }
        }
        menu.addItem(.separator())
        menu.addItem(SetupMenuItem("Reverse", enabled: enabled && info.canReverse,
                                   tooltip: "Patch the lights in the opposite order on the same outputs, for a string wired backwards") { [weak self] in
            self?.studioPatch("patch_reverse", ["id": info.id])
        })

        let swap = NSMenu()
        swap.autoenablesItems = false
        if info.swapWith.isEmpty {
            swap.addItem(SetupMenuItem(info.outputs == 0 ? "Patch it first" : "Nothing else uses \(info.outputs) outputs", enabled: false))
        }
        for target in info.swapWith {
            let item = SetupMenuItem("\(target.name) — \(target.text)", enabled: enabled) { [weak self] in
                self?.studioPatch("patch_swap", ["id": info.id, "withId": target.id])
            }
            swap.addItem(item)
        }
        let swapItem = SetupMenuItem("Swap With", enabled: info.outputs > 0,
                                     tooltip: "Trade outputs with an element or group that uses the same number, for two strings hung in each other's place")
        swapItem.submenu = swap
        menu.addItem(swapItem)

        let move = NSMenu()
        move.autoenablesItems = false
        if info.moveTo.isEmpty {
            move.addItem(SetupMenuItem(info.outputs == 0 ? "Patch it first" : "No \(info.outputs) free outputs in a row", enabled: false))
        }
        for controller in info.moveTo {
            move.addItem(SetupMenuItem(controller.controller, enabled: false))
            for spot in controller.spots {
                let address = spot.from.contains(":") ? " (\(spot.from)–\(spot.to))" : ""
                let title = "  Outputs \(spot.firstOutput)–\(spot.lastOutput)\(address)"
                move.addItem(SetupMenuItem(title, enabled: enabled) { [weak self] in
                    self?.studioPatch("patch_move", ["id": info.id, "controllerId": controller.controllerId, "startOutput": spot.startOutput])
                })
            }
        }
        let moveItem = SetupMenuItem("Move to Output", enabled: info.outputs > 0,
                                     tooltip: "Move these outputs, in order, to free outputs on a controller")
        moveItem.submenu = move
        menu.addItem(moveItem)

        let item = SetupMenuItem("Patch")
        item.submenu = menu
        return item
    }

    private func studioPatch(_ command: String, _ arguments: [String: Any]) {
        StudioDisplayLive.shared.perform(command: command, arguments: arguments, model: self, app: app)
    }
}
