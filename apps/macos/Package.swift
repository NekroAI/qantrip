// swift-tools-version:6.0
import PackageDescription

// qantrip_coreFFI / QantripCore 的内容由 scripts/build-macos.sh 从 Rust 核心生成。
let generatedLib = Context.packageDirectory + "/Generated/lib"

let package = Package(
    name: "Qantrip",
    platforms: [.macOS(.v14)],
    targets: [
        .target(name: "qantrip_coreFFI"),
        .target(
            name: "QantripCore",
            dependencies: ["qantrip_coreFFI"],
            linkerSettings: [.unsafeFlags(["-L", generatedLib]), .linkedLibrary("qantrip_core")]
        ),
        .executableTarget(name: "Qantrip", dependencies: ["QantripCore"]),
        // 测试用：接收粘贴并把内容写入文件的探针应用
        .executableTarget(name: "PasteProbe"),
        // 测试用：通过分布式通知触发唤起 / 插入
        .executableTarget(name: "QantripDebug"),
    ],
    swiftLanguageModes: [.v5]
)
