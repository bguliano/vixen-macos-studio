import AppKit
import SwiftUI
import VixenBridge
import VixenKit

/// About Vixen.
struct AboutView: View {
    @Environment(AdminModel.self) private var admin
    @Environment(\.dismissWindow) private var dismissWindow
    @State private var license = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 24) {
                if let image = VixenResources.image("images/VixenImage.png") {
                    Image(nsImage: image)
                        .resizable()
                        .scaledToFit()
                        .frame(width: 196, height: 197)
                        .accessibilityHidden(true)
                }
                VStack(alignment: .leading, spacing: 8) {
                    Text(admin.version.versionName)
                        .font(.system(size: 20))
                    // STUDIO-BEGIN about: Studio's credit and links
                    Text("Vixen Studio (alpha) by bguliano")
                        .font(.headline)
                    Text("A redesigned macOS edition built on the unofficial Vixen port. It is not made, endorsed or supported by the Vixen team. Please report problems on Studio's GitHub page, not to the Vixen team.")
                        .fixedSize(horizontal: false, vertical: true)
                    Link("github.com/bguliano/vixen-macos-studio", destination: URL(string: "https://github.com/bguliano/vixen-macos-studio")!)
                    // STUDIO-END about
                }
                .padding(.top, 24)
            }
            ReadOnlyTextView(text: license)
            HStack {
                Spacer()
                Button("OK") { dismissWindow(id: WindowID.about) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(12)
        .frame(width: 900, height: 800)
        .onAppear {
            // A missing file leaves the box empty and logs it, where the original throws from Load.
            guard let url = VixenResources.url("License.txt"), let text = bundledText(path: url.path) else {
                logError(logger: "VixenApplication.AboutVixen", message: "License.txt couldn't be read.", error: nil)
                return
            }
            license = text
        }
    }
}

/// Vixen Release Notes.
struct ReleaseNotesView: View {
    @Environment(AdminModel.self) private var admin
    @Environment(\.dismissWindow) private var dismissWindow
    @State private var text = "Loading release notes..."

    var body: some View {
        VStack(spacing: 12) {
            ReadOnlyTextView(text: text)
            HStack {
                Spacer()
                Button("OK") { dismissWindow(id: WindowID.releaseNotes) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(12)
        .frame(minWidth: 400, minHeight: 300)
        .task {
            text = "Loading release notes..."
            text = await admin.releaseNotes()
        }
    }
}

/// Instrumentation. Asks for the values once a second, only while the window is showing.
struct InstrumentationView: View {
    @Environment(AdminModel.self) private var admin
    @Environment(\.dismissWindow) private var dismissWindow
    @State private var text = ""

    var body: some View {
        VStack(spacing: 8) {
            ReadOnlyTextView(text: text, monospaced: true)
            HStack {
                Spacer()
                Button("Close") { dismissWindow(id: WindowID.instrumentation) }
                    .keyboardShortcut(.defaultAction)
                Button("Reset") {
                    admin.app.core?.instrumentationReset()
                    refresh()
                }
            }
        }
        .padding(8)
        .frame(minWidth: 400, minHeight: 200)
        .task {
            // The first tick is a second after opening, as the original's timer's is.
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(1))
                if !Task.isCancelled {
                    refresh()
                }
            }
        }
    }

    private func refresh() {
        text = admin.app.core?.instrumentationLines().joined(separator: "\n") ?? ""
    }
}

/// Installed Modules.
struct InstalledModulesView: View {
    @Environment(\.dismissWindow) private var dismissWindow
    @State private var groups: [ModuleGroupRecord] = []
    @State private var selection: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Table(of: ModuleRowRecord.self, selection: $selection) {
                TableColumn("Name") { cell($0.typeName) }.width(min: 100, ideal: 150)
                TableColumn("Description") { cell($0.description) }.width(min: 150, ideal: 285)
                TableColumn("Author") { cell($0.author) }.width(min: 60, ideal: 110)
                TableColumn("Version") { cell($0.version) }.width(min: 48, ideal: 50)
                TableColumn("File") { cell($0.file) }.width(min: 150, ideal: 275)
            } rows: {
                ForEach(groups, id: \.name) { group in
                    Section(group.name) {
                        ForEach(group.rows, id: \.typeIdText) { row in
                            TableRow(row)
                        }
                    }
                }
            }
            HStack {
                Button("Copy Module Id", action: copyId)
                    .disabled(selection == nil)
                Spacer()
                Button("Close") { dismissWindow(id: WindowID.installedModules) }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(12)
        .frame(width: 915)
        .frame(minHeight: 456, maxHeight: 681)
        .onAppear { groups = installedModules() }
    }

    /// Long text is cut off in the column, so it's all in the tooltip too.
    private func cell(_ text: String) -> some View {
        Text(text)
            .lineLimit(1)
            .help(text)
    }

    private func copyId() {
        guard let selection else { return }
        // A UI test leaves the user's clipboard alone.
        let board = UITestHarness.isActive ? NSPasteboard(name: NSPasteboard.Name("VixenUITest")) : NSPasteboard.general
        board.clearContents()
        board.setString(selection, forType: .string)
        UITestHarness.note("copied \(selection)")
    }
}

extension ModuleRowRecord: Identifiable {
    public var id: String { typeIdText }
}

extension SetupPreviewRow: Identifiable {}
