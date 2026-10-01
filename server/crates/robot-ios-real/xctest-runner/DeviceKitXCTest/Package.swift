// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "device_kit_xctest",
    platforms: [.iOS("13.0")],
    products: [
        .library(name: "device-kit-lib-xctest", targets: ["device_kit_lib_xctest"]),
    ],
    targets: [
        .target(
            name: "device_kit_lib_xctest",
            path: "Sources/device_kit_lib_xctest",
            linkerSettings: [.linkedFramework("XCTest")],
        ),
    ]
)
