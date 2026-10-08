// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "FrankenSonosSketch",
    platforms: [.macOS(.v14)],
    products: [.executable(name: "SketchShots", targets: ["SketchShots"])],
    targets: [
        .executableTarget(
            name: "SketchShots",
            path: "FrankenSonos",
            swiftSettings: [.unsafeFlags(["-Xfrontend", "-disable-sandbox"])]
        )
    ],
    swiftLanguageModes: [.v5]
)
