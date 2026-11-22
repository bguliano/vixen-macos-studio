import Foundation
import VixenBridge

/// The commit this app was built from, for the About window (#71), as `studio_status`' build reports it.
enum StudioBuildInfo {
    private struct Build: Decodable {
        let commit: String?
        let date: String?
        let dirty: Bool?
    }

    /// "Build 0dcd92986, 22 Nov 2026", with "(modified)" when tracked files differed from the commit; nil outside a checkout.
    static let line: String? = {
        guard let build = try? JSONDecoder().decode(Build.self, from: Data(studioBuildInfo().utf8)),
              let commit = build.commit else { return nil }
        var text = "Build \(commit.prefix(9))"
        if let date = build.date.flatMap({ try? Date($0, strategy: .iso8601) }) {
            text += ", " + date.formatted(date: .abbreviated, time: .omitted)
        }
        if build.dirty == true { text += " (modified)" }
        return text
    }()
}
