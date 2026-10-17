#!/usr/bin/env bash
# CI：把发布签名证书导入临时钥匙串（docs/decisions.md D22）。
#   输入：P12_BASE64（base64 编码的 p12）、P12_PASSWORD
#   输出：SIGN_IDENTITY / SIGN_KEYCHAIN（GitHub Actions 中写入 $GITHUB_ENV，否则打印 export 语句）
set -euo pipefail
: "${P12_BASE64:?缺少 P12_BASE64}" "${P12_PASSWORD:?缺少 P12_PASSWORD}"
KC="${RUNNER_TEMP:-$(mktemp -d)}/qantrip-signing.keychain-db"
KC_PASS="$(openssl rand -hex 16)"
P12_DIR="$(mktemp -d)"; P12="$P12_DIR/signing.p12"; trap 'rm -rf "$P12_DIR"' EXIT
printf '%s' "$P12_BASE64" | base64 --decode > "$P12"
security create-keychain -p "$KC_PASS" "$KC"
security set-keychain-settings "$KC"
security unlock-keychain -p "$KC_PASS" "$KC"
security import "$P12" -f pkcs12 -k "$KC" -P "$P12_PASSWORD" -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple: -s -k "$KC_PASS" "$KC" >/dev/null
IDENTITY="Nekro Qantrip Release"
security find-identity -p codesigning "$KC" | grep -q "$IDENTITY"
if [ -n "${GITHUB_ENV:-}" ]; then
  { echo "SIGN_IDENTITY=$IDENTITY"; echo "SIGN_KEYCHAIN=$KC"; } >> "$GITHUB_ENV"
else
  echo "export SIGN_IDENTITY='$IDENTITY' SIGN_KEYCHAIN='$KC'"
fi
