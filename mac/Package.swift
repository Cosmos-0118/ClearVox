// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Sordino",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(name: "Sordino", path: "Sources/Sordino")
    ]
)
