#!/usr/bin/env bash
# 构建官网在线演示用的 WebAssembly 核心（docs/decisions.md D27），产物写入 apps/cloud/public/wasm/。
# 依赖：rustup target add wasm32-unknown-unknown；cargo install wasm-bindgen-cli --version 0.2.129
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
OUT="$ROOT/apps/cloud/public/wasm"
cargo build --release --target wasm32-unknown-unknown --manifest-path "$ROOT/core/wasm/Cargo.toml"
mkdir -p "$OUT"
wasm-bindgen --target web --no-typescript --out-dir "$OUT" \
  "$ROOT/core/wasm/target/wasm32-unknown-unknown/release/qantrip_wasm.wasm"
ls -la "$OUT"

# 首屏快照与模板一览：WebAssembly 加载前，页面先用预先算好的结果渲染（静止时也完整，也便于搜索引擎读取）
node --input-type=module -e "
import { readFileSync, writeFileSync } from 'node:fs'
const dir = '$OUT'
const mod = await import(dir + '/qantrip_wasm.js')
mod.initSync({ module: readFileSync(dir + '/qantrip_wasm_bg.wasm') })
const inputs = { zh: 'r security --scope=staged -s 重点看认证和权限', en: 'r security --scope=staged -s focus on the auth module' }
const snapshot = {}, templates = {}
for (const [loc, input] of Object.entries(inputs)) {
  mod.set_locale(loc)
  snapshot[loc] = { input, analysis: JSON.parse(mod.analyze(input, undefined)) }
  templates[loc] = JSON.parse(mod.templates())
}
writeFileSync('$ROOT/apps/cloud/app/lib/demo-snapshot.json', JSON.stringify(snapshot))
writeFileSync('$ROOT/apps/cloud/app/lib/templates.json', JSON.stringify(templates))
console.log('已写入 app/lib/demo-snapshot.json 与 app/lib/templates.json')
"
