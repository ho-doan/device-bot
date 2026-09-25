// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "FlutterFramework",
    platforms: [
        .iOS("13.0"),
    ],
    products: [
        .library(name: "FlutterFramework", targets: ["FlutterFramework"]),
    ],
    targets: [
        .target(name: "FlutterFramework"),
    ]
)
