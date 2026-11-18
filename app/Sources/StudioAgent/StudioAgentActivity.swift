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
