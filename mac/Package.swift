// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "ClearVox",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(name: "ClearVox", path: "Sources/ClearVox")
    ]
)
