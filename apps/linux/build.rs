// 版本号取仓库根目录的 VERSION（与 macOS / Windows 一致）
fn main() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../VERSION");
    println!("cargo:rerun-if-changed={path}");
    let version = std::fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_else(|_| "0.0.0".into());
    println!("cargo:rustc-env=QANTRIP_VERSION={version}");
}
