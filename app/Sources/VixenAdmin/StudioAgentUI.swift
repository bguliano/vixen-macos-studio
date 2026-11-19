import StudioAgent
import SwiftUI
import VixenBridge
import VixenKit

/// The show window's agent status: hidden while no agent is connected, "Agent Connected" once one is, and
/// "Agent: <tool>" while it works. Click for the recent actions; agent edits undo from the Edit menu like your own.
struct StudioAgentStatusButton: View {
    let activity: StudioAgentActivity
    @State private var showingActions = false

    var body: some View {
        if activity.isAvailable, !activity.connectedClients.isEmpty || activity.currentTool != nil {
            Button { showingActions.toggle() } label: {
                if let tool = activity.currentTool {
                    Label("Agent: \(StudioAgentText.toolName(tool))", systemImage: "sparkles")
                        .symbolEffect(.pulse, options: .repeating)
                } else {
                    Label("Agent Connected", systemImage: "sparkles")
                }
            }
            .labelStyle(.titleAndIcon)
            .help("An AI agent is connected through MCP. Click to see what it has done.")
            .popover(isPresented: $showingActions, arrowEdge: .bottom) {
                StudioAgentActionsList(activity: activity)
            }
        }
    }
}

private struct StudioAgentActionsList: View {
    let activity: StudioAgentActivity

    var body: some View {
        VStack(alignment: .leading, spacing: StudioMetrics.spacingS) {
            Text("Agent Activity")
                .font(.headline)
            Text(activity.connectedClients.isEmpty ? "No agent connected" : "Connected: \(activity.connectedClients.joined(separator: ", "))")
                .font(.caption)
                .foregroundStyle(.secondary)
            Divider()
            if activity.recentActions.isEmpty {
                Text("No actions yet.")
                    .foregroundStyle(.secondary)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(activity.recentActions.reversed()) { action in
                            Label(StudioAgentText.toolName(action.summary),
                                  systemImage: action.succeeded ? "checkmark.circle" : "exclamationmark.triangle")
                                .foregroundStyle(action.succeeded ? Color.primary : Color.orange)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 260)
            }
            Text("Agent edits appear in Edit ▸ Undo, marked \"(Agent)\".")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(StudioMetrics.spacingM)
        .frame(width: 300)
    }
}

enum StudioAgentText {
    /// `effect_add` → "Effect Add".
    static func toolName(_ raw: String) -> String {
        raw.split(separator: "_").map { $0.prefix(1).uppercased() + $0.dropFirst() }.joined(separator: " ")
    }
}

/// Settings ▸ Agents: whether agents may drive real lights, and how to connect one.
struct StudioAgentSettings: View {
    @Environment(AdminModel.self) private var admin
    @State private var allowLiveOutput = false

    var body: some View {
        Form {
            Section {
                LabeledContent("MCP server") {
                    if let core = admin.app.core, core.studioMcpIsRunning() {
                        StudioStatusBadge("Running", tone: .active)
                    } else {
                        StudioStatusBadge("Not running", tone: .neutral)
                    }
                }
                Toggle("Allow agents to control live output", isOn: Binding(
                    get: { allowLiveOutput },
                    set: { allowed in
                        allowLiveOutput = allowed
                        admin.app.core?.studioSetAllowAgentsLiveOutput(allowed: allowed)
                    }
                ))
                .disabled(admin.app.core == nil)
            } footer: {
                Text("Off each time Studio starts. While off, agents can edit and preview your show but can't start controllers or send test patterns to real lights. Turning it off stops anything an agent started.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Section("Connect an Agent") {
                Text("Point any MCP client at the helper inside Studio, with your show folder:")
                    .font(.callout)
                Text(connectCommand)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                    .padding(StudioMetrics.spacingS)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.background.secondary, in: RoundedRectangle(cornerRadius: StudioMetrics.smallCornerRadius))
                Button("Copy Command") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(connectCommand, forType: .string)
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 520)
        .fixedSize(horizontal: false, vertical: true)
        .onAppear { allowLiveOutput = admin.app.core?.studioAllowAgentsLiveOutput() ?? false }
    }

    private var connectCommand: String {
        let helper = Bundle.main.bundleURL.appending(path: "Contents/Helpers/vixen-studio-mcp").path
        let profile = admin.app.core?.dataRoot() ?? "~/Documents/Vixen 3"
        return "claude mcp add --transport stdio vixen-studio -- \"\(helper)\" --profile \"\(profile)\""
    }
}
