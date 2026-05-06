// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "GPUMonitorTray",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "GPUMonitorTray",
            resources: [.process("Resources")]
        )
    ]
)
