// Studio Capture: captures one on-screen window to PNG with ScreenCaptureKit, for checking Vixen Studio's real rendering
// (Liquid Glass and materials, which offscreen snapshots can't draw). Launch with `open` so it owns its Screen Recording permission.
//   open -W -a "Studio Capture" --args --request
//   open -W -a "Studio Capture" --args --owner Vixen --title "Vixen Studio" --out /path/shot.png
import AppKit
import ScreenCaptureKit

let args = CommandLine.arguments
// CoreGraphics and ScreenCaptureKit need the window server connection NSApplication makes.
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
func value(_ flag: String) -> String? { args.firstIndex(of: flag).flatMap { $0 + 1 < args.count ? args[$0 + 1] : nil } }
let status = value("--status") ?? NSTemporaryDirectory() + "studio-capture-status.txt"
func finish(_ text: String) -> Never {
    try? text.write(toFile: status, atomically: true, encoding: .utf8)
    exit(text.hasPrefix("ok") ? 0 : 1)
}

if args.contains("--request") {
    if CGPreflightScreenCaptureAccess() { finish("ok granted") }
    _ = CGRequestScreenCaptureAccess()
    finish(CGPreflightScreenCaptureAccess() ? "ok granted" : "error not granted yet (approve in System Settings ▸ Privacy & Security ▸ Screen & System Audio Recording)")
}
guard CGPreflightScreenCaptureAccess() else { finish("error no screen recording permission") }
let owner = value("--owner") ?? "Vixen"
let title = value("--title")
guard let out = value("--out") else { finish("error --out required") }

Task {
    do {
        let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
        let windows = content.windows.filter {
            ($0.owningApplication?.applicationName ?? "").contains(owner) && (title == nil || ($0.title ?? "").contains(title!))
                && $0.frame.width > 100 && $0.frame.height > 100
        }
        guard let window = windows.first else {
            let names = content.windows.compactMap { w in w.owningApplication.map { "\($0.applicationName): \(w.title ?? "-")" } }
            finish("error no window; saw: " + names.filter { $0.contains(owner) }.joined(separator: " | "))
        }
        let filter = SCContentFilter(desktopIndependentWindow: window)
        let config = SCStreamConfiguration()
        let scale = CGFloat(filter.pointPixelScale)
        config.width = Int(window.frame.width * scale)
        config.height = Int(window.frame.height * scale)
        config.showsCursor = false
        let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
        let rep = NSBitmapImageRep(cgImage: image)
        try rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
        finish("ok \(image.width)x\(image.height) \(window.title ?? "")")
    } catch {
        finish("error \(error)")
    }
}
RunLoop.main.run()
