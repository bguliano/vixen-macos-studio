import Foundation
import Observation

/// UI adapter only. The server's future FFI callback should deliver snapshots on MainActor.
/// No timer, hardware switch, business logic, or simulated connection is hidden here.
@MainActor @Observable
public final class StudioAgentActivity {
    public struct Action: Identifiable, Sendable, Decodable {
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
    public private(set) var changedDocumentId: String?
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
    nonisolated let handler: @Sendable (UInt64, String, String, Bool, String?) -> Void
    nonisolated init(handler: @escaping @Sendable (UInt64, String, String, Bool, String?) -> Void) { self.handler = handler }
    nonisolated func changed(event: StudioChangeEvent) {
        handler(event.revision, event.kind, event.command, event.agent, event.documentId)
    }
}

extension StudioAgentActivity {
    /// Retain this model in the show window. Call stopObserving before releasing that window/core.
    public func observe(core: VixenCore) {
        core.studioObserveDisplay(observer: StudioActivityObserver { [weak self, weak core] revision, kind, command, agent, documentId in
            Task { @MainActor [weak self] in
                self?.receive(revision: revision, kind: kind, command: command, agent: agent, documentId: documentId)
                if let core { self?.refresh(core: core) }
            }
        })
        refresh(core: core)
    }
    private struct Snapshot: Decodable {let revision: UInt64; let clients: [String]; let tools: [String]; let actions: [Action]}
    public func refresh(core: VixenCore) {
        if let data = core.studioAgentSnapshot().data(using: .utf8), let snapshot = try? JSONDecoder().decode(Snapshot.self, from: data) {
            applySnapshot(revision: snapshot.revision, clients: snapshot.clients, currentTool: snapshot.tools.isEmpty ? nil : snapshot.tools.joined(separator: ", "), actions: snapshot.actions)
        }
        isAvailable = core.studioMcpIsRunning()
    }
    public func stopObserving(core: VixenCore) {
        core.studioStopObservingDisplay()
        disconnect()
    }
    private func receive(revision: UInt64, kind: String, command: String, agent: Bool, documentId: String?) {
        guard revision > self.revision else { return }
        self.revision = revision
        if kind == "documentChanged" { changedDocumentId = documentId }
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
