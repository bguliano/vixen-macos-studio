import Foundation
import Observation

/// UI adapter only. The server's future FFI callback should deliver snapshots on MainActor.
/// No timer, hardware switch, business logic, or simulated connection is hidden here.
@MainActor @Observable
public final class StudioAgentActivity {
    public struct Action: Identifiable, Sendable {
        public let id: UInt64
        public let command: String
        public let summary: String
        public let succeeded: Bool
        public init(id: UInt64, command: String, summary: String, succeeded: Bool) {
            self.id = id; self.command = command; self.summary = summary; self.succeeded = succeeded
        }
    }
    public private(set) var connectedClients: [String] = []
    public private(set) var currentTool: String?
    public private(set) var recentActions: [Action] = []
    public private(set) var revision: UInt64 = 0
    public private(set) var isAvailable = false
    public init() {}

    /// Snapshot replacement makes reconnects idempotent and avoids dropping revision gaps.
    public func applySnapshot(revision: UInt64, clients: [String], currentTool: String?, actions: [Action]) {
        guard revision >= self.revision else { return }
        self.revision = revision
        connectedClients = clients
        self.currentTool = currentTool
        recentActions = Array(actions.suffix(100))
        isAvailable = true
    }
    public func disconnect() {
        isAvailable = false
        connectedClients = []
        currentTool = nil
        revision = 0
    }
}

#if canImport(VixenBridge)
import VixenBridge

private final class StudioActivityObserver: StudioChangeObserver, @unchecked Sendable {
    nonisolated let handler: @Sendable (UInt64, String, String, Bool) -> Void
    nonisolated init(handler: @escaping @Sendable (UInt64, String, String, Bool) -> Void) { self.handler = handler }
    nonisolated func changed(event: StudioChangeEvent) {
        handler(event.revision, event.kind, event.command, event.agent)
    }
}

extension StudioAgentActivity {
    /// Retain this model in the show window. Call stopObserving before releasing that window/core.
    public func observe(core: VixenCore) {
        core.studioObserveDisplay(observer: StudioActivityObserver { [weak self] revision, kind, command, agent in
            Task { @MainActor [weak self] in
                self?.receive(revision: revision, kind: kind, command: command, agent: agent)
            }
        })
        isAvailable = true
    }
    public func stopObserving(core: VixenCore) {
        core.studioStopObservingDisplay()
        disconnect()
    }
    private func receive(revision: UInt64, kind: String, command: String, agent: Bool) {
        guard revision > self.revision else { return }
        self.revision = revision
        switch kind {
        case "clientConnected": if !connectedClients.contains(command) { connectedClients.append(command) }
        case "clientDisconnected": connectedClients.removeAll { $0 == command }
        case "commandStarted": currentTool = command
        case "commandFinished", "commandFailed":
            currentTool = nil
            recentActions.append(Action(id: revision, command: command, summary: command, succeeded: kind == "commandFinished"))
            recentActions = Array(recentActions.suffix(100))
        default: break // displayChanged still updates revision, triggering the pane's snapshot refresh.
        }
    }
}
#endif
