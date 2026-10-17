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

# 首屏快照：WebAssembly 加载前，页面先用这份预先算好的结果渲染（页面在静止时也完整）
node --input-type=module -e "
import { readFileSync, writeFileSync } from 'node:fs'
const dir = '$OUT'
const mod = await import(dir + '/qantrip_wasm.js')
mod.initSync({ module: readFileSync(dir + '/qantrip_wasm_bg.wasm') })
const input = 'r security --scope=staged -s 重点看认证和权限'
writeFileSync('$ROOT/apps/cloud/app/lib/demo-snapshot.json', JSON.stringify({ input, analysis: JSON.parse(mod.analyze(input, undefined)) }))
console.log('快照已写入 app/lib/demo-snapshot.json')
"
