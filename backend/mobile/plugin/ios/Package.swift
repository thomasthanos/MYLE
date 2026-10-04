// swift-tools-version:5.3

import PackageDescription

let package = Package(
  name: "tauri-plugin-myle-mobile",
  platforms: [
    .iOS(.v13)
  ],
  products: [
    .library(
      name: "tauri-plugin-myle-mobile",
      type: .static,
      targets: ["tauri-plugin-myle-mobile"])
  ],
  dependencies: [
    .package(name: "Tauri", path: "../.tauri/tauri-api")
  ],
  targets: [
    .target(
      name: "tauri-plugin-myle-mobile",
      dependencies: [
        .byName(name: "Tauri")
      ],
      path: "Sources")
  ]
)
