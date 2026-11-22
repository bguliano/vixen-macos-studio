// swift-tools-version: 6.2
import Foundation
import PackageDescription

// scripts/build_app.sh runs this from a build folder that holds the generated bindings. Build with the script,
// not with `swift build` in this folder.

/// UI modules are main-actor by default: SwiftUI and AppKit work happens there, and anything that should
/// leave the main thread says so explicitly.
let uiSettings: [SwiftSetting] = [
    .defaultIsolation(MainActor.self),
    .enableUpcomingFeature("InferIsolatedConformances"),
    .enableUpcomingFeature("NonisolatedNonsendingByDefault"),
]

/// One module per window area. A module may use VixenKit and VixenBridge, and no other area except as listed in
/// `areaDependencies`, so a worker can build it alone. VixenApp finds whichever are present with `#if canImport`.
let allAreas = [
    "VixenAdmin", "VixenDialogs", "VixenSetup", "VixenPreview", "VixenTimeline", "VixenEffectEditor", "VixenMarks", "VixenSequencer",
    "VixenScheduler", "VixenTools",
]
/// The sequencer window is assembled from the first three, plus the layer filter dialogs, and Display Setup opens the property, filter and controller dialogs
/// in the last. They stay independent of each other and of what uses them.
let areaDependencies = [
    "VixenSequencer": ["VixenTimeline", "VixenEffectEditor", "VixenMarks", "VixenDialogs"],
    "VixenSetup": ["VixenDialogs"],
    "VixenMarks": ["VixenDialogs"],
    "VixenTools": ["VixenEffectEditor"],
]
let requested = ProcessInfo.processInfo.environment["VIXEN_SWIFT_MODULES"]?.split(separator: ",").map(String.init) ?? []
let sourcesFolder = Context.packageDirectory + "/Sources"
let areas = (requested.isEmpty ? allAreas : requested).filter { area in
    let files = (try? FileManager.default.contentsOfDirectory(atPath: sourcesFolder + "/" + area)) ?? []
    return files.contains { $0.hasSuffix(".swift") }
}

let package = Package(
    name: "Vixen",
    platforms: [.macOS(.v26)],
    products: [.executable(name: "Vixen", targets: ["VixenApp"])],
    targets: [
        // STUDIO-BEGIN sparkle-package: automatic updates with Sparkle 2 (B49)
        // scripts/build_app.sh puts the pinned, hash-checked release here (scripts/studio_sparkle.sh).
        .binaryTarget(name: "Sparkle", path: "Frameworks/Sparkle.xcframework"),
        // STUDIO-END sparkle-package
        // The C header UniFFI generates for the Rust library.
        .target(name: "vixen_ffiFFI", path: "Sources/vixen_ffiFFI"),
        // The Swift bindings UniFFI generates. Generated code stays in Swift 5 mode.
        .target(
            name: "VixenBridge",
            dependencies: ["vixen_ffiFFI"],
            path: "Sources/VixenBridge",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .target(name: "StudioAgent", dependencies: ["VixenBridge"], path: "Sources/StudioAgent", swiftSettings: uiSettings), // STUDIO: live agent activity adapter
        .target(name: "VixenKit", dependencies: ["VixenBridge"], path: "Sources/VixenKit", swiftSettings: uiSettings),
    ] + areas.map { area in
        let others = (areaDependencies[area] ?? []).filter(areas.contains).map { Target.Dependency.target(name: $0) }
        let updater: [Target.Dependency] = area == "VixenAdmin" ? ["Sparkle"] : [] // STUDIO: the show window's area runs the updater (B49)
        return .target(name: area, dependencies: ["VixenKit", "VixenBridge", "StudioAgent"] + others + updater, path: "Sources/\(area)", swiftSettings: uiSettings) // STUDIO: areas show agent activity
    } + [
        .executableTarget(
            name: "VixenApp",
            dependencies: ["StudioAgent", "VixenKit", "VixenBridge"] + areas.map { .target(name: $0) }, // STUDIO: expose agent activity to app toolbar
            path: "Sources/VixenApp",
            swiftSettings: uiSettings,
            linkerSettings: [
                .unsafeFlags(["-L", Context.packageDirectory + "/lib"]),
                .unsafeFlags(["-Xlinker", "-rpath", "-Xlinker", "@executable_path/../Frameworks"]), // STUDIO: Sparkle.framework in the bundle (B49)
                .linkedLibrary("vixen_ffi"),
                .linkedLibrary("c++"),
                .linkedLibrary("curl"),
                .linkedFramework("AudioToolbox"),
                .linkedFramework("AudioUnit"),
                .linkedFramework("CoreAudio"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("CoreGraphics"),
                .linkedFramework("CoreText"),
                .linkedFramework("Foundation"),
                .linkedFramework("IOKit"),
                .linkedFramework("Metal"),
                .linkedFramework("QuartzCore"),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
            ]
        ),
    ]
)
